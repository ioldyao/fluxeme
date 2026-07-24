use std::sync::Arc;

use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::{Json, Router};
use chrono::{Duration, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::authz::AuthzModule;
use crate::db::Database;
use crate::domain::user::SessionInfo;
use crate::ratelimit::RateLimiter;

const SESSION_TTL_SECS: i64 = 24 * 3600;

// ── Sub-modules ────────────────────────────────────────────────────

pub mod auth;
pub mod billing;
pub mod channels;
pub mod dashboard;
pub mod health;
pub mod me;
pub mod models;
pub mod moderation;
pub mod routing;
pub mod rules;
pub mod settings;
pub mod subscriptions;
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
}

impl AdminModule {
    pub fn new(secret: &str, encryption_key: &str, db: Arc<Database>) -> Self {
        let rl = Arc::new(RateLimiter::new());
        rl.start_cleanup_task();
        Self {
            secret: secret.to_string(),
            encryption_key: encryption_key.to_string(),
            rate_limiter: rl,
            db,
        }
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

fn extract_token(headers: &HeaderMap) -> Result<String, AdminError> {
    // Try Authorization header first (for API/programmatic access)
    if let Some(token) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
    {
        return Ok(token);
    }
    // Fall back to httpOnly cookie (for browser-based admin UI)
    if let Some(cookie) = headers.get("cookie").and_then(|v| v.to_str().ok()) {
        for pair in cookie.split(';') {
            let pair = pair.trim();
            if let Some(value) = pair.strip_prefix("session_token=") {
                return Ok(value.to_string());
            }
        }
    }
    Err(AdminError::unauthorized("Missing or invalid admin token"))
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
    let session = admin.decode_token(&token)?;

    // Verify token_version against DB (session revocation enforcement)
    let db_user = admin
        .db
        .get_user(&session.user_id)
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?
        .ok_or_else(|| AdminError::unauthorized("User not found"))?;
    if db_user.token_version != session.token_version {
        return Err(AdminError::unauthorized(
            "Session has been revoked. Please log in again.",
        ));
    }

    // Rate limit: 300 requests/minute per admin session to prevent abuse
    admin
        .rate_limiter
        .check_rpm(&format!("admin:{}", session.user_id), 300)
        .map_err(|_| AdminError::too_many_requests("Too many requests. Try again later."))?;

    Ok(session)
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

/// Wrap a DB error from a bad-request operation: log and return a generic message.
fn db_err_bad_request(e: crate::db::DbError) -> AdminError {
    tracing::error!("[admin] DB bad-request error: {}", e.0);
    AdminError::bad_request("Bad request")
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
        // Current user
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
            "/api/health/routing",
            axum::routing::get(routing::routing_health),
        )
        .route(
            "/api/health/recent-paths",
            axum::routing::get(routing::recent_request_paths),
        )
        .route(
            "/api/models/{id}",
            axum::routing::put(models::update_model).delete(models::delete_model),
        )
        // Subscriptions
        .route(
            "/api/me/subscriptions",
            axum::routing::get(subscriptions::list_my_subscriptions),
        )
        .route(
            "/api/me/subscriptions/{model_id}",
            axum::routing::post(subscriptions::subscribe_model)
                .delete(subscriptions::unsubscribe_model),
        )
        .route(
            "/api/me/test-connection",
            axum::routing::post(subscriptions::test_subscription_connection),
        )
        // Routing rules
        .route(
            "/api/rules",
            axum::routing::get(rules::list_rules).post(rules::create_rule),
        )
        .route(
            "/api/rules/{name}",
            axum::routing::put(rules::update_rule).delete(rules::delete_rule),
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
            "/api/gateway/config",
            axum::routing::get(settings::get_gateway_config_handler)
                .put(settings::set_gateway_config_handler),
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
        // WebSocket real-time events
        .route(
            "/api/health/ws",
            axum::routing::get(crate::server::ws::ws_handler),
        )
}
