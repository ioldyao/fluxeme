use std::sync::Arc;

use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::{Json, Router};
use chrono::{Datelike, Duration, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::channel::Channel;
use crate::domain::model::Model;
use crate::domain::model::Pricing;
use crate::domain::routing::RoutingRule;
use crate::domain::usage::UsageFilter;
use crate::domain::user::{ApiKey, SessionInfo, User};
use crate::ratelimit::RateLimiter;
use crate::cache::compute_gate_status;
use crate::config::types::GatewayRuntimeConfig;
use crate::db::Database;
use crate::server::AppState;

const SESSION_TTL_SECS: i64 = 24 * 3600;

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
    rate_limiter: Arc<RateLimiter>,
    db: Arc<Database>,
}

impl AdminModule {
    pub fn new(secret: &str, db: Arc<Database>) -> Self {
        let rl = Arc::new(RateLimiter::new());
        rl.start_cleanup_task();
        Self {
            secret: secret.to_string(),
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
        let data = decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &Validation::default(),
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
            rate_limiter: Arc::clone(&self.rate_limiter),
            db: self.db.clone(),
        }
    }
}

fn validate_password(pw: &str) -> Result<(), AdminError> {
    if pw.len() < 8 {
        return Err(AdminError::bad_request("Password must be at least 8 characters"));
    }
    if !pw.chars().any(|c| c.is_uppercase()) {
        return Err(AdminError::bad_request("Password must contain an uppercase letter"));
    }
    if !pw.chars().any(|c| c.is_lowercase()) {
        return Err(AdminError::bad_request("Password must contain a lowercase letter"));
    }
    if !pw.chars().any(|c| c.is_ascii_digit()) {
        return Err(AdminError::bad_request("Password must contain a digit"));
    }
    Ok(())
}

// ── Auth helpers ──────────────────────────────────────────────────

fn extract_token(headers: &HeaderMap) -> Result<String, AdminError> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .ok_or_else(|| AdminError::unauthorized("Missing or invalid admin token"))
}

async fn require_session(admin: &AdminModule, headers: &HeaderMap) -> Result<SessionInfo, AdminError> {
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

/// Require admin role. Returns 403 (not 401) so the frontend can
/// distinguish "bad session" from "insufficient permissions".
async fn require_admin(admin: &AdminModule, headers: &HeaderMap) -> Result<SessionInfo, AdminError> {
    let session = require_session(admin, headers).await?;
    if session.role != "admin" {
        return Err(AdminError::forbidden("Admin access required"));
    }
    Ok(session)
}

// ── Error type ────────────────────────────────────────────────────

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
            AdminError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
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
            tz.offset_from_utc_datetime(&now.naive_utc()).fix().local_minus_utc() as i64
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

// ── Login ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

async fn admin_login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginReq>,
) -> Result<Json<Value>, AdminError> {
    // Rate limit login attempts by real peer IP
    let client_ip = addr.ip().to_string();
    if let Some(fwd) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        tracing::debug!(real_ip = %client_ip, forwarded_for = %fwd, "login attempt");
    }
    state
        .rate_limiter
        .check_rpm(&format!("login:{}", client_ip), 10)
        .map_err(|_| AdminError::too_many_requests("Too many login attempts. Try again later."))?;

    // Authenticate against database (all users including admins)
    let user = state
        .db
        .get_user_with_password(&req.username)
        .await.map_err(db_err)?;

    let mut password_matched = false;
    if let Some(ref u) = user {
        if let Some(ref hash) = u.password_hash {
            if !hash.is_empty() {
                match bcrypt::verify(&req.password, hash) {
                    Ok(true) => {
                        password_matched = true;
                    }
                    Ok(false) => { /* wrong password */ }
                    Err(e) => {
                        tracing::error!("bcrypt verify error for user {}: {}", u.id, e);
                        return Err(AdminError::internal("Authentication error"));
                    }
                }
            }
        }
    } else {
        // Constant-time dummy to prevent user enumeration via timing
        let _ = bcrypt::verify(&req.password, "$2b$10$EixZaYVK1fsbw1ZfbX3OXePaWxn96p36PQm4sEPhMNPfFhpYN76Oe");
    }

    if password_matched {
        let u = user.unwrap();
        let info = SessionInfo {
            user_id: u.id.clone(),
            user_name: u.name.clone(),
            role: u.role.clone(),
            token_version: u.token_version,
        };
        let token = state.admin.encode_token(&info)?;
        return Ok(Json(serde_json::json!({
            "token": token,
            "role": u.role,
            "user_id": u.id,
            "user_name": u.name,
            "timezone": u.timezone,
        })));
    }

    Err(AdminError::unauthorized("Invalid credentials"))
}

// ── Setup (first-time admin registration) ─────────────────────────

#[derive(Serialize)]
struct SetupStatus {
    setup_required: bool,
}

async fn setup_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SetupStatus>, AdminError> {
    let count = state
        .db
        .count_admins()
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?;
    Ok(Json(SetupStatus {
        setup_required: count == 0,
    }))
}

#[derive(Deserialize)]
struct SetupRegisterReq {
    username: String,
    password: String,
}

async fn setup_register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetupRegisterReq>,
) -> Result<Json<Value>, AdminError> {
    let count = state
        .db
        .count_admins()
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?;
    if count > 0 {
        return Err(AdminError::bad_request(
            "Admin already exists. Please log in.",
        ));
    }

    if req.username.is_empty() {
        return Err(AdminError::bad_request("Username is required"));
    }
    validate_password(&req.password)?;

    if state
        .db
        .get_user(&req.username)
        .await.map_err(db_err)?
        .is_some()
    {
        return Err(AdminError::bad_request("Username already exists"));
    }

    let hash =
        bcrypt::hash(&req.password, 10).map_err(|e| AdminError::internal(e.to_string()))?;

    let user = User {
        id: req.username.clone(),
        name: req.username.clone(),
        password_hash: Some(hash),
        rate_limits: None,
        timezone: "UTC".to_string(),
        token_version: 0,
        role: "admin".to_string(),
    };
    state.db.create_user(&user).await.map_err(db_err)?;
    state.auth.reload().await;

    tracing::info!("setup_register: first admin user created: {}", user.id);

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Dashboard ─────────────────────────────────────────────────────

#[derive(Serialize)]
struct DashboardResp {
    users: usize,
    channels: usize,
    models: usize,
    rules: usize,
    api_keys: usize,
    endpoints: usize,
    total_requests: usize,
}

async fn admin_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DashboardResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    if session.role == "admin" {
        let users = state.db.list_users().await.map_err(db_err)?;
        let channels = state.db.list_channels().await.map_err(db_err)?;
        let models = state.db.list_models().await.map_err(db_err)?;
        let rules = state.db.list_rules().await.map_err(db_err)?;

        let endpoint_count: usize = channels.iter().map(|c| c.endpoints.len()).sum();
        let total_requests = state.usage.count().await.unwrap_or(0);
        let api_key_count = state.db.all_api_keys().await.map(|k| k.len()).unwrap_or(0);

        Ok(Json(DashboardResp {
            users: users.len(),
            channels: channels.len(),
            models: models.len(),
            rules: rules.len(),
            api_keys: api_key_count,
            endpoints: endpoint_count,
            total_requests,
        }))
    } else {
        let api_keys = state.db.list_api_keys(&session.user_id).await.map_err(db_err)?;
        let user_requests = state.usage.count_by_user(&session.user_id).await.unwrap_or(0);

        Ok(Json(DashboardResp {
            users: 0,
            channels: 0,
            models: 0,
            rules: 0,
            api_keys: api_keys.len(),
            endpoints: 0,
            total_requests: user_requests,
        }))
    }
}

