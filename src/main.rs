mod admin;
mod authz;
mod balancer;
mod cache;
mod config;
mod crypto;
mod db;
mod domain;
mod observability;
mod provider;
mod ratelimit;
mod server;
mod service;
mod sso;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::RwLock as AsyncRwLock;

use crate::cache::GateStatus;

use crate::admin::AdminModule;
use crate::authz::AuthzModule;
use crate::cache::RedisCache;
use crate::config::loader;
use crate::db::Database;
use crate::provider::ProviderRegistry;
use crate::ratelimit::RateLimiter;
use crate::server::{build_router, AppState};
use crate::service::{
    AuthService, ContentFilterService, HealthProbeService, HealthService, RoutingService,
    UsageService,
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
    let _otlp_provider = crate::observability::trace::init_subscriber(
        "ai_gateway=info,tower_http=info",
        "ai-gateway",
    );

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

    // Initialize services
    let auth = Arc::new(AuthService::new(db.clone()).await);
    let routing = Arc::new(
        RoutingService::new(db.clone(), &encryption_key)
            .await
            .expect("Failed to initialize routing credentials"),
    );
    let providers = Arc::new(ProviderRegistry::new());
    let rate_limiter = Arc::new(RateLimiter::new());
    rate_limiter.start_cleanup_task();
    let health = Arc::new(
        HealthService::new(db.clone(), &encryption_key).expect("Failed to create HealthService"),
    );
    let admin = Arc::new(AdminModule::new(&jwt_secret, &encryption_key, db.clone()));

    let sso_config = config.read().unwrap().sso.clone();
    let sso = Arc::new(
        match sso::SsoModule::new(&sso_config, &encryption_key).await {
            Ok(m) => {
                if sso_config.enabled {
                    tracing::info!(
                        "SSO enabled: provider={}, issuer={}",
                        sso_config.provider_name,
                        sso_config.issuer_url
                    );
                }
                m
            }
            Err(e) => {
                tracing::error!("Failed to initialize SSO: {}", e);
                std::process::exit(1);
            }
        },
    );

    // Run usage log cleanup on startup
    {
        let days = config.read().unwrap().database.retention_days;
        if days > 0 {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
            let cutoff_str = cutoff.format("%Y-%m-%dT%H:%M:%S").to_string();
            match db.purge_usage_logs(&cutoff_str).await {
                Ok(count) => {
                    tracing::info!(
                        "Purged {} usage log records older than {} days",
                        count,
                        days
                    )
                }
                Err(e) => tracing::error!("Failed to purge usage logs: {}", e),
            }
        }
    }

    // Load allow_private_ips setting from DB (default: true)
    let allow_private = db.get_setting("allow_private_ips").await.ok().flatten();
    provider::set_allow_private_ips(allow_private.as_deref() != Some("false"));

    // Load runtime gateway config (timeouts, etc.)
    let gateway_config = Arc::new(RwLock::new(
        db.get_gateway_config().await.unwrap_or_default(),
    ));

    // Initialize Redis cache (noop when disabled)
    let cache_config = config.read().unwrap().cache.clone();
    let cache_ttl = gateway_config.read().unwrap().cache_ttl_secs;
    let cache = Arc::new(if cache_config.enabled {
        match RedisCache::new(&cache_config.redis_url, cache_ttl).await {
            Ok(c) => {
                tracing::info!("Redis cache enabled");
                c
            }
            Err(e) => {
                tracing::error!("Failed to connect to Redis: {}", e);
                RedisCache::noop()
            }
        }
    } else {
        RedisCache::noop()
    });

    // Event bus for real-time observability (WebSocket push to admin UI)
    let event_bus = observability::event_bus::EventBus::new(8192);

    // Initialize usage service (background writer for usage logs + billing deductions)
    let (usage, usage_handle) = UsageService::new(db.clone(), cache.clone(), event_bus.clone());

    // In-memory gate cache (populated by inspection, read by handler when Redis is down)
    let gate_cache: Arc<AsyncRwLock<HashMap<String, GateStatus>>> =
        Arc::new(AsyncRwLock::new(HashMap::new()));

    // Periodic inspection task: sync user gate status from PostgreSQL to Redis
    // and the local fallback cache. Pagination keeps each query bounded.
    {
        let db = db.clone();
        let cache = cache.clone();
        let gate_cache = gate_cache.clone();
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
                    // Batch-update both Redis and local cache for the page
                    let mut local_updates = Vec::with_capacity(page.len());
                    for (user_id, balance, frozen) in &page {
                        let status = crate::cache::compute_gate_status(*balance, *frozen);
                        if let Err(e) = cache.set_gate_and_balance(user_id, status, *balance).await
                        {
                            tracing::warn!(user_id, "Inspection: failed to update Redis: {}", e);
                        }
                        local_updates.push((user_id.clone(), status));
                    }
                    // Bulk-write local cache (single write lock acquisition per page)
                    {
                        let mut guard = gate_cache.write().await;
                        for (user_id, status) in &local_updates {
                            guard.insert(user_id.clone(), *status);
                        }
                    }
                    offset += PAGE_SIZE;
                    // Brief yield between pages to avoid monopolizing the executor.
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }
        });
    }

    // Initialize Casbin authorization enforcer
    let authz = Arc::new(
        AuthzModule::new()
            .await
            .expect("Failed to initialize Casbin authorization module"),
    );

    // Initialize content filter service
    let content_filter = Arc::new(ContentFilterService::new(db.clone()).await);

    // Initialize health probe service
    let health_probe = Arc::new(HealthProbeService::new(
        db.clone(),
        providers.clone(),
        routing.clone(),
    ));

    let state = Arc::new(AppState {
        config,
        auth,
        routing,
        providers,
        rate_limiter,
        usage,
        db,
        admin,
        authz,
        health,
        sso,
        gateway_config,
        cache,
        gate_cache,
        content_filter,
        health_probe,
        event_bus: event_bus.clone(),
    });

    let app = build_router(state);

    tracing::info!("AI Gateway starting on {}", addr);

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

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("Server error");

    usage_handle.abort();
}
