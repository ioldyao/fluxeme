use std::sync::Arc;

use axum::extract::Request;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use chrono::{Duration, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::authz::AuthzModule;
use crate::db::Database;
use crate::domain::user::{SessionInfo, USER_STATUS_ACTIVE};
use crate::ratelimit::RateLimiter;
use crate::server::AppState;

const SESSION_TTL_SECS: i64 = 24 * 3600;
pub(crate) const SESSION_COOKIE_NAME: &str = "session_token";
pub(crate) const HOST_SESSION_COOKIE_NAME: &str = "__Host-session_token";

// ── Sub-modules ────────────────────────────────────────────────────

pub mod announcements;
pub mod auth;
pub mod billing;
pub mod billing_groups;
pub mod channels;
pub mod dashboard;
pub mod health;
pub mod me;
pub mod models;
pub mod moderation;
pub mod policies;
pub mod routing;
pub mod rules;
pub mod settings;
pub mod skillhub;
pub mod sso;
pub mod teams;
pub mod token_packages;
pub mod usage;
pub mod users;
pub mod wallet;

// ── JWT claims ───────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct JwtClaims {
    /// user_id
    sub: String,
    /// user_name
    name: String,
    /// "admin" or "user"
    role: String,
    /// token version for session revocation
    #[serde(default)]
    ver: i64,
    /// expiration timestamp (UTC)
    exp: usize,
    /// issued at timestamp (UTC)
    iat: usize,
}

// ── Admin state ───────────────────────────────────────────────────

pub struct AdminModule {
    secret: String,
    encryption_key: String,
    rate_limiter: Arc<RateLimiter>,
    db: Arc<Database>,
    /// Optional OAuth2 Resource Server (Mode 2). Lets user-facing /api/*
    /// endpoints accept external IdP access tokens in addition to gateway
    /// session cookies (attached at startup).
    oidc: std::sync::RwLock<Option<Arc<crate::service::oidc::OidcResourceServer>>>,
}

impl AdminModule {
    /// `redis` is `Some` when the shared Redis cache is enabled — used for
    /// distributed rate limiting across instances.
    pub fn new(
        secret: &str,
        encryption_key: &str,
        db: Arc<Database>,
        redis: Arc<crate::cache::RedisCache>,
    ) -> Self {
        let rl = Arc::new(RateLimiter::new(redis));
        Self {
            secret: secret.to_string(),
            encryption_key: encryption_key.to_string(),
            rate_limiter: rl,
            db,
            oidc: std::sync::RwLock::new(None),
        }
    }

    /// Attach the OIDC Resource Server so user-facing /api/* endpoints accept
    /// external IdP access tokens (Mode 2) in addition to gateway sessions.
    pub fn attach_oidc(&self, oidc: Arc<crate::service::oidc::OidcResourceServer>) {
        *self.oidc.write().unwrap() = Some(oidc);
    }

    pub(crate) fn encode_token(&self, info: &SessionInfo) -> Result<String, AdminError> {
        let claims = JwtClaims {
            sub: info.user_id.clone(),
            name: info.user_name.clone(),
            role: info.role.clone(),
            ver: info.token_version,
            exp: (Utc::now() + Duration::seconds(SESSION_TTL_SECS)).timestamp() as usize,
            iat: Utc::now().timestamp() as usize,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| AdminError::internal(e.to_string()))
    }

    fn decode_token(&self, token: &str) -> Result<SessionInfo, AdminError> {
        let mut validation = Validation::default();
        validation.required_spec_claims =
            std::collections::HashSet::from(["sub".to_string(), "exp".to_string()]);
        let data = decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )
        .map_err(|e| {
            tracing::error!("JWT decode error: {}", e);
            AdminError::unauthorized("Invalid or expired session")
        })?;
        Ok(SessionInfo {
            user_id: data.claims.sub,
            user_name: data.claims.name,
            role: data.claims.role,
            token_version: data.claims.ver,
        })
    }
}

impl Clone for AdminModule {
    fn clone(&self) -> Self {
        Self {
            secret: self.secret.clone(),
            encryption_key: self.encryption_key.clone(),
            rate_limiter: Arc::clone(&self.rate_limiter),
            db: self.db.clone(),
            oidc: std::sync::RwLock::new(self.oidc.read().unwrap().as_ref().map(Arc::clone)),
        }
    }
}