#[derive(Serialize)]
struct TopModel {
    model: String,
    count: u64,
    percentage: f64,
}

#[derive(Serialize)]
struct DashboardAggregations {
    total_requests: u64,
    total_cost: f64,
    requests_24h: u64,
    cost_24h: f64,
    success_rate_24h: f64,
    avg_latency_ms_24h: f64,
    total_tokens_24h: u64,
    top_models_24h: Vec<TopModel>,
}

#[derive(Serialize)]
struct BillingSummary {
    total_requests: u64,
    total_cost: f64,
    balance: f64,
}

async fn billing_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<BillingSummary>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let user_filter: Option<&str> = if session.role == "admin" {
        None
    } else {
        Some(&session.user_id)
    };
    let records = state
        .usage
        .cost_rows_since("1970-01-01T00:00:00", user_filter)
        .await.map_err(AdminError::internal)?;
    let total_cost: f64 = records
        .iter()
        .map(|r| {
            let pp = if r.prompt_price > 0.0 { r.prompt_price } else { 0.0 };
            let cp = if r.completion_price > 0.0 { r.completion_price } else { 0.0 };
            (r.prompt_tokens as f64 / 1000.0 * pp)
                + (r.completion_tokens as f64 / 1000.0 * cp)
        })
        .sum();
    let total_requests = records.len() as u64;
    Ok(Json(BillingSummary {
        total_requests,
        total_cost: (total_cost * 100.0).round() / 100.0,
        balance: 0.0,
    }))
}

#[derive(Deserialize)]
struct PeriodQuery {
    year: Option<i32>,
    month: Option<u32>,
}

#[derive(Serialize)]
struct PeriodSummary {
    year: i32,
    month: u32,
    total_cost: f64,
    total_requests: u64,
    total_tokens: u64,
    by_model: Vec<ModelCostShare>,
    by_channel: Vec<ChannelCostShare>,
}

#[derive(Serialize)]
struct ModelCostShare {
    model: String,
    cost: f64,
    percentage: f64,
}

#[derive(Serialize)]
struct ChannelCostShare {
    channel: String,
    cost: f64,
    percentage: f64,
}

async fn billing_period_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<PeriodSummary>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let user_filter: Option<&str> = if session.role == "admin" {
        None
    } else {
        Some(&session.user_id)
    };

    let (total_cost, total_requests, total_tokens) = state.db.period_summary(year, month, user_filter)
        .await.map_err(|e| AdminError::internal(e.0))?;

    let by_model = state.db.period_model_breakdown(year, month, user_filter)
        .await.map_err(|e| AdminError::internal(e.0))?
        .into_iter()
        .map(|(model, cost)| {
            let pct = if total_cost > 0.0 { (cost / total_cost * 100.0 * 10.0).round() / 10.0 } else { 0.0 };
            ModelCostShare { model, cost: (cost * 100.0).round() / 100.0, percentage: pct }
        })
        .collect();

    let by_channel = state.db.period_channel_breakdown(year, month, user_filter)
        .await.map_err(|e| AdminError::internal(e.0))?
        .into_iter()
        .map(|(channel, cost)| {
            let pct = if total_cost > 0.0 { (cost / total_cost * 100.0 * 10.0).round() / 10.0 } else { 0.0 };
            ChannelCostShare { channel, cost: (cost * 100.0).round() / 100.0, percentage: pct }
        })
        .collect();

    Ok(Json(PeriodSummary {
        year, month,
        total_cost: (total_cost * 100.0).round() / 100.0,
        total_requests,
        total_tokens,
        by_model,
        by_channel,
    }))
}

#[derive(Serialize)]
struct DeductionRecord {
    time: String,
    amount: f64,
    method: String,
}

#[derive(Deserialize)]
struct DeductionQuery {
    year: Option<i32>,
    month: Option<u32>,
    limit: Option<usize>,
    offset: Option<usize>,
}

const DEFAULT_DEDUCTION_PAGE_SIZE: usize = 15;

async fn billing_deductions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<DeductionQuery>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(DEFAULT_DEDUCTION_PAGE_SIZE);
    let offset = q.offset.unwrap_or(0);
    let user_filter: Option<&str> = if session.role == "admin" {
        None
    } else {
        Some(&session.user_id)
    };

    let total = state.db.count_daily_deductions(year, month, user_filter)
        .await.map_err(|e| AdminError::internal(e.0))?;
    let records = state.db.daily_deductions_paginated(year, month, user_filter, limit, offset)
        .await.map_err(|e| AdminError::internal(e.0))?;
    let items: Vec<DeductionRecord> = records.into_iter().map(|(day, amount, _count)| DeductionRecord {
        time: format!("{}T00:00:00", day),
        amount: -((amount * 100.0).round() / 100.0),
        method: "按量计费".to_string(),
    }).collect();

    Ok(Json(serde_json::json!({ "items": items, "total": total })))
}

async fn billing_topups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    Ok(Json(vec![]))
}

async fn billing_invoices(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    Ok(Json(vec![]))
}

async fn billing_months(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, AdminError> {
    let _session = require_admin(&state.admin, &headers).await?;
    state.db.billing_months().await.map_err(|e| AdminError::internal(e.0)).map(Json)
}

#[derive(Serialize)]
struct MonthSummary {
    month: String,
    total_cost: f64,
    total_requests: u64,
    total_tokens: u64,
}

async fn billing_period_summary_all(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<MonthSummary>>, AdminError> {
    let _session = require_admin(&state.admin, &headers).await?;
    let records = state.db.period_summary_all().await.map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(records.into_iter().map(|(month, cost, req, tok)| MonthSummary {
        month,
        total_cost: (cost * 100.0).round() / 100.0,
        total_requests: req,
        total_tokens: tok,
    }).collect()))
}

// ── Wallet ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct WalletOverview {
    balance: f64,
    frozen: f64,
    total_consumed: f64,
    total_recharged: f64,
}

async fn wallet_overview(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<WalletOverview>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let user_id = &session.user_id;
    let (balance, frozen) = state.db.get_wallet_balance(user_id).await.map_err(|e| AdminError::internal(e.0))?;
    let total_consumed = state.db.get_total_consumed(user_id).await.map_err(|e| AdminError::internal(e.0))?;
    let total_recharged = state.db.get_total_recharged(user_id).await.map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(WalletOverview { balance, frozen, total_consumed, total_recharged }))
}

#[derive(Deserialize)]
struct RechargeReq {
    amount: f64,
}

#[derive(Serialize)]
struct RechargeResp {
    transaction_id: String,
    amount: f64,
    balance: f64,
}

