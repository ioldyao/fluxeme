pub mod handlers;

use std::sync::{Arc, RwLock};

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::config::types::AppConfig;
use crate::provider::ProviderRegistry;
use crate::ratelimit::RateLimiter;
use crate::service::{AuthService, HealthService, RoutingService, UsageService};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub auth: Arc<AuthService>,
    pub routing: Arc<RoutingService>,
    pub providers: Arc<ProviderRegistry>,
    pub rate_limiter: Arc<RateLimiter>,
    pub usage: UsageService,
    pub db: Arc<crate::db::Database>,
    pub admin: Arc<crate::admin::AdminModule>,
    pub health: Arc<HealthService>,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/v1/chat/completions",
            axum::routing::post(handlers::chat_completions),
        )
        .route("/v1/messages", axum::routing::post(handlers::messages))
        .route("/v1/completions", axum::routing::post(handlers::completions))
        .route("/v1/embeddings", axum::routing::post(handlers::embeddings))
        .route("/tokenize", axum::routing::post(handlers::tokenize))
        .route("/detokenize", axum::routing::post(handlers::detokenize))
        .route("/v1/models", axum::routing::get(handlers::list_models))
        .route("/health", axum::routing::get(handlers::health))
        // admin API
        .merge(crate::admin::admin_routes())
        // static files for admin frontend
        .fallback_service(
            tower_http::services::ServeDir::new("web")
                .fallback(tower_http::services::ServeFile::new("web/index.html")),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