fn validate_password(pw: &str) -> Result<(), AdminError> {
    if pw.len() < 8 {
        return Err(AdminError::bad_request(
            "Password must be at least 8 characters",
        ));
    }
    if !pw.chars().any(|c| c.is_uppercase()) {
        return Err(AdminError::bad_request(
            "Password must contain an uppercase letter",
        ));
    }
    if !pw.chars().any(|c| c.is_lowercase()) {
        return Err(AdminError::bad_request(
            "Password must contain a lowercase letter",
        ));
    }
    if !pw.chars().any(|c| c.is_ascii_digit()) {
        return Err(AdminError::bad_request("Password must contain a digit"));
    }
    Ok(())
}

// ── Auth helpers ──────────────────────────────────────────────────

fn request_host_with_port(headers: &HeaderMap) -> Option<String> {
    headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            // For IPv6 literal like [::1]:8080, strip brackets but keep port
            if value.starts_with('[') {
                if let Some(end) = value.find(']') {
                    return value[..=end].to_string();
                }
            }
            value.to_string()
        })
}

fn request_host(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            if let Some(stripped) = value.strip_prefix('[') {
                if let Some(end) = stripped.find(']') {
                    return &stripped[..end];
                }
            }
            value.split(':').next().unwrap_or(value)
        })
}

pub(crate) fn should_set_secure_cookie(headers: &HeaderMap) -> bool {
    if request_host(headers).is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1")) {
        return false;
    }

    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("https"))
        || headers
            .get("x-forwarded-scheme")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("https"))
        || headers
            .get("x-forwarded-ssl")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.eq_ignore_ascii_case("on"))
        || headers
            .get("origin")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("https://"))
        || headers
            .get("referer")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("https://"))
}

fn cookie_value_from_header(cookie_header: &str, name: &str) -> Option<String> {
    cookie_header
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(str::to_string))
}

pub(crate) fn extract_cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie_header| cookie_value_from_header(cookie_header, name))
}

fn extract_token(headers: &HeaderMap) -> Result<String, AdminError> {
    if let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
    {
        return Ok(token);
    }

    if let Some(token) = extract_cookie_value(headers, HOST_SESSION_COOKIE_NAME) {
        return Ok(token);
    }

    if let Some(token) = extract_cookie_value(headers, SESSION_COOKIE_NAME) {
        return Ok(token);
    }

    Err(AdminError::unauthorized("Missing or invalid admin token"))
}

fn request_origin(headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = headers.get("origin").and_then(|value| value.to_str().ok()) {
        return Some(origin.to_string());
    }

    headers
        .get("referer")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| url::Url::parse(value).ok())
        .map(|url| url.origin().ascii_serialization())
}

fn has_session_cookie(headers: &HeaderMap) -> bool {
    extract_cookie_value(headers, HOST_SESSION_COOKIE_NAME).is_some()
        || extract_cookie_value(headers, SESSION_COOKIE_NAME).is_some()
}

fn is_safe_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::OPTIONS | Method::TRACE
    )
}

async fn reject_cross_origin_cookie_requests(
    request: Request,
    next: Next,
) -> Result<Response, AdminError> {
    if is_safe_method(request.method())
        || request.headers().get("authorization").is_some()
        || !has_session_cookie(request.headers())
    {
        return Ok(next.run(request).await);
    }

    let host = request_host_with_port(request.headers());
    let origin = request_origin(request.headers());

    if let (Some(host), Some(origin)) = (host, origin) {
        let is_same_origin = origin.eq_ignore_ascii_case(&format!("https://{host}"))
            || origin.eq_ignore_ascii_case(&format!("http://{host}"));
        if is_same_origin {
            return Ok(next.run(request).await);
        }
    }

    Err(AdminError::forbidden("Cross-origin request blocked"))
}

pub(crate) async fn require_session_internal(
    admin: &AdminModule,
    headers: &HeaderMap,
) -> Result<SessionInfo, AdminError> {
    require_session(admin, headers).await
}