#[derive(Deserialize)]
struct WalletCreateKeyReq {
    amount: f64,
    expires_at: Option<String>,
}

#[derive(Serialize)]
struct CreateKeyResp {
    key: String,
    amount: f64,
    expires_at: Option<String>,
}

#[derive(Deserialize)]
struct RedeemKeyReq {
    key: String,
}

#[derive(Serialize)]
struct RedeemKeyResp {
    amount: f64,
    balance: f64,
}

async fn wallet_recharge(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    _req: Json<RechargeReq>,
) -> Result<Json<RechargeResp>, AdminError> {
    return Err(AdminError::bad_request("Recharge is under development"));
}

async fn wallet_create_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<WalletCreateKeyReq>,
) -> Result<Json<CreateKeyResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    if req.amount <= 0.0 {
        return Err(AdminError::bad_request("Amount must be positive"));
    }
    let key = uuid::Uuid::new_v4().to_string();
    state.db.create_recharge_key(&key, req.amount, &session.user_id, req.expires_at.as_deref()).await.map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(CreateKeyResp { key, amount: req.amount, expires_at: req.expires_at }))
}

async fn wallet_redeem_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<RedeemKeyReq>,
) -> Result<Json<RedeemKeyResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let amount = state.db.redeem_recharge_key(&req.key, &session.user_id).await.map_err(|e| AdminError::bad_request(e.0))?;
    let (balance, frozen) = state.db.get_wallet_balance(&session.user_id).await.map_err(|e| AdminError::internal(e.0))?;

    // Sync to Redis gate cache
    let status = compute_gate_status(balance, frozen);
    if let Err(e) = state.cache.set_gate_and_balance(&session.user_id, status, balance).await {
        tracing::warn!(user_id = &session.user_id, "Failed to sync redeem to Redis: {}", e);
    }

    Ok(Json(RedeemKeyResp { amount, balance }))
}

#[derive(Deserialize)]
struct KeyListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    search: Option<String>,
    status: Option<String>,
    used_by: Option<String>,
}

#[derive(Deserialize)]
struct RevokeKeyReq {
    key: String,
}

const DEFAULT_KEY_PAGE_SIZE: usize = 20;

async fn wallet_list_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<KeyListQuery>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let _session = require_admin(&state.admin, &headers).await?;
    let limit = q.limit.unwrap_or(DEFAULT_KEY_PAGE_SIZE);
    let offset = q.offset.unwrap_or(0);
    let total = state.db.count_recharge_keys_filtered(
        q.search.as_deref(),
        q.status.as_deref(),
        q.used_by.as_deref(),
    ).await.map_err(|e| AdminError::internal(e.0))?;
    let items = state.db.list_recharge_keys_filtered(
        limit, offset,
        q.search.as_deref(),
        q.status.as_deref(),
        q.used_by.as_deref(),
    ).await.map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(serde_json::json!({ "items": items, "total": total })))
}

async fn wallet_revoke_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<RevokeKeyReq>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let _session = require_admin(&state.admin, &headers).await?;
    state.db.revoke_recharge_key(&req.key).await.map_err(|e| AdminError::bad_request(e.0))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct WalletTxQuery {
    page: Option<usize>,
    size: Option<usize>,
    since: Option<String>,
    until: Option<String>,
    tx_type: Option<String>,
}

#[derive(Serialize)]
struct WalletTxResp {
    items: Vec<WalletTxItem>,
    total_dates: usize,
}

#[derive(Serialize)]
struct WalletTxItem {
    id: String,
    tx_type: String,
    amount: f64,
    balance_before: f64,
    balance_after: f64,
    method: String,
    status: String,
    note: String,
    created_at: String,
}

async fn wallet_transactions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<WalletTxQuery>,
) -> Result<Json<WalletTxResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let page = q.page.unwrap_or(1);
    let size = q.size.unwrap_or(15).min(31);
    let uid_filter: Option<&str> = if session.role == "admin" { None } else { Some(&session.user_id) };
    let (rows, total_dates) = state.db.list_wallet_tx_by_dates(
        uid_filter, page, size, q.since.as_deref(), q.until.as_deref(), q.tx_type.as_deref(),
    ).await.map_err(|e| AdminError::internal(e.0))?;
    let items = rows.into_iter().map(|r| WalletTxItem {
        id: r.id,
        tx_type: r.tx_type,
        amount: r.amount,
        balance_before: r.balance_before,
        balance_after: r.balance_after,
        method: r.method,
        status: r.status,
        note: r.note,
        created_at: r.created_at,
    }).collect();
    Ok(Json(WalletTxResp { items, total_dates }))
}

#[derive(Serialize)]
struct EstimatedDaysResp {
    days: Option<f64>,
}

async fn wallet_estimated_days(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<EstimatedDaysResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let days = state.db.get_wallet_estimated_days(&session.user_id).await.map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(EstimatedDaysResp { days }))
}

