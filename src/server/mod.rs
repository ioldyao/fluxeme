pub mod handlers;
pub mod health;
pub mod ws;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use tokio::sync::RwLock as AsyncRwLock;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::authz::AuthzModule;
use crate::cache::{GateStatus, RedisCache};
use crate::ch_backend::ClickHouseBackend;
use crate::config::types::{AppConfig, GatewayRuntimeConfig};
use crate::provider::ProviderRegistry;
use crate::ratelimit::RateLimiter;
use crate::service::{
    AuthService, ContentFilterService, HealthProbeService, HealthService, RoutingService,
    UsageService,
};
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
    pub gateway_config: Arc<RwLock<GatewayRuntimeConfig>>,
    pub cache: Arc<RedisCache>,
    pub gate_cache: Arc<AsyncRwLock<HashMap<String, GateStatus>>>,
    pub content_filter: Arc<ContentFilterService>,
    pub health_probe: Arc<HealthProbeService>,
    pub event_bus: crate::observability::event_bus::EventBus,
    pub ch: Option<Arc<ClickHouseBackend>>,
    pub instance_id: String,
}

async fn frontend_fallback(req: Request<Body>) -> Response {
    let path = req.uri().path().to_string();

    if path.starts_with("/admin") {
        if path == "/admin" || path == "/admin/" {
            return serve_file("web/admin/index.html").await;
        }

        let trimmed = path.trim_start_matches('/');
        let candidate = format!("web/{trimmed}");
        if tokio::fs::metadata(&candidate).await.is_ok() {
            return serve_file(&candidate).await;
        }

        return serve_file("web/admin/index.html").await;
    }

    if path == "/" {
        return serve_file("web/portal/index.html").await;
    }

    let trimmed = path.trim_start_matches('/');
    let candidate = format!("web/portal/{trimmed}");
    if tokio::fs::metadata(&candidate).await.is_ok() {
        return serve_file(&candidate).await;
    }

    serve_file("web/portal/index.html").await
}

async fn serve_file(path: &str) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let mime = mime_for_path(path);
            Response::builder()
                .status(StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, mime)
                .body(Body::from(bytes))
                .unwrap_or_else(|_| Response::new(Body::empty()))
        }
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap_or_else(|_| Response::new(Body::empty())),
    }
}

fn mime_for_path(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".woff") {
        "font/woff"
    } else if path.ends_with(".ttf") {
        "font/ttf"
    } else {
        "application/octet-stream"
    }
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let allowed_origins: Vec<HeaderValue> = state
        .config
        .read()
        .unwrap()
        .cors
        .allowed_origins
        .iter()
        .map(|o| {
            o.parse()
                .expect("Invalid origin URL in cors.allowed_origins")
        })
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
            "/v1/messages/count_tokens",
            axum::routing::post(handlers::messages_count_tokens),
        )
        .route(
            "/v1/completions",
            axum::routing::post(handlers::completions),
        )
        .route(
            "/responses/input_tokens",
            axum::routing::post(handlers::responses_input_tokens),
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
        .route("/healthz", axum::routing::get(health::liveness))
        .route("/readyz", axum::routing::get(health::readiness))
        .merge(crate::admin::admin_routes())
        .fallback(axum::routing::any(frontend_fallback))
        .layer(DefaultBodyLimit::disable())
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors)
        .layer(security_headers)
        .with_state(state)
}