async fn require_session(
    admin: &AdminModule,
    headers: &HeaderMap,
) -> Result<SessionInfo, AdminError> {
    let token = extract_token(headers)?;
    let session = match admin.decode_token(&token) {
        Ok(s) => s,
        Err(_) => {
            // Mode 2: not a gateway session JWT — try it as an external IdP
            // access token (e.g. Keycloak) so the portal can fetch its own
            // data. Admin endpoints stay protected by the later role checks.
            return require_oidc_session(admin, &token).await;
        }
    };

    // Verify token_version against DB (session revocation enforcement)
    let db_user = admin
        .db
        .get_user(&session.user_id)
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?
        .ok_or_else(|| AdminError::unauthorized("User not found"))?;
    if db_user.token_version != session.token_version || db_user.status != USER_STATUS_ACTIVE {
        return Err(AdminError::unauthorized(
            "Session has been revoked. Please log in again.",
        ));
    }

    // Rate limit: 300 requests/minute per admin session to prevent abuse
    admin
        .rate_limiter
        .check_rpm(&format!("admin:{}", db_user.id), 300)
        .await
        .map_err(|_| AdminError::too_many_requests("Too many requests. Try again later."))?;

    Ok(SessionInfo {
        user_id: db_user.id,
        user_name: db_user.name,
        role: db_user.role,
        token_version: db_user.token_version,
    })
}

/// Mode 2 session resolution: validate an external IdP access token (RS256 +
/// issuer + expiry via JWKS) and map its `sub` to the gateway SSO user. Used
/// by user-facing /api/* endpoints so the portal can fetch its own data with a
/// Keycloak token, without a gateway session cookie.
async fn require_oidc_session(admin: &AdminModule, token: &str) -> Result<SessionInfo, AdminError> {
    let oidc = admin
        .oidc
        .read()
        .unwrap()
        .as_ref()
        .cloned()
        .ok_or_else(|| AdminError::unauthorized("External token auth is not configured"))?;

    let subject = oidc
        .validate(token)
        .map_err(|_| AdminError::unauthorized("Invalid or expired token"))?;

    let db_user = admin
        .db
        .get_user(&subject.user_id)
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?
        .ok_or_else(|| {
            AdminError::unauthorized("OIDC identity has no gateway account; sign in via SSO first")
        })?;
    if db_user.status != USER_STATUS_ACTIVE {
        return Err(AdminError::unauthorized("User account is suspended"));
    }

    // Same request budget as session-based requests.
    admin
        .rate_limiter
        .check_rpm(&format!("admin:{}", db_user.id), 300)
        .await
        .map_err(|_| AdminError::too_many_requests("Too many requests. Try again later."))?;

    Ok(SessionInfo {
        user_id: db_user.id,
        user_name: db_user.name,
        role: db_user.role,
        token_version: db_user.token_version,
    })
}

/// Bump the shared config_version so other gateway instances reload their
/// in-memory caches. Called after any admin mutation that changes routing /
/// auth / content_filter / authz state. Errors are logged, not propagated.
async fn notify_config_changed(state: &Arc<AppState>) {
    if let Err(e) = state.db.bump_config_version().await {
        tracing::warn!("Failed to bump config_version: {}", e);
    }
}

/// Check Casbin permission for the given session.
/// Returns 403 if the session's role lacks the permission.
async fn check_perm(
    authz: &AuthzModule,
    session: &SessionInfo,
    perm: &str,
) -> Result<(), AdminError> {
    if !authz.enforce(&session.role, perm).await {
        return Err(AdminError::forbidden("Insufficient permissions"));
    }
    Ok(())
}

// ── Error type ────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug)]
pub enum AdminError {
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Internal(String),
    BadRequest(String),
    Conflict(String),
    TooManyRequests(String),
}

impl AdminError {
    pub(crate) fn unauthorized(msg: impl Into<String>) -> Self {
        AdminError::Unauthorized(msg.into())
    }
    pub(crate) fn forbidden(msg: impl Into<String>) -> Self {
        AdminError::Forbidden(msg.into())
    }
    pub(crate) fn not_found(msg: impl Into<String>) -> Self {
        AdminError::NotFound(msg.into())
    }
    pub(crate) fn bad_request(msg: impl Into<String>) -> Self {
        AdminError::BadRequest(msg.into())
    }
    pub(crate) fn internal(msg: impl Into<String>) -> Self {
        AdminError::Internal(msg.into())
    }
    pub(crate) fn conflict(msg: impl Into<String>) -> Self {
        AdminError::Conflict(msg.into())
    }
    fn too_many_requests(msg: impl Into<String>) -> Self {
        AdminError::TooManyRequests(msg.into())
    }
}

impl IntoResponse for AdminError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            AdminError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            AdminError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            AdminError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AdminError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal server error".to_string(),
            ),
            AdminError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AdminError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            AdminError::TooManyRequests(msg) => (StatusCode::TOO_MANY_REQUESTS, msg),
        };
        let body = serde_json::json!({
            "error": message,
        });
        (status, Json(body)).into_response()
    }
}