async fn dashboard_aggregations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DashboardAggregations>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since_24h = since_local_days_ago(1, offset);

    let user_filter: Option<&str> = if session.role == "admin" {
        None
    } else {
        Some(&session.user_id)
    };

    // Load model pricing map once
    let models = state.db.list_models().await.unwrap_or_default();
    let mut pricing: std::collections::HashMap<String, (f64, f64)> =
        std::collections::HashMap::new();
    for m in &models {
        pricing.insert(
            m.name.clone(),
            (m.pricing.prompt_price, m.pricing.completion_price),
        );
        pricing.insert(
            m.model_pattern.clone(),
            (m.pricing.prompt_price, m.pricing.completion_price),
        );
    }

    // Build sorted prefix list for glob pattern matching (O(log n) per lookup)
    let mut prefix_prices: Vec<(&str, (f64, f64))> = pricing
        .iter()
        .filter_map(|(k, v)| k.strip_suffix('*').map(|p| (p, *v)))
        .collect();
    prefix_prices.sort_by_key(|b| std::cmp::Reverse(b.0.len())); // most specific first

    fn lookup_price<'a>(
        model_name: &str,
        pricing: &'a std::collections::HashMap<String, (f64, f64)>,
        prefix_prices: &'a [(&str, (f64, f64))],
    ) -> (f64, f64) {
        if let Some(price) = pricing.get(model_name) {
            return *price;
        }
        for (prefix, price) in prefix_prices {
            if model_name.starts_with(prefix) {
                return *price;
            }
        }
        (0.0, 0.0)
    }

    // All-time totals: use COUNT SQL aggregate instead of loading all rows
    let total_requests = match user_filter {
        Some(uid) => state.usage.count_by_user(uid).await.unwrap_or(0),
        None => state.usage.count().await.unwrap_or(0),
    } as u64;

    // 24h stats: use SQL aggregates
    let (requests_24h, success_count, total_latency, total_tokens_24h) = state
        .usage
        .stats_since(&since_24h, user_filter)
        .await
        .unwrap_or((0, 0, 0, 0));

    if requests_24h == 0 {
        return Ok(Json(DashboardAggregations {
            total_requests,
            total_cost: 0.0,
            requests_24h: 0,
            cost_24h: 0.0,
            success_rate_24h: 0.0,
            avg_latency_ms_24h: 0.0,
            total_tokens_24h: 0,
            top_models_24h: vec![],
        }));
    }

    // Compute cost from 24h records (loads only token + model columns)
    let records = state
        .usage
        .cost_rows_since(&since_24h, user_filter)
        .await.map_err(AdminError::internal)?;
    let mut total_cost_24h = 0.0_f64;
    let mut model_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for r in &records {
        let (pp, cp) = if r.prompt_price > 0.0 || r.completion_price > 0.0 {
            (r.prompt_price, r.completion_price)
        } else {
            lookup_price(&r.model, &pricing, &prefix_prices)
        };
        let cost = (r.prompt_tokens as f64 / 1000.0 * pp)
            + (r.completion_tokens as f64 / 1000.0 * cp);
        total_cost_24h += cost;
        *model_counts.entry(r.model.clone()).or_default() += 1;
    }

    // All-time cost: load records with stored pricing
    let all_records = state
        .usage
        .cost_rows_since("1970-01-01T00:00:00", user_filter)
        .await.map_err(AdminError::internal)?;
    let total_cost: f64 = all_records
        .iter()
        .map(|r| {
            let (pp, cp) = if r.prompt_price > 0.0 || r.completion_price > 0.0 {
                (r.prompt_price, r.completion_price)
            } else {
                lookup_price(&r.model, &pricing, &prefix_prices)
            };
            (r.prompt_tokens as f64 / 1000.0 * pp)
                + (r.completion_tokens as f64 / 1000.0 * cp)
        })
        .sum();

    let success_rate = if requests_24h > 0 {
        success_count as f64 / requests_24h as f64 * 100.0
    } else {
        0.0
    };
    let avg_latency = if requests_24h > 0 {
        total_latency as f64 / requests_24h as f64
    } else {
        0.0
    };

    let mut top_models: Vec<TopModel> = model_counts
        .into_iter()
        .map(|(model, count)| TopModel {
            percentage: (count as f64 / requests_24h as f64 * 100.0 * 100.0).round() / 100.0,
            count,
            model,
        })
        .collect();
    top_models.sort_by(|a, b| b.count.cmp(&a.count));
    top_models.truncate(10);

    Ok(Json(DashboardAggregations {
        total_requests,
        total_cost: (total_cost * 100.0).round() / 100.0,
        requests_24h,
        cost_24h: (total_cost_24h * 100.0).round() / 100.0,
        success_rate_24h: (success_rate * 100.0).round() / 100.0,
        avg_latency_ms_24h: (avg_latency * 100.0).round() / 100.0,
        total_tokens_24h,
        top_models_24h: top_models,
    }))
}

// ── Current User ("Me") ───────────────────────────────────────────

#[derive(Deserialize)]
struct ChangePasswordReq {
    current_password: String,
    new_password: String,
}

async fn change_my_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    validate_password(&req.new_password)?;

    // Verify current password
    let user = state
        .db
        .get_user_with_password(&session.user_id)
        .await.map_err(db_err)?;

    if let Some(u) = user {
        if let Some(ref hash) = u.password_hash {
            if !hash.is_empty() {
                match bcrypt::verify(&req.current_password, hash) {
                    Ok(true) => { /* correct password - continue */ }
                    Ok(false) => {
                        return Err(AdminError::bad_request("Current password is incorrect"));
                    }
                    Err(e) => {
                        tracing::error!("bcrypt verify error for user {}: {}", session.user_id, e);
                        return Err(AdminError::internal("Authentication error"));
                    }
                }
            } else {
                return Err(AdminError::bad_request(
                    "Cannot change password for this account",
                ));
            }
        } else {
            return Err(AdminError::bad_request(
                "Cannot change password for this account",
            ));
        }
    } else {
        return Err(AdminError::not_found("User not found"));
    }

    let new_hash =
        bcrypt::hash(&req.new_password, 10).map_err(|e| AdminError::internal(e.to_string()))?;

    let existing = state
        .db
        .get_user(&session.user_id)
        .await.map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;

    let updated = User {
        id: session.user_id.clone(),
        name: session.user_name.clone(),
        password_hash: Some(new_hash),
        rate_limits: existing.rate_limits,
        timezone: existing.timezone,
        token_version: existing.token_version + 1,
        role: existing.role,
    };
    state.db.update_user(&updated).await.map_err(db_err)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
struct UpdateTimezoneReq {
    timezone: String,
}

async fn get_my_timezone(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    Ok(Json(serde_json::json!({ "timezone": tz })))
}

async fn update_my_timezone(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UpdateTimezoneReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    // Validate IANA timezone name
    if req.timezone.parse::<Tz>().is_err() {
        return Err(AdminError::bad_request("Invalid timezone"));
    }

    state
        .db
        .update_user_timezone(&session.user_id, &req.timezone)
        .await.map_err(db_err)?;

    Ok(Json(serde_json::json!({ "ok": true, "timezone": req.timezone })))
}

async fn my_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKey>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let keys = state.db.list_api_keys(&session.user_id).await.map_err(db_err)?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
struct CreateMyKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
    expires_at: Option<String>,
    #[serde(default)]
    spend_limit: Option<f64>,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
}

async fn create_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateMyKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let key_value = format!("sk-{}", uuid::Uuid::new_v4());
    let ak = ApiKey {
        key: key_value.clone(),
        user_id: session.user_id.clone(),
        name: req.name.unwrap_or_default(),
        enabled: req.enabled.unwrap_or(true),
        expires_at: req.expires_at,
        spend_limit: req.spend_limit,
        allowed_models: req.allowed_models,
    };

    state.db.create_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;

    Ok(Json(serde_json::json!({
        "key": ak.key,
        "user_id": ak.user_id,
        "name": ak.name,
        "enabled": ak.enabled,
    })))
}

#[derive(Deserialize)]
struct UpdateMyKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
    expires_at: Option<String>,
    #[serde(default)]
    spend_limit: Option<f64>,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
}

async fn update_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_val): Path<String>,
    Json(req): Json<UpdateMyKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let keys = state.db.list_api_keys(&session.user_id).await.map_err(db_err)?;
    let existing = keys
        .iter()
        .find(|k| k.key == key_val)
        .ok_or_else(|| AdminError::not_found("Key not found"))?;

    let ak = ApiKey {
        key: key_val.clone(),
        user_id: session.user_id.clone(),
        name: req.name.unwrap_or(existing.name.clone()),
        enabled: req.enabled.unwrap_or(existing.enabled),
        expires_at: req.expires_at.or(existing.expires_at.clone()),
        spend_limit: req.spend_limit.or(existing.spend_limit),
        allowed_models: req.allowed_models.or(existing.allowed_models.clone()),
    };

    state.db.update_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;

    Ok(Json(serde_json::json!({ "key": key_val, "updated": true })))
}

async fn delete_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_val): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    // Verify the key belongs to the current user
    let keys = state.db.list_api_keys(&session.user_id).await.map_err(db_err)?;
    if !keys.iter().any(|k| k.key == key_val) {
        return Err(AdminError::not_found("Key not found"));
    }

    state.db.delete_api_key(&key_val).await.map_err(db_err)?;
    state.auth.reload().await;

    Ok(Json(serde_json::json!({ "deleted": key_val })))
}

