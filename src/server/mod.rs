pub mod handlers;
pub mod ws;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::http::HeaderValue;
use tokio::sync::RwLock as AsyncRwLock;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{CorsLayer, AllowOrigin};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::authz::AuthzModule;
use crate::cache::{GateStatus, RedisCache};
use crate::config::types::{AppConfig, GatewayRuntimeConfig};
use crate::provider::ProviderRegistry;
use crate::ratelimit::RateLimiter;
use crate::service::{AuthService, ContentFilterService, HealthProbeService, HealthService, RoutingService, UsageService};
use crate::sso::SsoModule;

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
    pub authz: Arc<AuthzModule>,
    pub health: Arc<HealthService>,
    pub sso: Arc<SsoModule>,
    /// Runtime-adjustable timeout config. Read on every request, updated by
    /// PUT /admin/api/gateway/config.  Uses RwLock so writes propagate instantly
    /// (single-instance; multi-instance deployments would need a refresh loop).
    pub gateway_config: Arc<RwLock<GatewayRuntimeConfig>>,
    pub cache: Arc<RedisCache>,
    /// In-memory gate-status cache used as second fallback when Redis is
    /// unavailable (avoids SQLite mutex contention during Redis outages).
    pub gate_cache: Arc<AsyncRwLock<HashMap<String, GateStatus>>>,
    /// Content filter service for request/response moderation.
    pub content_filter: Arc<ContentFilterService>,
    /// Health probe service for model channel health checks (DB-persisted).
    pub health_probe: Arc<HealthProbeService>,
    /// Event bus for real-time request path events (WebSocket push).
    pub event_bus: crate::observability::event_bus::EventBus,
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let allowed_origins: Vec<HeaderValue> = state
        .config
        .read()
        .unwrap()
        .cors
        .allowed_origins
        .iter()
        .map(|o| o.parse().expect("Invalid origin URL in cors.allowed_origins"))
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::HeaderName::from_static("x-api-key"),
        ]);

    let security_headers = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_CONTENT_TYPE_OPTIONS,
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::X_FRAME_OPTIONS,
            axum::http::HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::header::CONTENT_SECURITY_POLICY,
            axum::http::HeaderValue::from_static(
                "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; form-action 'self'; frame-ancestors 'none'",
            ),
        ));

    Router::new()
        .route(
            "/v1/chat/completions",
            axum::routing::post(handlers::chat_completions),
        )
        .route("/v1/messages", axum::routing::post(handlers::messages))
        .route(
            "/v1/completions",
            axum::routing::post(handlers::completions),
        )
        .route("/v1/embeddings", axum::routing::post(handlers::embeddings))
        .route(
            "/v1/messages/batches",
            axum::routing::post(handlers::batches),
        )
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
        .layer(CompressionLayer::new())
        .layer(cors)
        .layer(security_headers)
        .with_state(state)
}
