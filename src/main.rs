mod admin;
mod authz;
mod balancer;
mod cache;
mod ch_backend;
mod config;
mod crypto;
mod db;
mod domain;
mod gateway;
mod management;
mod observability;
mod provider;
mod ratelimit;
mod server;
mod service;
mod skill_runtime;
mod sso;

use crate::ch_backend::ClickHouseBackend;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use crate::admin::AdminModule;
use crate::authz::{AuthzModule, TeamAuthzModule};
use crate::cache::RedisCache;
use crate::config::loader;
use crate::db::Database;
use crate::provider::ProviderRegistry;
use crate::ratelimit::RateLimiter;
use crate::server::{build_router, AppState};
use crate::service::{
    AuthService, ContentFilterService, HealthProbeService, HealthService, OidcResourceServer,
    RoutingService, UsageService,
};

async fn migrate_endpoint_credentials(
    db: &Database,
    encryption_key: &str,
    previous_encryption_key: Option<&str>,
    legacy_jwt_secret: &str,
) -> Result<usize, String> {
    let channels = db
        .list_channels()
        .await
        .map_err(|e| format!("failed to list channels: {e}"))?;
    let mut migrated = 0usize;

    for channel in channels {
        for endpoint in channel.endpoints {
            if endpoint.api_key.is_empty() {
                continue;
            }
            let endpoint_id = endpoint.id.ok_or_else(|| {
                format!(
                    "channel '{}' contains an endpoint without a database id",
                    channel.id
                )
            })?;
            let mut fallback_keys = Vec::with_capacity(2);
            if let Some(previous) = previous_encryption_key {
                fallback_keys.push(previous);
            }
            fallback_keys.push(legacy_jwt_secret);
            let (plaintext, needs_migration) = crate::crypto::decrypt_for_migration(
                &endpoint.api_key,
                encryption_key,
                &fallback_keys,
            )
            .map_err(|e| {
                format!(
                    "cannot decrypt API key for channel '{}' endpoint {}: {}",
                    channel.id, endpoint_id, e
                )
            })?;

            if needs_migration {
                let encrypted = crate::crypto::encrypt_store(&plaintext, encryption_key);
                db.update_endpoint_api_key(endpoint_id, &encrypted)
                    .await
                    .map_err(|e| {
                        format!(
                            "failed to migrate API key for channel '{}' endpoint {}: {}",
                            channel.id, endpoint_id, e
                        )
                    })?;
                migrated += 1;
            }
        }
    }
    Ok(migrated)
}