#[derive(Deserialize)]
struct ToggleKeyReq {
    enabled: bool,
}

async fn toggle_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_val): Path<String>,
    Json(req): Json<ToggleKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let keys = state.db.list_api_keys(&session.user_id).await.map_err(db_err)?;
    if !keys.iter().any(|k| k.key == key_val) {
        return Err(AdminError::not_found("Key not found"));
    }

    let ak = ApiKey {
        key: key_val.clone(),
        user_id: session.user_id.clone(),
        name: String::new(),
        enabled: req.enabled,
        expires_at: None,
        spend_limit: None,
        allowed_models: None,
    };
    state.db.update_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;

    Ok(Json(
        serde_json::json!({ "key": key_val, "enabled": req.enabled }),
    ))
}

async fn toggle_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, key_val)): Path<(String, String)>,
    Json(req): Json<ToggleKeyReq>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers).await?;

    let keys = state.db.list_api_keys(&user_id).await.map_err(db_err)?;
    let existing = keys
        .iter()
        .find(|k| k.key == key_val)
        .ok_or_else(|| AdminError::not_found("Key not found"))?;
    let mut ak = existing.clone();
    ak.enabled = req.enabled;
    state.db.update_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;

    Ok(Json(
        serde_json::json!({ "key": key_val, "enabled": req.enabled }),
    ))
}

// ── User CRUD ─────────────────────────────────────────────────────

async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<User>>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let users = state.db.list_users().await.map_err(db_err)?;
    Ok(Json(users))
}

#[derive(Serialize)]
struct UserDetail {
    #[serde(flatten)]
    user: User,
    keys: Vec<ApiKey>,
}

async fn get_user_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<UserDetail>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let user = state
        .db
        .get_user(&id)
        .await.map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;
    let keys = state.db.list_api_keys(&id).await.map_err(db_err)?;
    Ok(Json(UserDetail { user, keys }))
}

#[derive(Deserialize)]
struct CreateUserReq {
    id: String,
    name: String,
    password: Option<String>,
    rate_limits: Option<crate::domain::user::RateLimit>,
}

async fn create_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateUserReq>,
) -> Result<Json<User>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    if req.id.is_empty() {
        return Err(AdminError::bad_request("User ID is required"));
    }

    let password_hash = if let Some(ref pw) = req.password {
        if pw.is_empty() {
            None
        } else {
            validate_password(pw)?;
            Some(bcrypt::hash(pw, 10).map_err(|e| AdminError::internal(e.to_string()))?)
        }
    } else {
        None
    };

    let user = User {
        id: req.id,
        name: req.name,
        password_hash,
        rate_limits: req.rate_limits,
        timezone: "UTC".to_string(),
        token_version: 0,
        role: "user".to_string(),
    };

    state.db.create_user(&user).await.map_err(db_err)?;
    state.auth.reload().await;

    tracing::info!(
        "admin={} action=create_user target={}",
        session.user_id,
        user.id
    );

    Ok(Json(User {
        password_hash: None,
        ..user
    }))
}

#[derive(Deserialize)]
struct UpdateUserReq {
    name: Option<String>,
    password: Option<String>,
    rate_limits: Option<crate::domain::user::RateLimit>,
}

async fn update_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserReq>,
) -> Result<Json<User>, AdminError> {
    require_admin(&state.admin, &headers).await?;

    let existing = state
        .db
        .get_user(&id)
        .await.map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;

    let user = User {
        id: id.clone(),
        name: req.name.unwrap_or(existing.name.clone()),
        password_hash: if let Some(pw) = req.password {
            if pw.is_empty() {
                None // keep existing
            } else {
                Some(bcrypt::hash(pw, 10).map_err(|e| AdminError::internal(e.to_string()))?)
            }
        } else {
            None // keep existing
        },
        rate_limits: req.rate_limits.or(existing.rate_limits),
        timezone: existing.timezone,
        token_version: existing.token_version,
        role: existing.role,
    };

    state.db.update_user(&user).await.map_err(db_err)?;
    state.auth.reload().await;

    Ok(Json(User {
        password_hash: None,
        ..user
    }))
}

async fn delete_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    state.db.delete_user(&id).await.map_err(db_err)?;
    state.auth.reload().await;

    tracing::info!("admin={} action=delete_user target={}", session.user_id, id);

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── API Key CRUD (admin manages any user's keys) ──────────────────

async fn list_user_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<ApiKey>>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let keys = state.db.list_api_keys(&user_id).await.map_err(db_err)?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
struct CreateKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
    expires_at: Option<String>,
    #[serde(default)]
    spend_limit: Option<f64>,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
}

async fn create_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    let key_value = format!("sk-{}", uuid::Uuid::new_v4());
    let ak = ApiKey {
        key: key_value.clone(),
        user_id: user_id.clone(),
        name: req.name.unwrap_or_default(),
        enabled: req.enabled.unwrap_or(true),
        expires_at: req.expires_at,
        spend_limit: req.spend_limit,
        allowed_models: req.allowed_models,
    };

    state.db.create_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;

    tracing::info!(
        "admin={} action=create_api_key target={} user={}",
        session.user_id,
        ak.key,
        user_id
    );

    Ok(Json(serde_json::json!({
        "key": ak.key,
        "user_id": ak.user_id,
        "name": ak.name,
        "enabled": ak.enabled,
    })))
}

async fn update_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, key_val)): Path<(String, String)>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    let keys = state.db.list_api_keys(&user_id).await.map_err(db_err)?;
    let existing = keys
        .iter()
        .find(|k| k.key == key_val)
        .ok_or_else(|| AdminError::not_found("Key not found"))?;

    let ak = ApiKey {
        key: key_val.clone(),
        user_id: user_id.clone(),
        name: req.name.unwrap_or(existing.name.clone()),
        enabled: req.enabled.unwrap_or(existing.enabled),
        expires_at: req.expires_at.or(existing.expires_at.clone()),
        spend_limit: req.spend_limit.or(existing.spend_limit),
        allowed_models: req.allowed_models.or(existing.allowed_models.clone()),
    };

    state.db.update_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;

    tracing::info!(
        "admin={} action=update_api_key target={} user={}",
        session.user_id,
        key_val,
        user_id
    );

    Ok(Json(serde_json::json!({ "key": key_val, "updated": true })))
}

async fn delete_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((_user_id, key_val)): Path<(String, String)>,
) -> Result<Json<Value>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    state.db.delete_api_key(&key_val).await.map_err(db_err)?;
    state.auth.reload().await;

    tracing::info!(
        "admin={} action=delete_api_key target={}",
        session.user_id,
        key_val
    );

    Ok(Json(serde_json::json!({ "deleted": key_val })))
}

// ── Channel CRUD ──────────────────────────────────────────────────

async fn list_channels(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Channel>>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let channels = state.db.list_channels().await.map_err(db_err)?;
    Ok(Json(channels))
}