/// Wrap a DB error: log the detail server-side and return a generic message.
fn db_err(e: crate::db::DbError) -> AdminError {
    tracing::error!("[admin] DB error: {}", e.0);
    AdminError::internal("Internal server error")
}

/// Wrap a DB error from a bad-request operation: pass through the error message.
fn db_err_bad_request(e: crate::db::DbError) -> AdminError {
    tracing::error!("[admin] DB bad-request error: {}", e.0);
    AdminError::bad_request(e.0)
}

/// Parse IANA timezone name (e.g. "Asia/Shanghai") and return the current
/// UTC offset in seconds. Falls back to 0 (UTC) on invalid input.
fn tz_offset_seconds(tz: Option<&str>) -> i64 {
    let name = match tz {
        Some(s) if !s.is_empty() => s,
        _ => return 0,
    };
    match name.parse::<Tz>() {
        Ok(tz) => {
            let now = Utc::now();
            tz.offset_from_utc_datetime(&now.naive_utc())
                .fix()
                .local_minus_utc() as i64
        }
        Err(_) => {
            tracing::warn!(tz = name, "Invalid timezone, falling back to UTC");
            0
        }
    }
}

/// Compute the `since` timestamp (UTC RFC3339) for "N days ago in the user's
/// local timezone". A request at 2026-07-11 00:30 Asia/Shanghai for 14 days
/// should include data from 2026-06-28 00:00 local (= 2026-06-27 16:00 UTC).
fn since_local_days_ago(days: i64, offset_seconds: i64) -> String {
    let now_utc = Utc::now();
    let local_offset = chrono::Duration::seconds(offset_seconds);
    let now_local = now_utc + local_offset;
    let since_local = now_local - Duration::days(days);
    let since_utc = since_local - local_offset;
    since_utc.format("%Y-%m-%dT%H:%M:%S").to_string()
}

// ── Router ────────────────────────────────────────────────────────