#[tokio::main]
async fn main() {
    // Load .env early so OTLP_ENDPOINT is available for tracing setup.
    dotenvy::dotenv().ok();

    // Initialise tracing subscriber (fmt + optional OTLP layer).
    let _otlp_provider =
        // Crate name is `fluxeme` (Cargo.toml). The old "ai_gateway=info" target
        // matched no module, so all fluxeme::* info/warn logs were silently
        // dropped and only ERROR-level events survived. Keep tower_http for the
        // TraceLayer lines; RUST_LOG overrides everything when set.
        crate::observability::trace::init_subscriber("fluxeme=info,tower_http=info", "fluxeme");

    let config_path =
        std::env::var("GATEWAY_CONFIG").unwrap_or_else(|_| "config/config.yaml".to_string());

    // Load config (server settings only)
    let raw_config = match loader::load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to load config: {}", e);
            std::process::exit(1);
        }
    };

    let addr = format!("{}:{}", raw_config.server.host, raw_config.server.port);

    let pg_url = if raw_config.database.pg_url.is_empty() {
        let user = std::env::var("DB_USER").unwrap_or_else(|_| "postgres".to_string());
        let password = std::env::var("DB_PASSWORD").unwrap_or_else(|_| {
            tracing::error!("DB_PASSWORD must be set when database.pg_url is empty");
            std::process::exit(1);
        });
        let db_name = std::env::var("DB_NAME").unwrap_or_else(|_| "aigateway".to_string());
        let host = std::env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = std::env::var("DB_PORT").unwrap_or_else(|_| "5432".to_string());
        format!(
            "postgres://{}:{}@{}:{}/{}",
            user, password, host, port, db_name
        )
    } else {
        raw_config.database.pg_url.clone()
    };
    let jwt_secret = loader::resolve_jwt_secret(&raw_config);
    let encryption_key = loader::resolve_encryption_key(&raw_config);
    let previous_encryption_key = loader::resolve_previous_encryption_key(&raw_config);
    if encryption_key == jwt_secret {
        panic!("CRITICAL: GATEWAY_ENCRYPTION_KEY must be different from GATEWAY_JWT_SECRET");
    }
    let config = Arc::new(RwLock::new(raw_config));

    let db = Arc::new(Database::new(&pg_url).await);

    // Initialize database
    if let Err(e) = db.migrate().await {
        tracing::error!("Failed to initialize database: {}", e);
        std::process::exit(1);
    }

    // SkillHub 控制面子系统：自洽子系统，独立 schema 迁移，复用 PG 连接池
    // （业务数据归属 PostgreSQL）。技能包落盘 SKILLHUB_ARTIFACT_DIR（默认
    // data/skills），将来多实例可换对象存储实现。
    let skillhub = Arc::new(fluxeme_skillhub::SkillHubModule::new(
        db.pg_pool().clone(),
        std::env::var("SKILLHUB_ARTIFACT_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("data/skills")),
    ));
    if let Err(e) = skillhub.migrate().await {
        tracing::error!("Failed to initialize skillhub: {}", e);
        std::process::exit(1);
    }

    // Seed from config YAML if database is empty
    if let Err(e) = loader::seed_from_config(&config_path, &db).await {
        tracing::error!("Failed to seed database: {}", e);
        std::process::exit(1);
    }

    match migrate_endpoint_credentials(
        &db,
        &encryption_key,
        previous_encryption_key.as_deref(),
        &jwt_secret,
    )
    .await
    {
        Ok(0) => {}
        Ok(count) => tracing::info!(
            "Migrated {} endpoint credential(s) to the independent encryption key",
            count
        ),
        Err(e) => {
            tracing::error!("Endpoint credential migration failed: {}", e);
            std::process::exit(1);
        }
    }

    // Load runtime gateway config (timeouts, etc.)
    let gateway_config = Arc::new(RwLock::new(
        db.get_gateway_config().await.unwrap_or_default(),
    ));

    // Redis is mandatory: billing backlog, observability events, distributed
    // rate limiting, and cross-instance event delivery all depend on it.
    let redis_config = config.read().unwrap().redis.clone();
    if !redis_config.enabled {
        tracing::error!("Redis is mandatory; set redis.enabled=true");
        std::process::exit(1);
    }
    let cache_ttl = gateway_config.read().unwrap().cache_ttl_secs;
    let cache = Arc::new(match RedisCache::new(&redis_config.url, cache_ttl).await {
        Ok(c) => {
            tracing::info!("Redis connected");
            c
        }
        Err(e) => {
            tracing::error!("Failed to connect to mandatory Redis: {}", e);
            std::process::exit(1);
        }
    });

    // Initialize services
    // Reclaim only expired `reserved` requests. The DB transaction in the
    // reclaimer releases wallet holds and package reserved_units atomically.
    tokio::spawn(crate::service::token_reservation::run_expiry_reclaimer(
        db.clone(),
    ));
    tracing::info!(
        interval_secs = 10,
        "starting token settlement recovery worker"
    );
    match db
        .recover_token_settlement_receivables(100, "startup-recovery-probe")
        .await
    {
        Ok(recovered) => tracing::info!(
            recovered,
            "startup token settlement recovery probe completed"
        ),
        Err(error) => tracing::error!(%error, "startup token settlement recovery probe failed"),
    }
    tokio::spawn(crate::service::token_reservation::run_receivable_recovery(
        db.clone(),
    ));
    let auth = Arc::new(AuthService::new(db.clone()).await);
    let routing = Arc::new(
        RoutingService::new(db.clone(), &encryption_key)
            .await
            .expect("Failed to initialize routing credentials"),
    );
    let providers = Arc::new(ProviderRegistry::new());
    let rate_limiter = Arc::new(RateLimiter::new(cache.clone()));
    let health = Arc::new(
        HealthService::new(db.clone(), &encryption_key).expect("Failed to create HealthService"),
    );
    let admin = Arc::new(AdminModule::new(
        &jwt_secret,
        &encryption_key,
        db.clone(),
        cache.clone(),
    ));

    let sso = Arc::new(sso::SsoModule::new(&encryption_key, db.clone()).await);
    if sso.is_enabled() {
        tracing::info!("SSO enabled with {} provider(s)", sso.providers().len());
    }

    // OAuth2 Resource Server (Mode 2): accept access tokens issued by a
    // trusted IdP (the enabled SSO configs) in addition to gateway API keys.
    let oidc = Arc::new(OidcResourceServer::new());
    oidc.refresh(&sso.providers()).await;
    auth.attach_oidc(oidc.clone());
    // Also let user-facing /api/* endpoints accept external tokens (Mode 2).
    admin.attach_oidc(oidc.clone());
    // Configurable expected `aud` (strict Mode 2), controlled by the admin API
    // /api/settings/oidc-audience. None/empty = audience not checked.
    let expected_aud = db
        .get_setting("oidc_expected_audience")
        .await
        .ok()
        .flatten();
    oidc.set_expected_audience(expected_aud);
    if oidc.is_trusting_any() {
        tracing::info!(
            "OIDC resource server trusting {} issuer(s)",
            sso.providers().len()
        );
    }

    // Load allow_private_ips setting from DB (default: true)
    let allow_private = db.get_setting("allow_private_ips").await.ok().flatten();
    provider::set_allow_private_ips(allow_private.as_deref() != Some("false"));

    // Unique instance ID for multi-instance ops (logs, health probes)
    let instance_id = std::env::var("INSTANCE_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let raw = uuid::Uuid::new_v4().simple().to_string();
            raw[..12].to_string()
        });

    // Event bus for real-time observability (WebSocket push to admin UI).
    // Bridged to Redis pub/sub so events fan out across instances.
    let event_bus =
        observability::event_bus::EventBus::new(8192, cache.clone(), instance_id.clone());

    // Remote event subscriber: relays events from other instances into the
    // local bus so WebSocket clients here see all gateway traffic.
    {
        let bus = event_bus.clone();
        let redis = cache.clone();
        tokio::spawn(async move {
            let mut pubsub = match redis.subscribe(observability::event_bus::BUS_CHANNEL).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Failed to subscribe to event bus channel: {e}");
                    return;
                }
            };
            let mut stream = pubsub.on_message();
            use futures::StreamExt;
            while let Some(msg) = stream.next().await {
                let payload: String = match msg.get_payload() {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("Event bus payload error: {e}");
                        continue;
                    }
                };
                match serde_json::from_str::<observability::event_bus::RemoteEnvelope>(&payload) {
                    Ok(envelope) => bus.inject_remote(envelope),
                    Err(e) => tracing::warn!("Event bus envelope parse error: {e}"),
                }
            }
        });
    }

    // ClickHouse backend is mandatory for observability.
    let (clickhouse_cfg, ch_retention_days) = {
        let cfg = config.read().unwrap();
        (cfg.database.clickhouse.clone(), cfg.database.retention_days)
    };
    let ch = match ClickHouseBackend::new(&clickhouse_cfg).await {
        Ok(Some(ch)) => ch,
        Ok(None) => {
            tracing::error!("ClickHouse is mandatory; configure database.clickhouse.host");
            std::process::exit(1);
        }
        Err(e) => {
            tracing::error!("Failed to connect to mandatory ClickHouse: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = ch.migrate(ch_retention_days as u32).await {
        tracing::error!("ClickHouse migration failed: {e}");
        std::process::exit(1);
    }

    // Initialize usage service with billing workers (ClickHouse is now decoupled via Redis Stream)
    let flow_tracker =
        crate::observability::flow_tracker::FlowTracker::new(cache.clone(), instance_id.clone());
    let (usage, usage_handles) = UsageService::new(
        db.clone(),
        cache.clone(),
        event_bus.clone(),
        flow_tracker.clone(),
    );

    // Billing backlog drain — reads from Redis Stream and retries billing
    {
        let cache = cache.clone();
        let db = db.clone();
        tokio::spawn(crate::cache::start_billing_backlog_drain(cache, db));
    }

    // Obs consumer — reads Redis Stream obs:events → batch writes ClickHouse
    // Decouples CH availability from the gateway and PG.
    {
        let ch = ch.clone();
        let cache = cache.clone();
        let db = db.clone();
        tokio::spawn(crate::cache::start_obs_consumer(Some(ch), cache, db));
    }

    // Periodic inspection task: refresh Redis gate status from PostgreSQL.
    // Redis is mandatory, so no local gate cache is maintained.
    {
        let db = db.clone();
        let cache = cache.clone();
        tokio::spawn(async move {
            const PAGE_SIZE: usize = 100;
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                let mut offset = 0usize;
                loop {
                    let page = match db.get_balances_page(PAGE_SIZE, offset).await {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::warn!("Inspection: failed to read balances page: {}", e);
                            break;
                        }
                    };
                    if page.is_empty() {
                        break;
                    }
                    for (user_id, balance, frozen) in &page {
                        let status = crate::cache::compute_gate_status(*balance, *frozen);
                        if let Err(e) = cache.set_gate_and_balance(user_id, status, *balance).await
                        {
                            tracing::warn!(user_id, "Inspection: failed to update Redis: {}", e);
                        }
                    }
                    offset += PAGE_SIZE;
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        });
    }

    // Initialize Casbin authorization enforcer (DB-backed policies)
    let authz = Arc::new(
        AuthzModule::new()
            .await
            .expect("Failed to initialize Casbin authorization module"),
    );
    authz
        .seed_defaults(&db)
        .await
        .expect("Failed to seed Casbin policies");
    authz
        .reload(&db)
        .await
        .expect("Failed to load Casbin policies from DB");

    // Initialize team-scoped Casbin enforcer (domain-aware team RBAC).
    let team_authz = Arc::new(
        TeamAuthzModule::new()
            .await
            .expect("Failed to initialize team Casbin module"),
    );
    // Rebuild team role bindings from the DB so team permissions survive a
    // restart (the enforcer is in-memory).
    team_authz.reload_all(&db).await;

    // Initialize content filter service
    let content_filter = Arc::new(ContentFilterService::new(db.clone()).await);

    // Initialize health probe service
    let health_probe = Arc::new(HealthProbeService::new(
        db.clone(),
        providers.clone(),
        routing.clone(),
        Some(ch.clone()),
        cache.clone(),
        instance_id.clone(),
    ));

    // Automatic model health probes: probe all channel endpoints of every
    // model on an admin-configurable interval (default 60s, stored in
    // balancer_settings key "probe_interval_secs", clamped 10..=3600).
    // The interval is re-read each cycle so changes apply without a
    // restart. Only Open binding endpoints are probed for recovery; healthy
    // endpoints remain available to traffic without periodic synthetic calls.
    {
        let db = db.clone();
        let health_probe = health_probe.clone();
        tokio::spawn(async move {
            loop {
                let interval_secs = db
                    .get_setting("probe_interval_secs")
                    .await
                    .ok()
                    .flatten()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(60)
                    .clamp(10, 3600);
                tokio::time::sleep(Duration::from_secs(interval_secs)).await;
                if let Err(e) = health_probe.probe_open_bindings().await {
                    tracing::warn!("Auto probe failed: {}", e);
                }
            }
        });
    }

    // Skill Runtime 数据面子系统：只依赖 contract Port（目录/鉴权/计量），
    // 不 import skillhub 代码。poller 后台消费 outbox 任务部署端点。
    let skill_authorizer = Arc::new(crate::skill_runtime::SkillKeyAuthorizer::new(db.clone()));
    let skill_meter = Arc::new(crate::skill_runtime::SkillRuntimeMeter::new(Some(
        ch.clone(),
    )));
    let skill_backing = Arc::new(fluxeme_skill_backing::SkillBackingModule::new(
        db.pg_pool().clone(),
        skillhub.clone(),
        skill_authorizer,
        skill_meter,
    ));
    if let Err(e) = skill_backing.migrate().await {
        tracing::error!("Failed to initialize skill-backing: {}", e);
        std::process::exit(1);
    }
    {
        let backing = skill_backing.clone();
        tokio::spawn(async move { backing.run_poller().await });
    }

    let state = Arc::new(AppState {
        config,
        auth,
        routing,
        providers,
        rate_limiter,
        usage,
        db,
        skillhub,
        skill_backing,
        admin,
        authz,
        team_authz,
        health,
        sso,
        oidc,
        gateway_config,
        cache,
        content_filter,
        health_probe,
        event_bus: event_bus.clone(),
        flow_tracker,
        ch: Some(ch),
        instance_id: instance_id.clone(),
    });

    // Cross-instance config invalidation: poll config_version; when it
    // changes (bumped by an admin mutation on any instance), reload the
    // in-memory caches so all instances converge.
    {
        let poll_state = state.clone();
        tokio::spawn(async move {
            // Initialize to current version so we don't reload on startup
            let mut last_version = poll_state
                .db
                .get_setting("config_version")
                .await
                .ok()
                .flatten();
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let current = match poll_state.db.get_setting("config_version").await {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("config_version poll failed: {}", e);
                        continue;
                    }
                };
                if current != last_version {
                    if let Err(e) = poll_state.routing.reload().await {
                        tracing::error!("Routing cache reload failed: {}", e);
                        continue;
                    }
                    last_version = current;
                    poll_state.auth.reload().await;
                    poll_state.content_filter.reload().await;
                    poll_state.oidc.refresh(&poll_state.sso.providers()).await;
                    let expected_aud = poll_state
                        .db
                        .get_setting("oidc_expected_audience")
                        .await
                        .ok()
                        .flatten();
                    poll_state.oidc.set_expected_audience(expected_aud);
                    let _ = poll_state.authz.reload(&poll_state.db).await;
                    tracing::debug!("Reloaded in-memory caches after config_version change");
                }
            }
        });
    }

    let app = build_router(state);

    tracing::info!(instance_id = %instance_id, "Fluxeme AI Gateway starting on {}", addr);

    use std::net::{IpAddr, SocketAddr};
    use tokio::net::TcpSocket;

    let addr: SocketAddr = addr.parse().expect("Invalid bind address");
    let socket = match addr.ip() {
        IpAddr::V4(_) => TcpSocket::new_v4(),
        IpAddr::V6(_) => TcpSocket::new_v6(),
    }
    .expect("Failed to create TcpSocket");
    socket
        .set_reuseaddr(true)
        .expect("Failed to set SO_REUSEADDR");
    socket.bind(addr).expect("Failed to bind address");
    let listener = socket.listen(32768).expect("Failed to listen");

    // Graceful shutdown: on SIGTERM/SIGINT, stop accepting new connections,
    // drain in-flight requests, then exit. Required for rolling deploys.
    async fn shutdown_signal() {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => { tracing::info!("SIGINT received, draining connections..."); }
            _ = terminate => { tracing::info!("SIGTERM received, draining connections..."); }
        }
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("Server error");

    for h in usage_handles {
        h.abort();
    }

    tracing::info!(instance_id = %instance_id, "Fluxeme AI Gateway stopped");
}