async fn create_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(ch): Json<Channel>,
) -> Result<Json<Channel>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    if ch.id.is_empty() {
        return Err(AdminError::bad_request("Channel ID is required"));
    }
    if ch.provider.is_empty() {
        return Err(AdminError::bad_request("Provider is required"));
    }

    state.db.create_channel(&ch).await.map_err(|e| {
        tracing::error!("create_channel error: {:?}", e);
        AdminError::internal("Internal server error")
    })?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=create_channel target={}",
        session.user_id,
        ch.id
    );

    Ok(Json(ch))
}

async fn update_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut ch): Json<Channel>,
) -> Result<Json<Channel>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    ch.id = id.clone();
    state.db.update_channel(&ch).await.map_err(db_err)?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=update_channel target={}",
        session.user_id,
        id
    );

    Ok(Json(ch))
}

async fn delete_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    state.db.delete_channel(&id).await.map_err(db_err)?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=delete_channel target={}",
        session.user_id,
        id
    );

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── Model CRUD ────────────────────────────────────────────────────

async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let models = state.db.list_models().await.map_err(db_err)?;
    Ok(Json(models))
}

async fn create_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut model): Json<Model>,
) -> Result<Json<Model>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    model.id = model.id.trim().to_string();
    if model.id.is_empty() {
        return Err(AdminError::bad_request("Model ID is required"));
    }

    state.db.create_model(&model).await.map_err(db_err)?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=create_model target={}",
        session.user_id,
        model.id
    );

    Ok(Json(model))
}

async fn update_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut model): Json<Model>,
) -> Result<Json<Model>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    model.id = id.clone();
    state.db.update_model(&model).await.map_err(db_err)?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=update_model target={}",
        session.user_id,
        id
    );

    Ok(Json(model))
}

async fn delete_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    state.db.delete_model(&id).await.map_err(db_err)?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=delete_model target={}",
        session.user_id,
        id
    );

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── Public Models (any authenticated user) ────────────────────────

async fn list_public_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    require_session(&state.admin, &headers).await?;
    let models = state.db.list_published_models().await.map_err(db_err)?;
    Ok(Json(models))
}

async fn toggle_publish_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;
    let models = state.db.list_models().await.map_err(db_err)?;
    let model = models
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AdminError::not_found("Model not found"))?;
    let new_status = !model.published;
    state
        .db
        .set_model_published(&id, new_status)
        .await.map_err(db_err)?;
    if !new_status {
        let _ = state.db.delete_subscriptions_by_model(&id).await;
    }
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=toggle_publish_model target={} published={}",
        session.user_id,
        id,
        new_status
    );

    Ok(Json(
        serde_json::json!({ "id": id, "published": new_status }),
    ))
}

async fn update_model_pricing(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(pricing): Json<Pricing>,
) -> Result<Json<Value>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;
    state.db.set_model_pricing(&id, &pricing).await.map_err(db_err)?;

    tracing::info!(
        "admin={} action=update_model_pricing target={}",
        session.user_id,
        id
    );

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── User Subscriptions ────────────────────────────────────────────

async fn list_my_subscriptions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let models = state
        .db
        .list_subscriptions(&session.user_id)
        .await.map_err(db_err)?;
    Ok(Json(models))
}

async fn subscribe_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    state
        .db
        .subscribe_user(&session.user_id, &model_id)
        .await.map_err(db_err)?;
    Ok(Json(serde_json::json!({ "subscribed": model_id })))
}

async fn unsubscribe_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    state
        .db
        .unsubscribe_user(&session.user_id, &model_id)
        .await.map_err(db_err)?;
    Ok(Json(serde_json::json!({ "unsubscribed": model_id })))
}

#[derive(Deserialize)]
struct TestConnectionBody {
    model_id: String,
}

async fn test_subscription_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<TestConnectionBody>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    // Check the user is subscribed to this model
    let subscribed = state
        .db
        .list_subscribed_model_ids(&session.user_id)
        .await.map_err(|e| AdminError::internal(e.0))?;
    if !subscribed.contains(&body.model_id) {
        return Err(AdminError::forbidden("未订阅此模型"));
    }

    // Load model to get channel bindings
    let model = state
        .db
        .get_model(&body.model_id)
        .await.map_err(|e| AdminError::internal(e.0))?
        .ok_or_else(|| AdminError::not_found("模型不存在"))?;

    // Find the first enabled channel for this model (by priority)
    let mut bindings = model.channels;
    bindings.sort_by_key(|b| b.priority);

    let channel_id = bindings
        .iter()
        .find_map(|b| state.routing.get_channel(&b.channel_id).filter(|ch| ch.enabled).map(|ch| ch.id.clone()))
        .ok_or_else(|| AdminError::internal("该模型没有可用的通道"))?;

    // Resolve provider adapter + endpoint from the channel
    let (provider_name, balancer) = state
        .routing
        .get_route(&channel_id)
        .ok_or_else(|| AdminError::internal("通道路由不可用"))?;

    let adapter = state
        .providers
        .get(&provider_name)
        .ok_or_else(|| AdminError::internal("未找到提供商适配器"))?;

    let (endpoint_idx, endpoint) = balancer
        .as_health_aware()
        .select()
        .ok_or_else(|| AdminError::internal("没有可用的端点"))?;

    // Send a minimal request as a connectivity probe.
    // Use native /v1/messages format for Anthropic, /v1/chat/completions for others.
    let start = std::time::Instant::now();
    let result = if provider_name == "anthropic" {
        let test_body = serde_json::json!({
            "model": model.model_pattern,
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1,
        });
        adapter.messages(endpoint, test_body).await
    } else {
        let test_body = serde_json::json!({
            "model": model.model_pattern,
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1,
            "stream": false,
        });
        adapter.chat_complete(endpoint, test_body).await
    };
    let latency_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => {
            balancer.as_health_aware().record_success(endpoint_idx);
            Ok(Json(serde_json::json!({
                "success": true,
                "model": resp.get("model"),
                "status": "ok",
                "latency_ms": latency_ms,
            })))
        }
        Err(e) => {
            balancer.as_health_aware().record_failure(endpoint_idx);
            Ok(Json(serde_json::json!({
                "success": false,
                "error": e.0,
                "latency_ms": latency_ms,
            })))
        }
    }
}

// ── Routing Rule CRUD ─────────────────────────────────────────────

async fn list_rules(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<RoutingRule>>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let rules = state.db.list_rules().await.map_err(db_err)?;
    Ok(Json(rules))
}

async fn create_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(rule): Json<RoutingRule>,
) -> Result<Json<RoutingRule>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    if rule.name.is_empty() {
        return Err(AdminError::bad_request("Rule name is required"));
    }

    state.db.create_rule(&rule).await.map_err(db_err)?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=create_rule target={}",
        session.user_id,
        rule.name
    );

    Ok(Json(rule))
}

async fn update_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(mut rule): Json<RoutingRule>,
) -> Result<Json<RoutingRule>, AdminError> {
    require_admin(&state.admin, &headers).await?;

    rule.name = name;
    state.db.update_rule(&rule).await.map_err(db_err)?;
    state.routing.reload().await;

    Ok(Json(rule))
}

