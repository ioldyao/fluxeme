mod admin;
mod balancer;
mod config;
mod db;
mod domain;
mod provider;
mod ratelimit;
mod server;
mod service;
mod sso;

use std::sync::{Arc, RwLock};

use tracing_subscriber::EnvFilter;

use crate::admin::AdminModule;
use crate::config::loader;
use crate::db::Database;
use crate::provider::ProviderRegistry;
use crate::ratelimit::RateLimiter;
use crate::server::{build_router, AppState};
use crate::service::{AuthService, HealthService, RoutingService, UsageService};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("ai_gateway=info,tower_http=info")),
        )
        .init();

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
    let admin_username = raw_config.admin.username.clone();
    let db_path = raw_config.database.path.clone();
    let jwt_secret = loader::resolve_jwt_secret(&raw_config);
    let config = Arc::new(RwLock::new(raw_config));

    let db = Arc::new(Database::new(&db_path));

    // Initialize database
    if let Err(e) = db.migrate() {
        tracing::error!("Failed to initialize database: {}", e);
        std::process::exit(1);
    }

    // Seed from config YAML if database is empty
    if let Err(e) = loader::seed_from_config(&config_path, &db, &admin_username) {
        tracing::error!("Failed to seed database: {}", e);
        std::process::exit(1);
    }

    // Initialize services
    let auth = Arc::new(AuthService::new(db.clone()));
    let routing = Arc::new(RoutingService::new(db.clone()));
    let (usage, usage_handle) = UsageService::new(db.clone());
    let providers = Arc::new(ProviderRegistry::new());
    let rate_limiter = Arc::new(RateLimiter::new());
    rate_limiter.start_cleanup_task();
    let health = Arc::new(HealthService::new(db.clone()).expect("Failed to create HealthService"));
    let admin = Arc::new(AdminModule::new(&jwt_secret));

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

    let state = Arc::new(AppState {
        config,
        auth,
        routing,
        providers,
        rate_limiter,
        usage,
        db,
        admin,
        health,
        sso,
    });

    let app = build_router(state);

    tracing::info!("AI Gateway starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind address");

    axum::serve(listener, app).await.expect("Server error");

    usage_handle.abort();
}
