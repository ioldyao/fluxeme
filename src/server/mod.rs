pub mod handlers;
pub mod health;
pub mod ws;

use std::sync::{Arc, RwLock};

use axum::extract::DefaultBodyLimit;
use axum::http::HeaderValue;
use axum::response::Redirect;
use axum::routing::get;
use axum::Router;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

use crate::authz::{AuthzModule, TeamAuthzModule};
use crate::cache::RedisCache;
use crate::ch_backend::ClickHouseBackend;
use crate::config::types::{AppConfig, GatewayRuntimeConfig};
use crate::ratelimit::RateLimiter;
use crate::service::{
    AuthService, ContentFilterService, HealthProbeService, HealthService, OidcResourceServer,
    RoutingService,
};
use crate::sso::SsoModule;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub auth: Arc<AuthService>,
    pub routing: Arc<RoutingService>,
    /// Unified request scheduling service (route → dispatch → retry → usage).
    pub scheduler: Arc<crate::scheduler::SchedulerService>,
    pub rate_limiter: Arc<RateLimiter>,
    pub db: Arc<crate::db::Database>,
    /// SkillHub 控制面子系统（目录/版本/安装/包存储）。
    pub skillhub: Arc<fluxeme_skillhub::SkillHubModule>,
    /// Skill Runtime 数据面子系统（部署/鉴权/代理/计量）。
    pub skill_backing: Arc<fluxeme_skill_backing::SkillBackingModule>,
    pub admin: Arc<crate::admin::AdminModule>,
    pub authz: Arc<AuthzModule>,
    /// Team-scoped RBAC enforcer (domain-aware). Independent of `authz`.
    pub team_authz: Arc<TeamAuthzModule>,
    pub health: Arc<HealthService>,
    pub sso: Arc<SsoModule>,
    /// OAuth2 Resource Server (Mode 2): validates access tokens issued by a
    /// trusted IdP so `/v1/*` accepts them in place of gateway API keys.
    pub oidc: Arc<OidcResourceServer>,
    /// Runtime-adjustable timeout config. Read on every request, updated by
    /// PUT /admin/api/gateway/config.  Uses RwLock so writes propagate instantly
    /// (single-instance; multi-instance deployments would need a refresh loop).
    pub gateway_config: Arc<RwLock<GatewayRuntimeConfig>>,
    pub cache: Arc<RedisCache>,
    /// Content filter service for request/response moderation.
    pub content_filter: Arc<ContentFilterService>,
    /// Health probe service for model channel health checks (DB-persisted).
    pub health_probe: Arc<HealthProbeService>,
    /// Event bus for real-time request path events (WebSocket push).
    pub event_bus: crate::observability::event_bus::EventBus,
    /// In-memory flow lifecycle tracker for realtime in-flight/upstream metrics.
    pub flow_tracker: crate::observability::flow_tracker::FlowTracker,
    /// ClickHouse backend for observability data. Required at startup.
    pub ch: Option<Arc<ClickHouseBackend>>,
    /// Unique identifier for this instance (INSTANCE_ID env or generated).
    /// Used in logs and health probe responses for multi-instance ops.
    pub instance_id: String,
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
        ])
        .allow_credentials(true);

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
            "/apigw/{*rest}",
            axum::routing::get(crate::gateway::gateway_proxy)
                .post(crate::gateway::gateway_proxy)
                .put(crate::gateway::gateway_proxy)
                .patch(crate::gateway::gateway_proxy)
                .delete(crate::gateway::gateway_proxy)
                .head(crate::gateway::gateway_proxy)
                .options(crate::gateway::gateway_proxy),
        )
        // Importers/callers: this router is the public HTTP entrypoint for gateway APIs.
        // Affected API: adds POST /v1/messages/count_tokens. Data schema: Anthropic
        // request body and response shape {"input_tokens": number}. User instruction:
        // "要，添加`/v1/messages/count_tokens`的端点支持".
        .route(
            "/v1/messages/count_tokens",
            axum::routing::post(handlers::messages_count_tokens),
        )
        .route(
            "/v1/completions",
            axum::routing::post(handlers::completions),
        )
        .route("/v1/responses", axum::routing::post(handlers::responses))
        // Importers/callers: this router is the public HTTP entrypoint for gateway APIs.
        // Affected API: adds POST /responses/input_tokens and relays upstream to
        // POST /v1/responses/input_tokens. Data schema: OpenAI Responses request body
        // and response shape {"object":"response.input_tokens","input_tokens":
        // number}. User instruction: "openai的提供商层，也添加这个openai的端点`POST/responses/input_tokens`".
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
        // Liveness/readiness probes for load balancers (no auth, no dependency)
        .route("/healthz", axum::routing::get(health::liveness))
        .route("/readyz", axum::routing::get(health::readiness))
        // Dedicated external management API. It has its own mk-* only
        // authentication layer and is intentionally separate from /api.
        .nest("/management/v1", crate::management::routes(state.clone()))
        // admin API
        .merge(crate::admin::admin_routes())
        // static files — SPA routing
        // nest strips /admin/ prefix before passing to ServeDir,
        // so /admin/assets/foo.js → web/admin/assets/foo.js
        .route("/admin", get(|| async { Redirect::permanent("/admin/") }))
        .nest(
            "/admin",
            Router::new().fallback_service(
                ServeDir::new("web/admin").fallback(ServeFile::new("web/admin/index.html")),
            ),
        )
        // everything else → user SPA
        .fallback_service(ServeDir::new("web").fallback(ServeFile::new("web/index.html")))
        // Remove the request body limit: LLM requests carry base64 images and
        // long conversation context. Axum's default is 2MB (via Json extractor),
        // which Claude Code hits with a few screenshots → 413 "request too large".
        .layer(DefaultBodyLimit::disable())
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors)
        .layer(security_headers)
        .with_state(state)
}