async fn delete_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_admin(&state.admin, &headers).await?;

    state.db.delete_rule(&name).await.map_err(db_err)?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=delete_rule target={}",
        session.user_id,
        name
    );

    Ok(Json(serde_json::json!({ "deleted": name })))
}

// ── Usage Logs ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct UsageQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    user_id: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    api_format: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
}

#[derive(Serialize)]
struct UsageResponse {
    records: Vec<crate::domain::usage::UsageRecord>,
    total: usize,
}

async fn get_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageQuery>,
) -> Result<Json<UsageResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let limit = q.limit.unwrap_or(50);
    let offset = q.offset.unwrap_or(0);

    // Regular users can only see their own usage
    let user_filter: Option<String> = if session.role == "user" {
        Some(session.user_id.clone())
    } else {
        q.user_id
    };

    let filter = UsageFilter {
        user_id: user_filter,
        model: q.model,
        api_key_name: q.api_key,
        api_format: q.api_format,
        start_date: q.start_date,
        end_date: q.end_date,
    };

    let total = state
        .usage
        .count_filtered(&filter)
        .await
        .map_err(|e| {
            tracing::error!("Usage count failed: {}", e);
            AdminError::internal("Internal server error")
        })?;

    let records = state
        .usage
        .query(limit, offset, &filter)
        .await
        .map_err(|e| {
            tracing::error!("Usage query failed: {}", e);
            AdminError::internal("Internal server error")
        })?;

    Ok(Json(UsageResponse { records, total }))
}

async fn get_usage_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<crate::domain::usage::UsageRecord>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let record = state
        .usage
        .get_detail(&request_id)
        .await
        .map_err(|e| {
            tracing::error!("Usage detail query failed: {}", e);
            AdminError::internal("Internal server error")
        })?
        .ok_or_else(|| AdminError::not_found("Usage record not found"))?;

    if session.role != "admin" && record.user_id != session.user_id {
        return Err(AdminError::not_found("Usage record not found"));
    }

    Ok(Json(record))
}

#[derive(Serialize)]
struct DailyUsage {
    date: String,
    count: i64,
}

async fn daily_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageQuery>,
) -> Result<Json<Vec<DailyUsage>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let days = q.limit.unwrap_or(14) as i64;
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);

    let user_filter: Option<&str> = if session.role == "admin" {
        None
    } else {
        Some(&session.user_id)
    };

    let records = state
        .usage
        .daily_counts(&since, user_filter, offset)
        .await.map_err(AdminError::internal)?;

    Ok(Json(
        records
            .into_iter()
            .map(|(date, count)| DailyUsage { date, count })
            .collect(),
    ))
}

// ── Usage Aggregation ─────────────────────────────────────────────

#[derive(Deserialize)]
struct UsageAggregateQuery {
    days: Option<i64>,
    user_id: Option<String>,
}

#[derive(Serialize)]
struct DailyAggregate {
    date: String,
    count: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    success_count: u64,
    latency_ms: u64,
}

async fn usage_aggregate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageAggregateQuery>,
) -> Result<Json<Vec<DailyAggregate>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let days = q.days.unwrap_or(14);
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);

    let user_filter: Option<&str> = if session.role == "admin" {
        q.user_id.as_deref()
    } else {
        Some(&session.user_id)
    };

    let records = state
        .usage
        .daily_stats(&since, user_filter, offset)
        .await.map_err(AdminError::internal)?;

    Ok(Json(
        records
            .into_iter()
            .map(|(date, count, pt, ct, tt, sc, lat)| DailyAggregate {
                date,
                count,
                prompt_tokens: pt,
                completion_tokens: ct,
                total_tokens: tt,
                success_count: sc,
                latency_ms: lat,
            })
            .collect(),
    ))
}

// ── Model Activity ────────────────────────────────────────────────

#[derive(Serialize)]
struct ModelActivity {
    model: String,
    total_requests: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    success_count: u64,
    failure_count: u64,
}

async fn model_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageAggregateQuery>,
) -> Result<Json<Vec<ModelActivity>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let days = q.days.unwrap_or(7) as i64;
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);
    let user_filter: Option<&str> = if session.role == "admin" {
        q.user_id.as_deref()
    } else {
        Some(&session.user_id)
    };
    let records = state
        .db
        .model_activity(&since, user_filter)
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?;
    Ok(Json(
        records
            .into_iter()
            .map(|(model, total, pt, ct, sc, fc)| ModelActivity {
                model,
                total_requests: total,
                prompt_tokens: pt,
                completion_tokens: ct,
                success_count: sc,
                failure_count: fc,
            })
            .collect(),
    ))
}

// ── Health Check ──────────────────────────────────────────────────

#[derive(Serialize)]
struct HealthCheckResult {
    models_updated: usize,
    channels_checked: usize,
}

async fn health_check_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<HealthCheckResult>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let (models_updated, channels_checked) = state
        .health
        .check_all_channels()
        .await
        .map_err(AdminError::internal)?;
    Ok(Json(HealthCheckResult {
        models_updated,
        channels_checked,
    }))
}

async fn health_check_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::service::health::ChannelHealthResult>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let result = state
        .health
        .check_channel(&id)
        .await
        .map_err(AdminError::internal)?;
    Ok(Json(result))
}

async fn list_upstream_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<crate::service::health::UpstreamModelInfo>>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let models = state
        .health
        .list_upstream_models(&id)
        .await
        .map_err(AdminError::internal)?;
    Ok(Json(models))
}

// ── Load balancer/health API ─────────────────────────────────────

#[derive(Deserialize)]
struct ToggleEndpointBody {
    enabled: bool,
}

#[derive(Serialize)]
struct EndpointHealthItem {
    endpoint_id: i64,
    url: String,
    enabled: bool,
    available: bool,
}

#[derive(Serialize)]
struct ChannelHealthResponse {
    channel_id: String,
    endpoints: Vec<EndpointHealthItem>,
}

async fn get_channel_health(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ChannelHealthResponse>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let eps = state.routing.channel_health(&id);
    let ch = state.db.get_channel(&id).await.map_err(|e| AdminError::internal(e.0))?;
    let channel_id = ch.as_ref().map(|c| c.id.clone()).unwrap_or(id);
    let mut endpoints = Vec::with_capacity(eps.len());
    for (eid, enabled, available) in eps {
        let url = state
            .db
            .get_endpoint(eid)
            .await
            .ok()
            .flatten()
            .map(|ep| ep.url)
            .unwrap_or_default();
        endpoints.push(EndpointHealthItem {
            endpoint_id: eid,
            url,
            enabled,
            available,
        });
    }
    Ok(Json(ChannelHealthResponse { channel_id, endpoints }))
}

async fn toggle_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<ToggleEndpointBody>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    state
        .db
        .update_endpoint_enabled(id, body.enabled)
        .await.map_err(|e| AdminError::internal(e.0))?;
    state.routing.set_endpoint_enabled(id, body.enabled);
    Ok(Json(serde_json::json!({ "success": true })))
}

// ── Settings ──────────────────────────────────────────────────────

async fn get_allow_private_ips(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let value = state.db.get_setting("allow_private_ips").await.map_err(db_err)?;
    // Default to true when no setting is stored (matches AtomicBool default)
    let enabled = value.as_deref() != Some("false");
    Ok(Json(serde_json::json!({ "enabled": enabled })))
}