pub fn admin_routes() -> Router<Arc<crate::server::AppState>> {
    Router::new()
        .route("/api/login", axum::routing::post(auth::admin_login))
        .route("/api/logout", axum::routing::post(auth::admin_logout))
        .route("/api/auth/session", axum::routing::get(auth::auth_session))
        .route("/api/setup/status", axum::routing::get(auth::setup_status))
        .route(
            "/api/setup/register",
            axum::routing::post(auth::setup_register),
        )
        .route(
            "/api/sso/status",
            axum::routing::get(crate::sso::sso_status_handler),
        )
        .route(
            "/api/sso/login",
            axum::routing::get(crate::sso::sso_login_handler),
        )
        .route(
            "/api/sso/callback",
            axum::routing::get(crate::sso::sso_callback_handler),
        )
        .route(
            "/api/dashboard",
            axum::routing::get(dashboard::admin_dashboard),
        )
        .route(
            "/api/dashboard/aggregations",
            axum::routing::get(dashboard::dashboard_aggregations),
        )
        // Importers/callers: these routes are consumed by ui/src/api/dashboard.ts
        // from ui/src/pages/Dashboard.tsx. Affected APIs: GET /api/dashboard/self
        // and GET /api/dashboard/self/aggregations. Data schemas: SelfDashboardResp
        // { api_keys, total_requests } and DashboardAggregations. User instruction:
        // "`网关运行总览` 这个前端页面中，哪些还有计算全部用户的，统一修改只看当前个人用户的数据,admin登陆也只看自己的数据".
        .route(
            "/api/dashboard/self",
            axum::routing::get(dashboard::self_dashboard),
        )
        .route(
            "/api/dashboard/self/aggregations",
            axum::routing::get(dashboard::self_dashboard_aggregations),
        )
        // Current user
        .route("/api/me", axum::routing::get(me::get_my_session))
        .route(
            "/api/me/password",
            axum::routing::post(me::change_my_password),
        )
        .route(
            "/api/me/timezone",
            axum::routing::get(me::get_my_timezone).put(me::update_my_timezone),
        )
        .route(
            "/api/me/currency",
            axum::routing::get(me::get_my_currency).put(me::update_my_currency),
        )
        .route(
            "/api/me/keys",
            axum::routing::get(me::my_keys).post(me::create_my_key),
        )
        .route(
            "/api/me/keys/{key_val}",
            axum::routing::delete(me::delete_my_key)
                .patch(me::toggle_my_key)
                .put(me::update_my_key),
        )
        .route(
            "/api/me/permissions",
            axum::routing::get(me::my_permissions),
        )
        // My teams (self-service)
        .route("/api/teams", axum::routing::get(me::my_teams))
        .route(
            "/api/teams/{team_id}",
            axum::routing::get(me::my_team_detail),
        )
        .route(
            "/api/teams/{team_id}/members",
            axum::routing::get(me::my_team_members).post(me::add_my_team_member),
        )
        .route(
            "/api/teams/{team_id}/members/{user_id}",
            axum::routing::put(me::set_my_team_member_role).delete(me::remove_my_team_member),
        )
        .route(
            "/api/teams/{team_id}/wallet",
            axum::routing::get(me::my_team_wallet).post(me::credit_my_team_wallet),
        )
        .route(
            "/api/teams/{team_id}/wallet/transactions",
            axum::routing::get(me::my_team_wallet_transactions),
        )
        .route(
            "/api/teams/{team_id}/keys",
            axum::routing::get(me::my_team_api_keys).post(me::create_my_team_api_key),
        )
        .route(
            "/api/teams/{team_id}/keys/{key_val}",
            axum::routing::delete(me::delete_my_team_api_key),
        )
        .route(
            "/api/teams/{team_id}/rules",
            axum::routing::get(me::my_team_rules).post(me::create_my_team_rule),
        )
        .route(
            "/api/teams/{team_id}/rules/{rule_id}",
            axum::routing::delete(me::delete_my_team_rule),
        )
        // Admin team management
        .route(
            "/api/admin/teams",
            axum::routing::get(teams::list_all_teams).post(teams::create_team),
        )
        .route(
            "/api/admin/teams/{team_id}",
            axum::routing::get(teams::get_team_detail)
                .put(teams::update_team)
                .delete(teams::delete_team),
        )
        .route(
            "/api/admin/teams/{team_id}/members",
            axum::routing::get(teams::list_team_members).post(teams::add_team_member),
        )
        .route(
            "/api/admin/teams/{team_id}/members/{user_id}",
            axum::routing::put(teams::set_team_member_role).delete(teams::remove_team_member),
        )
        .route(
            "/api/admin/teams/{team_id}/wallet",
            axum::routing::get(teams::get_team_wallet_detail).post(teams::credit_team_wallet),
        )
        .route(
            "/api/admin/teams/{team_id}/wallet/transactions",
            axum::routing::get(teams::list_team_wallet_transactions),
        )
        // Users
        .route(
            "/api/users",
            axum::routing::get(users::list_users).post(users::create_user),
        )
        .route(
            "/api/users/{id}",
            axum::routing::get(users::get_user_detail)
                .put(users::update_user)
                .delete(users::delete_user),
        )
        .route(
            "/api/users/{id}/suspend",
            axum::routing::post(users::suspend_user),
        )
        .route(
            "/api/users/{id}/restore",
            axum::routing::post(users::restore_user),
        )
        // User API keys (admin)
        .route(
            "/api/users/{user_id}/keys",
            axum::routing::get(users::list_user_keys).post(users::create_user_key),
        )
        .route(
            "/api/users/{user_id}/keys/{key_val}",
            axum::routing::delete(users::delete_user_key)
                .patch(users::toggle_user_key)
                .put(users::update_user_key),
        )
        // Channels
        .route(
            "/api/channels",
            axum::routing::get(channels::list_channels).post(channels::create_channel),
        )
        .route(
            "/api/channels/{id}",
            axum::routing::put(channels::update_channel).delete(channels::delete_channel),
        )
        .route(
            "/api/channels/{id}/health",
            axum::routing::get(channels::get_channel_health),
        )
        .route(
            "/api/endpoints/{id}",
            axum::routing::patch(channels::toggle_endpoint),
        )
        // Models
        .route(
            "/api/models",
            axum::routing::get(models::list_models).post(models::create_model),
        )
        .route(
            "/api/models/public",
            axum::routing::get(models::list_public_models),
        )
        .route(
            "/api/models/{id}/publish",
            axum::routing::post(models::toggle_publish_model),
        )
        .route(
            "/api/models/{id}/pricing",
            axum::routing::patch(models::update_model_pricing),
        )
        .route(
            "/api/models/{id}/health-check",
            axum::routing::post(models::model_health_check),
        )
        .route(
            "/api/probe-results",
            axum::routing::get(models::list_probe_results),
        )
        .route(
            "/api/probe-results/recent",
            axum::routing::get(models::list_recent_probes),
        )
        .route(
            "/api/health/routing",
            axum::routing::get(routing::routing_health),
        )
        .route(
            "/api/health/recent-paths",
            axum::routing::get(routing::recent_request_paths),
        )
        .route(
            "/api/health/flow-metrics",
            axum::routing::get(routing::flow_metrics),
        )
        .route(
            "/api/models/{id}",
            axum::routing::put(models::update_model).delete(models::delete_model),
        )
        // Routing rules (system-level, admin)
        .route(
            "/api/rules",
            axum::routing::get(rules::list_rules).post(rules::create_rule),
        )
        .route(
            "/api/rules/{id}",
            axum::routing::put(rules::update_rule).delete(rules::delete_rule),
        )
        // User-level routing rules (self-service)
        .route(
            "/api/me/rules",
            axum::routing::get(me::list_my_rules).post(me::create_my_rule),
        )
        .route(
            "/api/me/rules/{id}",
            axum::routing::delete(me::delete_my_rule),
        )
        // Token resource packages
        .route(
            "/api/admin/token-packages",
            axum::routing::get(token_packages::list_plans).post(token_packages::create_plan),
        )
        .route(
            "/api/admin/token-packages/{id}",
            axum::routing::delete(token_packages::delete_plan),
        )
        .route(
            "/api/admin/token-packages/grants",
            axum::routing::get(token_packages::list_grants).post(token_packages::create_grant),
        )
        .route(
            "/api/admin/token-packages/grants/{id}",
            axum::routing::get(token_packages::get_grant),
        )
        .route(
            "/api/admin/token-packages/grants/{id}/revoke",
            axum::routing::post(token_packages::revoke_grant),
        )
        .route(
            "/api/me/token-packages",
            axum::routing::get(token_packages::list_my_grants),
        )
        // Usage
        .route("/api/usage", axum::routing::get(usage::get_usage))
        .route("/api/usage/daily", axum::routing::get(usage::daily_usage))
        .route(
            "/api/usage/aggregate",
            axum::routing::get(usage::usage_aggregate),
        )
        .route(
            "/api/usage/model-activity",
            axum::routing::get(usage::model_activity),
        )
        .route("/api/usage/funnel", axum::routing::get(usage::usage_funnel))
        .route("/api/me/usage", axum::routing::get(usage::get_my_usage))
        .route(
            "/api/me/usage/billing",
            axum::routing::get(usage::get_usage_billing),
        )
        .route(
            "/api/admin/usage/billing",
            axum::routing::get(usage::get_admin_usage_billing),
        )
        .route(
            "/api/me/usage/aggregate",
            axum::routing::get(usage::my_usage_aggregate),
        )
        .route(
            "/api/me/usage/model-activity",
            axum::routing::get(usage::my_model_activity),
        )
        .route(
            "/api/me/usage/funnel",
            axum::routing::get(usage::my_usage_funnel),
        )
        .route(
            "/api/routing/snapshot",
            axum::routing::get(routing::routing_flow_snapshot_handler),
        )
        .route(
            "/api/routing/history",
            axum::routing::get(routing::routing_history),
        )
        .route(
            "/api/usage/{request_id}",
            axum::routing::get(usage::get_usage_detail),
        )
        // Billing groups
        .route(
            "/api/admin/billing-groups",
            axum::routing::get(billing_groups::list_billing_groups)
                .post(billing_groups::create_billing_group),
        )
        .route(
            "/api/admin/billing-groups/{id}",
            axum::routing::patch(billing_groups::set_billing_group_status)
                .delete(billing_groups::delete_billing_group),
        )
        .route(
            "/api/billing-groups/active",
            axum::routing::get(billing_groups::list_active_billing_groups),
        )
        // Billing
        .route(
            "/api/billing/summary",
            axum::routing::get(billing::billing_summary),
        )
        .route(
            "/api/billing/period-summary",
            axum::routing::get(billing::billing_period_summary),
        )
        .route(
            "/api/billing/activities",
            axum::routing::get(billing::billing_activities),
        )
        .route(
            "/api/billing/deductions",
            axum::routing::get(billing::billing_deductions),
        )
        .route(
            "/api/billing/topups",
            axum::routing::get(billing::billing_topups),
        )
        .route(
            "/api/billing/invoices",
            axum::routing::get(billing::billing_invoices),
        )
        .route(
            "/api/billing/months",
            axum::routing::get(billing::billing_months),
        )
        .route(
            "/api/billing/period-summary-all",
            axum::routing::get(billing::billing_period_summary_all),
        )
        .route(
            "/api/admin/billing/summary",
            axum::routing::get(billing::admin_billing_summary),
        )
        .route(
            "/api/admin/billing/active",
            axum::routing::get(billing::admin_billing_activity),
        )
        .route(
            "/api/admin/billing/team-spend-ranking",
            axum::routing::get(billing::admin_billing_team_spend_ranking),
        )
        .route(
            "/api/admin/billing/teams",
            axum::routing::get(billing::admin_billing_teams),
        )
        .route(
            "/api/admin/billing/teams/{team_id}/users",
            axum::routing::get(billing::admin_billing_team_users),
        )
        .route(
            "/api/admin/billing/teams/{team_id}/users/{user_id}/api-keys",
            axum::routing::get(billing::admin_billing_team_user_api_keys),
        )
        .route(
            "/api/admin/billing/teams/{team_id}/users/{user_id}/api-key-costs",
            axum::routing::get(billing::admin_billing_user_api_key_costs),
        )
        .route(
            "/api/admin/billing/users/{user_id}/api-key-costs",
            axum::routing::get(billing::admin_billing_user_api_key_costs_global),
        )
        .route(
            "/api/admin/billing/teams/{team_id}/users/{user_id}/api-keys/{api_key_name}",
            axum::routing::get(billing::admin_billing_api_key_detail),
        )
        .route(
            "/api/admin/billing/users/{user_id}/api-keys/{api_key_name}",
            axum::routing::get(billing::admin_billing_api_key_detail_global),
        )
        .route(
            "/api/admin/billing/teams/{team_id}/requests",
            axum::routing::get(billing::admin_billing_team_requests),
        )
        .route(
            "/api/admin/billing/requests/{request_id}",
            axum::routing::get(billing::admin_billing_request_detail),
        )
        .route(
            "/api/admin/billing/period-summary",
            axum::routing::get(billing::admin_billing_period_summary),
        )
        .route(
            "/api/admin/billing/activities",
            axum::routing::get(billing::admin_billing_activities),
        )
        .route(
            "/api/admin/billing/scoped-period-summary",
            axum::routing::get(billing::admin_billing_scoped_period_summary),
        )
        .route(
            "/api/admin/billing/daily-trend",
            axum::routing::get(billing::admin_billing_daily_trend),
        )
        .route(
            "/api/admin/billing/user-spend-ranking",
            axum::routing::get(billing::admin_billing_user_spend_ranking_scoped),
        )
        .route(
            "/api/admin/billing/deductions",
            axum::routing::get(billing::admin_billing_deductions),
        )
        .route(
            "/api/admin/billing/months",
            axum::routing::get(billing::admin_billing_months),
        )
        .route(
            "/api/admin/billing/period-summary-all",
            axum::routing::get(billing::admin_billing_period_summary_all),
        )
        // Wallet
        .route(
            "/api/wallet/overview",
            axum::routing::get(wallet::wallet_overview),
        )
        .route(
            "/api/wallet/recharge",
            axum::routing::post(wallet::wallet_recharge),
        )
        .route(
            "/api/wallet/create-key",
            axum::routing::post(wallet::wallet_create_key),
        )
        .route(
            "/api/wallet/redeem-key",
            axum::routing::post(wallet::wallet_redeem_key),
        )
        .route(
            "/api/wallet/keys",
            axum::routing::get(wallet::wallet_list_keys),
        )
        .route(
            "/api/wallet/revoke-key",
            axum::routing::post(wallet::wallet_revoke_key),
        )
        .route(
            "/api/wallet/transactions",
            axum::routing::get(wallet::wallet_transactions),
        )
        .route(
            "/api/wallet/estimated-days",
            axum::routing::get(wallet::wallet_estimated_days),
        )
        // Health check
        .route(
            "/api/health-check/models",
            axum::routing::post(health::health_check_models),
        )
        .route(
            "/api/health-check/channels/{id}",
            axum::routing::post(health::health_check_channel),
        )
        // Upstream model sync
        .route(
            "/api/channels/{id}/upstream-models",
            axum::routing::get(channels::list_upstream_models),
        )
        // Settings
        .route(
            "/api/settings/allow-private-ips",
            axum::routing::get(settings::get_allow_private_ips)
                .put(settings::set_allow_private_ips),
        )
        .route(
            "/api/settings/oidc-audience",
            axum::routing::get(settings::get_oidc_expected_audience)
                .put(settings::set_oidc_expected_audience),
        )
        .route(
            "/api/settings/probe-interval",
            axum::routing::get(settings::get_probe_interval).put(settings::set_probe_interval),
        )
        .route(
            "/api/gateway/config",
            axum::routing::get(settings::get_gateway_config_handler)
                .put(settings::set_gateway_config_handler),
        )
        .route(
            "/api/settings/currency",
            axum::routing::put(settings::set_currency),
        )
        .route(
            "/api/app/config",
            axum::routing::get(settings::get_app_config),
        )
        // SSO Configs (admin:settings)
        .route(
            "/api/settings/sso-configs",
            axum::routing::get(sso::list_sso_configs).post(sso::create_sso_config),
        )
        .route(
            "/api/settings/sso-configs/{id}",
            axum::routing::get(sso::get_sso_config)
                .put(sso::update_sso_config)
                .delete(sso::delete_sso_config),
        )
        // Announcements
        .route(
            "/api/announcements",
            axum::routing::get(announcements::list_announcements)
                .post(announcements::create_announcement),
        )
        .route(
            "/api/announcements/public",
            axum::routing::get(announcements::list_published_announcements),
        )
        .route(
            "/api/announcements/{id}",
            axum::routing::put(announcements::update_announcement)
                .delete(announcements::delete_announcement),
        )
        // SkillHub 管理端（控制面，admin:skillhub）
        .route(
            "/api/admin/skills",
            axum::routing::get(skillhub::list_skills).post(skillhub::create_skill),
        )
        .route(
            "/api/admin/skills/{id}",
            axum::routing::get(skillhub::get_skill)
                .patch(skillhub::update_skill)
                .delete(skillhub::delete_skill),
        )
        .route(
            "/api/admin/skills/{id}/status",
            axum::routing::post(skillhub::set_skill_status),
        )
        .route(
            "/api/admin/skills/{id}/versions",
            axum::routing::get(skillhub::list_versions),
        )
        .route(
            "/api/admin/skills/{id}/versions/upload",
            axum::routing::post(skillhub::upload_artifact),
        )
        // SkillHub 用户端（发布态目录/安装/下载）
        .route(
            "/api/skills",
            axum::routing::get(skillhub::list_published_skills),
        )
        .route(
            "/api/skills/{slug}",
            axum::routing::get(skillhub::get_published_skill),
        )
        .route(
            "/api/skills/{slug}/versions",
            axum::routing::get(skillhub::list_published_versions),
        )
        .route(
            "/api/skills/{slug}/download",
            axum::routing::get(skillhub::download_skill),
        )
        .route(
            "/api/skills/runtime-status",
            axum::routing::get(skillhub::runtime_statuses),
        )
        // Skill Runtime 数据面：/api/skills/{slug}/{*rest} 运行时代理。
        // 更具体的 /download、/install 静态段优先匹配，不会冲突。
        .route(
            "/api/skills/{slug}/{*rest}",
            axum::routing::get(skillhub::runtime_proxy)
                .post(skillhub::runtime_proxy)
                .put(skillhub::runtime_proxy)
                .patch(skillhub::runtime_proxy)
                .delete(skillhub::runtime_proxy),
        )
        // Content Moderation
        .route(
            "/api/moderation/rules",
            axum::routing::get(moderation::list_filter_rules).post(moderation::create_filter_rule),
        )
        .route(
            "/api/moderation/rules/{id}",
            axum::routing::put(moderation::update_filter_rule)
                .delete(moderation::delete_filter_rule),
        )
        .route(
            "/api/moderation/enabled",
            axum::routing::get(moderation::get_content_moderation_enabled)
                .put(moderation::set_content_moderation_enabled),
        )
        // Casbin policy management
        .route(
            "/api/admin/policies",
            axum::routing::get(policies::list_policies)
                .post(policies::add_policy)
                .delete(policies::remove_policy),
        )
        // WebSocket real-time events
        .route(
            "/api/health/ws",
            axum::routing::get(crate::server::ws::ws_handler),
        )
        .route_layer(middleware::from_fn(reject_cross_origin_cookie_requests))
}
