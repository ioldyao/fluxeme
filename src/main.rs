mod admin;
mod authz;
mod balancer;
mod cache;
mod config;
mod db;
mod domain;
mod provider;
mod ratelimit;
mod server;
mod service;
mod sso;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use tokio::sync::RwLock as AsyncRwLock;
use tracing_subscriber::EnvFilter;

use crate::cache::GateStatus;
use crate::server::PerUserSemaphore;

use crate::admin::AdminModule;
use crate::authz::AuthzModule;
use crate::cache::RedisCache;
use crate::config::loader;
use crate::db::Database;
use crate::provider::ProviderRegistry;
use crate::ratelimit::RateLimiter;
use crate::server::{build_router, AppState};
use crate::service::{AuthService, ContentFilterService, HealthProbeService, HealthService, RoutingService, UsageService};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("ai_gateway=info,tower_http=info")),
        )
        .init();

    // Load .env file if present (for env var references in config like ${GATEWAY_JWT_SECRET})
    dotenvy::dotenv().ok();

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

    let db_type = raw_config.database.db_type.clone();
    let db_path = raw_config.database.path.clone();
    let pg_url = if raw_config.database.pg_url.is_empty() {
        let user = std::env::var("DB_USER").unwrap_or_else(|_| "postgres".to_string());
        let password = std::env::var("DB_PASSWORD").unwrap_or_else(|_| "postgres123".to_string());
        let db_name = std::env::var("DB_NAME").unwrap_or_else(|_| "aigateway".to_string());
        let host = std::env::var("DB_HOST").unwrap_or_else(|_| "localhost".to_string());
        let port = std::env::var("DB_PORT").unwrap_or_else(|_| "5432".to_string());
        format!("postgres://{}:{}@{}:{}/{}", user, password, host, port, db_name)
    } else {
        raw_config.database.pg_url.clone()
    };
    let jwt_secret = loader::resolve_jwt_secret(&raw_config);
    let config = Arc::new(RwLock::new(raw_config));

    let db = Arc::new(Database::new(&db_type, &db_path, &pg_url).await);

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

    // Initialize services
    let auth = Arc::new(AuthService::new(db.clone()).await);
    let routing = Arc::new(RoutingService::new(db.clone()).await);
    let providers = Arc::new(ProviderRegistry::new());
    let rate_limiter = Arc::new(RateLimiter::new());
    rate_limiter.start_cleanup_task();
    let health = Arc::new(HealthService::new(db.clone()).expect("Failed to create HealthService"));
    let admin = Arc::new(AdminModule::new(&jwt_secret, db.clone()));

    let sso = Arc::new(
        match sso::SsoModule::new(&config.read().unwrap().sso).await {
            Ok(m) => {
                let cfg = config.read().unwrap();
                if cfg.sso.enabled {
                    tracing::info!(
                        "SSO enabled: provider={}, issuer={}",
                        cfg.sso.provider_name,
                        cfg.sso.issuer_url
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
                    tracing::info!("Purged {} usage log records older than {} days", count, days)
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
    let cache = Arc::new(
        if config.read().unwrap().cache.enabled {
            let ttl = gateway_config.read().unwrap().cache_ttl_secs;
            match RedisCache::new(&config.read().unwrap().cache.redis_url, ttl).await {
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
        },
    );

    // Initialize usage service (background writer for usage logs + billing deductions)
    let (usage, usage_handle) = UsageService::new(db.clone(), cache.clone());

    // In-memory gate cache (populated by inspection, read by handler when Redis is down)
    let gate_cache: Arc<AsyncRwLock<HashMap<String, GateStatus>>> = Arc::new(AsyncRwLock::new(HashMap::new()));
    // Per-user concurrency limiter (caps in-flight requests per user to 5)
    let concurrency = Arc::new(PerUserSemaphore::new());

    // Periodic inspection task: sync user gate status from SQLite to Redis + local cache.
    // Uses pagination to avoid holding the SQLite mutex for too long.
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
                        if let Err(e) = cache.set_gate_and_balance(user_id, status, *balance).await {
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
                    // Brief yield between pages to reduce SQLite lock contention
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
    let health_probe = Arc::new(HealthProbeService::new(db.clone(), providers.clone(), routing.clone()));

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
        concurrency,
        content_filter,
        health_probe,
    });

    let app = build_router(state);

    tracing::info!("AI Gateway starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .expect("Server error");

    usage_handle.abort();
}