#[derive(Deserialize)]
struct AllowPrivateIpsReq {
    enabled: bool,
}

async fn set_allow_private_ips(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AllowPrivateIpsReq>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let value = if req.enabled { "true" } else { "false" };
    state.db.set_setting("allow_private_ips", value).await.map_err(db_err)?;
    crate::provider::set_allow_private_ips(req.enabled);
    Ok(Json(serde_json::json!({ "enabled": req.enabled })))
}

// ── Gateway Config ──────────────────────────────────────────────────

async fn get_gateway_config_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<GatewayRuntimeConfig>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    let config = state.db.get_gateway_config().await.map_err(db_err)?;
    Ok(Json(config))
}

async fn set_gateway_config_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(config): Json<GatewayRuntimeConfig>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers).await?;
    // Validate and persist
    state.db.set_gateway_config(&config).await.map_err(db_err)?;
    // Update in-memory config
    *state.gateway_config.write().unwrap() = config;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Router ────────────────────────────────────────────────────────

pub fn admin_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/api/login", axum::routing::post(admin_login))
        .route(
            "/admin/api/setup/status",
            axum::routing::get(setup_status),
        )
        .route(
            "/admin/api/setup/register",
            axum::routing::post(setup_register),
        )
        .route(
            "/admin/api/sso/status",
            axum::routing::get(crate::sso::sso_status_handler),
        )
        .route(
            "/admin/api/sso/login",
            axum::routing::get(crate::sso::sso_login_handler),
        )
        .route(
            "/admin/api/sso/callback",
            axum::routing::get(crate::sso::sso_callback_handler),
        )
        .route("/admin/api/dashboard", axum::routing::get(admin_dashboard))
        .route(
            "/admin/api/dashboard/aggregations",
            axum::routing::get(dashboard_aggregations),
        )
        // Current user
        .route(
            "/admin/api/me/password",
            axum::routing::post(change_my_password),
        )
        .route(
            "/admin/api/me/timezone",
            axum::routing::get(get_my_timezone).put(update_my_timezone),
        )
        .route(
            "/admin/api/me/keys",
            axum::routing::get(my_keys).post(create_my_key),
        )
        .route(
            "/admin/api/me/keys/{key_val}",
            axum::routing::delete(delete_my_key)
                .patch(toggle_my_key)
                .put(update_my_key),
        )
        // Users
        .route(
            "/admin/api/users",
            axum::routing::get(list_users).post(create_user),
        )
        .route(
            "/admin/api/users/{id}",
            axum::routing::get(get_user_detail)
                .put(update_user)
                .delete(delete_user),
        )
        // User API keys (admin)
        .route(
            "/admin/api/users/{user_id}/keys",
            axum::routing::get(list_user_keys).post(create_user_key),
        )
        .route(
            "/admin/api/users/{user_id}/keys/{key_val}",
            axum::routing::delete(delete_user_key)
                .patch(toggle_user_key)
                .put(update_user_key),
        )
        // Channels
        .route(
            "/admin/api/channels",
            axum::routing::get(list_channels).post(create_channel),
        )
        .route(
            "/admin/api/channels/{id}",
            axum::routing::put(update_channel).delete(delete_channel),
        )
        .route(
            "/admin/api/channels/{id}/health",
            axum::routing::get(get_channel_health),
        )
        .route(
            "/admin/api/endpoints/{id}",
            axum::routing::patch(toggle_endpoint),
        )
        // Models
        .route(
            "/admin/api/models",
            axum::routing::get(list_models).post(create_model),
        )
        .route(
            "/admin/api/models/public",
            axum::routing::get(list_public_models),
        )
        .route(
            "/admin/api/models/{id}/publish",
            axum::routing::post(toggle_publish_model),
        )
        .route(
            "/admin/api/models/{id}/pricing",
            axum::routing::patch(update_model_pricing),
        )
        .route(
            "/admin/api/models/{id}",
            axum::routing::put(update_model).delete(delete_model),
        )
        // Subscriptions
        .route(
            "/admin/api/me/subscriptions",
            axum::routing::get(list_my_subscriptions),
        )
        .route(
            "/admin/api/me/subscriptions/{model_id}",
            axum::routing::post(subscribe_model).delete(unsubscribe_model),
        )
        .route(
            "/admin/api/me/test-connection",
            axum::routing::post(test_subscription_connection),
        )
        // Routing rules
        .route(
            "/admin/api/rules",
            axum::routing::get(list_rules).post(create_rule),
        )
        .route(
            "/admin/api/rules/{name}",
            axum::routing::put(update_rule).delete(delete_rule),
        )
        // Usage
        .route("/admin/api/usage", axum::routing::get(get_usage))
        .route("/admin/api/usage/daily", axum::routing::get(daily_usage))
        .route("/admin/api/usage/aggregate", axum::routing::get(usage_aggregate))
        .route("/admin/api/usage/model-activity", axum::routing::get(model_activity))
        .route(
            "/admin/api/usage/{request_id}",
            axum::routing::get(get_usage_detail),
        )
        // Billing
        .route("/admin/api/billing/summary", axum::routing::get(billing_summary))
        .route("/admin/api/billing/period-summary", axum::routing::get(billing_period_summary))
        .route("/admin/api/billing/deductions", axum::routing::get(billing_deductions))
        .route("/admin/api/billing/topups", axum::routing::get(billing_topups))
        .route("/admin/api/billing/invoices", axum::routing::get(billing_invoices))
        .route("/admin/api/billing/months", axum::routing::get(billing_months))
        .route("/admin/api/billing/period-summary-all", axum::routing::get(billing_period_summary_all))
        // Wallet
        .route("/admin/api/wallet/overview", axum::routing::get(wallet_overview))
        .route("/admin/api/wallet/recharge", axum::routing::post(wallet_recharge))
        .route("/admin/api/wallet/create-key", axum::routing::post(wallet_create_key))
        .route("/admin/api/wallet/redeem-key", axum::routing::post(wallet_redeem_key))
        .route("/admin/api/wallet/keys", axum::routing::get(wallet_list_keys))
        .route("/admin/api/wallet/revoke-key", axum::routing::post(wallet_revoke_key))
        .route("/admin/api/wallet/transactions", axum::routing::get(wallet_transactions))
        .route("/admin/api/wallet/estimated-days", axum::routing::get(wallet_estimated_days))
        // Health check
        .route(
            "/admin/api/health-check/models",
            axum::routing::post(health_check_models),
        )
        .route(
            "/admin/api/health-check/channels/{id}",
            axum::routing::post(health_check_channel),
        )
        // Upstream model sync
        .route(
            "/admin/api/channels/{id}/upstream-models",
            axum::routing::get(list_upstream_models),
        )
        // Settings
        .route(
            "/admin/api/settings/allow-private-ips",
            axum::routing::get(get_allow_private_ips).put(set_allow_private_ips),
        )
        .route(
            "/admin/api/gateway/config",
            axum::routing::get(get_gateway_config_handler)
                .put(set_gateway_config_handler),
        )
}
