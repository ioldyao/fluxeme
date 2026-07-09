use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::{Json, Router};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::channel::Channel;
use crate::domain::model::Model;
use crate::domain::routing::RoutingRule;
use crate::domain::user::{ApiKey, SessionInfo, User};
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
    /// expiration timestamp (UTC)
    exp: usize,
    /// issued at timestamp (UTC)
    iat: usize,
}

// ── Admin state ───────────────────────────────────────────────────

pub struct AdminModule {
    secret: String,
}

impl AdminModule {
    pub fn new(secret: &str) -> Self {
        Self {
            secret: secret.to_string(),
        }
    }

    fn encode_token(&self, info: &SessionInfo) -> Result<String, AdminError> {
        let claims = JwtClaims {
            sub: info.user_id.clone(),
            name: info.user_name.clone(),
            role: info.role.clone(),
            exp: (Utc::now() + Duration::seconds(SESSION_TTL_SECS))
                .timestamp() as usize,
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
        .map_err(|e| AdminError::unauthorized(format!("Invalid token: {}", e)))?;
        Ok(SessionInfo {
            user_id: data.claims.sub,
            user_name: data.claims.name,
            role: data.claims.role,
        })
    }
}

impl Clone for AdminModule {
    fn clone(&self) -> Self {
        Self {
            secret: self.secret.clone(),
        }
    }
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

fn require_session(admin: &AdminModule, headers: &HeaderMap) -> Result<SessionInfo, AdminError> {
    let token = extract_token(headers)?;
    admin.decode_token(&token)
}

/// Require admin role. Returns 403 (not 401) so the frontend can
/// distinguish "bad session" from "insufficient permissions".
fn require_admin(admin: &AdminModule, headers: &HeaderMap) -> Result<SessionInfo, AdminError> {
    let session = require_session(admin, headers)?;
    if session.role != "admin" {
        return Err(AdminError::forbidden("Admin access required"));
    }
    Ok(session)
}

// ── Error type ────────────────────────────────────────────────────

pub struct AdminError {
    status: StatusCode,
    message: String,
}

impl AdminError {
    fn unauthorized(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::UNAUTHORIZED, message: msg.into() }
    }
    fn forbidden(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::FORBIDDEN, message: msg.into() }
    }
    fn not_found(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::NOT_FOUND, message: msg.into() }
    }
    fn bad_request(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: msg.into() }
    }
    fn internal(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, message: msg.into() }
    }
}

impl IntoResponse for AdminError {
    fn into_response(self) -> axum::response::Response {
        let body = serde_json::json!({
            "error": self.message,
        });
        (self.status, Json(body)).into_response()
    }
}

// ── Login ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

async fn admin_login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginReq>,
) -> Result<Json<Value>, AdminError> {
    // First check: admin credentials from config (super admin)
    {
        let cfg = state.config.read().unwrap();
        if req.username == cfg.admin.username && req.password == cfg.admin.password {
            let info = SessionInfo {
                user_id: cfg.admin.username.clone(),
                user_name: "管理员".to_string(),
                role: "admin".to_string(),
            };
            let token = state.admin.encode_token(&info)?;
            return Ok(Json(serde_json::json!({
                "token": token,
                "role": "admin",
                "user_id": cfg.admin.username,
                "user_name": "管理员",
            })));
        }
    }

    // Second check: regular user from database
    let user = state.db.get_user_with_password(&req.username)
        .map_err(|e| AdminError::internal(e.0))?;

    if let Some(u) = user {
        if let Some(ref hash) = u.password_hash {
            if !hash.is_empty() && bcrypt::verify(&req.password, hash).unwrap_or(false) {
                let info = SessionInfo {
                    user_id: u.id.clone(),
                    user_name: u.name.clone(),
                    role: "user".to_string(),
                };
                let token = state.admin.encode_token(&info)?;
                return Ok(Json(serde_json::json!({
                    "token": token,
                    "role": "user",
                    "user_id": u.id,
                    "user_name": u.name,
                })));
            }
        }
    }

    Err(AdminError::unauthorized("Invalid credentials"))
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
    let session = require_session(&state.admin, &headers)?;

    if session.role == "admin" {
        let users = state.db.list_users().map_err(|e| AdminError::internal(e.0))?;
        let channels = state.db.list_channels().map_err(|e| AdminError::internal(e.0))?;
        let models = state.db.list_models().map_err(|e| AdminError::internal(e.0))?;
        let rules = state.db.list_rules().map_err(|e| AdminError::internal(e.0))?;

        let endpoint_count: usize = channels.iter().map(|c| c.endpoints.len()).sum();
        let total_requests = state.usage.count().unwrap_or(0);
        let api_key_count: usize = users.iter().map(|u| {
            state.db.list_api_keys(&u.id).map(|k| k.len()).unwrap_or(0)
        }).sum();

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
        let api_keys = state.db.list_api_keys(&session.user_id)
            .map_err(|e| AdminError::internal(e.0))?;
        let user_requests = state.usage.count_by_user(&session.user_id).unwrap_or(0);

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

async fn dashboard_aggregations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DashboardAggregations>, AdminError> {
    let session = require_session(&state.admin, &headers)?;
    let since_24h = (chrono::Utc::now() - chrono::Duration::hours(24))
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();

    let user_filter: Option<&str> = if session.role == "admin" { None } else { Some(&session.user_id) };

    // Load model pricing map
    let models = state.db.list_models().unwrap_or_default();
    let mut pricing: std::collections::HashMap<String, (f64, f64)> = std::collections::HashMap::new();
    for m in &models {
        pricing.insert(m.name.clone(), (m.pricing.prompt_price, m.pricing.completion_price));
        pricing.insert(m.model_pattern.clone(), (m.pricing.prompt_price, m.pricing.completion_price));
    }

    fn lookup_price<'a>(model_name: &str, pricing: &'a std::collections::HashMap<String, (f64, f64)>) -> &'a (f64, f64) {
        if let Some(price) = pricing.get(model_name) {
            return price;
        }
        for (pattern, price) in pricing {
            if let Some(prefix) = pattern.strip_suffix('*') {
                if model_name.starts_with(prefix) {
                    return price;
                }
            }
        }
        pricing.get("").unwrap_or(&(0.0, 0.0))
    }

    // All-time totals
    let total_requests = match user_filter {
        Some(uid) => state.usage.count_by_user(uid).unwrap_or(0),
        None => state.usage.count().unwrap_or(0),
    } as u64;

    // 24h records
    let records = state.db.query_usage_since(&since_24h, user_filter)
        .map_err(|e| AdminError::internal(e.0))?;
    let requests_24h = records.len() as u64;
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

    let mut total_cost_24h = 0.0_f64;
    let mut total_cost = 0.0_f64;
    let mut success_count = 0_u64;
    let mut total_latency = 0_u64;
    let mut total_tokens_24h = 0_u64;
    let mut model_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    for r in &records {
        let price = *lookup_price(&r.model, &pricing);
        let cost = (r.prompt_tokens as f64 / 1000.0 * price.0)
                  + (r.completion_tokens as f64 / 1000.0 * price.1);
        total_cost_24h += cost;
        if r.success { success_count += 1; }
        total_latency += r.latency_ms as u64;
        total_tokens_24h += r.total_tokens as u64;
        *model_counts.entry(r.model.clone()).or_default() += 1;
    }

    // All-time cost
    let all_records = state.db.query_usage_since("1970-01-01T00:00:00", user_filter)
        .map_err(|e| AdminError::internal(e.0))?;
    for r in &all_records {
        let price = *lookup_price(&r.model, &pricing);
        total_cost += (r.prompt_tokens as f64 / 1000.0 * price.0)
                    + (r.completion_tokens as f64 / 1000.0 * price.1);
    }

    let success_rate = if requests_24h > 0 { success_count as f64 / requests_24h as f64 * 100.0 } else { 0.0 };
    let avg_latency = if requests_24h > 0 { total_latency as f64 / requests_24h as f64 } else { 0.0 };

    let mut top_models: Vec<TopModel> = model_counts.into_iter()
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
    let session = require_session(&state.admin, &headers)?;

    if req.new_password.is_empty() {
        return Err(AdminError::bad_request("New password cannot be empty"));
    }
    if req.new_password.len() < 6 {
        return Err(AdminError::bad_request("Password must be at least 6 characters"));
    }

    // Verify current password
    let user = state.db.get_user_with_password(&session.user_id)
        .map_err(|e| AdminError::internal(e.0))?;

    if let Some(u) = user {
        if let Some(ref hash) = u.password_hash {
            if !hash.is_empty() && !bcrypt::verify(&req.current_password, hash).unwrap_or(false) {
                return Err(AdminError::bad_request("Current password is incorrect"));
            }
        } else {
            return Err(AdminError::bad_request("Cannot change password for this account"));
        }
    } else {
        return Err(AdminError::not_found("User not found"));
    }

    let new_hash = bcrypt::hash(&req.new_password, 10)
        .map_err(|e| AdminError::internal(e.to_string()))?;

    let updated = User {
        id: session.user_id.clone(),
        name: session.user_name.clone(),
        password_hash: Some(new_hash),
        rate_limits: None,
    };
    state.db.update_user(&updated)
        .map_err(|e| AdminError::internal(e.0))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn my_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKey>>, AdminError> {
    let session = require_session(&state.admin, &headers)?;
    let keys = state.db.list_api_keys(&session.user_id)
        .map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
struct CreateMyKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
}

async fn create_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateMyKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers)?;

    let key_value = format!("sk-{}", uuid::Uuid::new_v4());
    let ak = ApiKey {
        key: key_value.clone(),
        user_id: session.user_id.clone(),
        name: req.name.unwrap_or_default(),
        enabled: req.enabled.unwrap_or(true),
        expires_at: None,
    };

    state.db.create_api_key(&ak).map_err(|e| AdminError::internal(e.0))?;
    state.auth.reload();

    Ok(Json(serde_json::json!({
        "key": ak.key,
        "user_id": ak.user_id,
        "name": ak.name,
        "enabled": ak.enabled,
    })))
}

async fn delete_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_val): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers)?;

    // Verify the key belongs to the current user
    let keys = state.db.list_api_keys(&session.user_id)
        .map_err(|e| AdminError::internal(e.0))?;
    if !keys.iter().any(|k| k.key == key_val) {
        return Err(AdminError::not_found("Key not found"));
    }

    state.db.delete_api_key(&key_val).map_err(|e| AdminError::internal(e.0))?;
    state.auth.reload();

    Ok(Json(serde_json::json!({ "deleted": key_val })))
}

// ── User CRUD ─────────────────────────────────────────────────────

async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<User>>, AdminError> {
    require_admin(&state.admin, &headers)?;
    let users = state.db.list_users().map_err(|e| AdminError::internal(e.0))?;
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
    require_admin(&state.admin, &headers)?;
    let user = state.db.get_user(&id).map_err(|e| AdminError::internal(e.0))?
        .ok_or_else(|| AdminError::not_found(format!("User '{}' not found", id)))?;
    let keys = state.db.list_api_keys(&id).map_err(|e| AdminError::internal(e.0))?;
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
    require_admin(&state.admin, &headers)?;

    if req.id.is_empty() {
        return Err(AdminError::bad_request("User ID is required"));
    }

    let password_hash = if let Some(ref pw) = req.password {
        if pw.is_empty() {
            None
        } else {
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
    };

    state.db.create_user(&user).map_err(|e| AdminError::internal(e.0))?;
    state.auth.reload();

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
    require_admin(&state.admin, &headers)?;

    let existing = state.db.get_user(&id).map_err(|e| AdminError::internal(e.0))?
        .ok_or_else(|| AdminError::not_found(format!("User '{}' not found", id)))?;

    let user = User {
        id: id.clone(),
        name: req.name.unwrap_or(existing.name),
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
    };

    state.db.update_user(&user).map_err(|e| AdminError::internal(e.0))?;
    state.auth.reload();

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
    require_admin(&state.admin, &headers)?;

    state.db.delete_user(&id).map_err(|e| AdminError::internal(e.0))?;
    state.auth.reload();

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── API Key CRUD (admin manages any user's keys) ──────────────────

async fn list_user_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<ApiKey>>, AdminError> {
    require_admin(&state.admin, &headers)?;
    let keys = state.db.list_api_keys(&user_id).map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
struct CreateKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
}

async fn create_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers)?;

    let key_value = format!("sk-{}", uuid::Uuid::new_v4());
    let ak = ApiKey {
        key: key_value.clone(),
        user_id: user_id.clone(),
        name: req.name.unwrap_or_default(),
        enabled: req.enabled.unwrap_or(true),
        expires_at: None,
    };

    state.db.create_api_key(&ak).map_err(|e| AdminError::internal(e.0))?;
    state.auth.reload();

    Ok(Json(serde_json::json!({
        "key": ak.key,
        "user_id": ak.user_id,
        "name": ak.name,
        "enabled": ak.enabled,
    })))
}

async fn delete_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((_user_id, key_val)): Path<(String, String)>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers)?;

    state.db.delete_api_key(&key_val).map_err(|e| AdminError::internal(e.0))?;
    state.auth.reload();

    Ok(Json(serde_json::json!({ "deleted": key_val })))
}

// ── Channel CRUD ──────────────────────────────────────────────────

async fn list_channels(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Channel>>, AdminError> {
    require_admin(&state.admin, &headers)?;
    let channels = state.db.list_channels().map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(channels))
}

async fn create_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(ch): Json<Channel>,
) -> Result<Json<Channel>, AdminError> {
    require_admin(&state.admin, &headers)?;

    if ch.id.is_empty() {
        return Err(AdminError::bad_request("Channel ID is required"));
    }
    if ch.provider.is_empty() {
        return Err(AdminError::bad_request("Provider is required"));
    }

    state.db.create_channel(&ch).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();

    Ok(Json(ch))
}

async fn update_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut ch): Json<Channel>,
) -> Result<Json<Channel>, AdminError> {
    require_admin(&state.admin, &headers)?;

    ch.id = id;
    state.db.update_channel(&ch).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();

    Ok(Json(ch))
}

async fn delete_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers)?;

    state.db.delete_channel(&id).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── Model CRUD ────────────────────────────────────────────────────

async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    require_admin(&state.admin, &headers)?;
    let models = state.db.list_models().map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(models))
}

async fn create_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut model): Json<Model>,
) -> Result<Json<Model>, AdminError> {
    require_admin(&state.admin, &headers)?;

    model.id = model.id.trim().to_string();
    if model.id.is_empty() {
        return Err(AdminError::bad_request("Model ID is required"));
    }

    state.db.create_model(&model).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();

    Ok(Json(model))
}

async fn update_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut model): Json<Model>,
) -> Result<Json<Model>, AdminError> {
    require_admin(&state.admin, &headers)?;

    model.id = id;
    state.db.update_model(&model).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();

    Ok(Json(model))
}

async fn delete_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers)?;

    state.db.delete_model(&id).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── Public Models (any authenticated user) ────────────────────────

async fn list_public_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    require_session(&state.admin, &headers)?;
    let models = state.db.list_published_models().map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(models))
}

async fn toggle_publish_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers)?;
    let models = state.db.list_models().map_err(|e| AdminError::internal(e.0))?;
    let model = models.iter().find(|m| m.id == id)
        .ok_or_else(|| AdminError::not_found(format!("Model '{}' not found", id)))?;
    let new_status = !model.published;
    state.db.set_model_published(&id, new_status).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();
    Ok(Json(serde_json::json!({ "id": id, "published": new_status })))
}

// ── User Subscriptions ────────────────────────────────────────────

async fn list_my_subscriptions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    let session = require_session(&state.admin, &headers)?;
    let models = state.db.list_subscriptions(&session.user_id)
        .map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(models))
}

async fn subscribe_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers)?;
    state.db.subscribe_user(&session.user_id, &model_id)
        .map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(serde_json::json!({ "subscribed": model_id })))
}

async fn unsubscribe_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers)?;
    state.db.unsubscribe_user(&session.user_id, &model_id)
        .map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(serde_json::json!({ "unsubscribed": model_id })))
}

// ── Routing Rule CRUD ─────────────────────────────────────────────

async fn list_rules(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<RoutingRule>>, AdminError> {
    require_admin(&state.admin, &headers)?;
    let rules = state.db.list_rules().map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(rules))
}

async fn create_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(rule): Json<RoutingRule>,
) -> Result<Json<RoutingRule>, AdminError> {
    require_admin(&state.admin, &headers)?;

    if rule.name.is_empty() {
        return Err(AdminError::bad_request("Rule name is required"));
    }

    state.db.create_rule(&rule).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();

    Ok(Json(rule))
}

async fn update_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(mut rule): Json<RoutingRule>,
) -> Result<Json<RoutingRule>, AdminError> {
    require_admin(&state.admin, &headers)?;

    rule.name = name;
    state.db.update_rule(&rule).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();

    Ok(Json(rule))
}

async fn delete_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, AdminError> {
    require_admin(&state.admin, &headers)?;

    state.db.delete_rule(&name).map_err(|e| AdminError::internal(e.0))?;
    state.routing.reload();

    Ok(Json(serde_json::json!({ "deleted": name })))
}

// ── Usage Logs ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct UsageQuery {
    limit: Option<usize>,
    user_id: Option<String>,
}

async fn get_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageQuery>,
) -> Result<Json<Vec<crate::domain::usage::UsageRecord>>, AdminError> {
    let session = require_session(&state.admin, &headers)?;

    let limit = q.limit.unwrap_or(50);

    // Regular users can only see their own usage
    let user_filter = if session.role == "user" {
        Some(session.user_id.clone())
    } else {
        q.user_id
    };

    let records = state.usage.query(limit, user_filter.as_deref())
        .map_err(|e| AdminError::internal(format!("DB query failed: {}", e)))?;

    Ok(Json(records))
}

async fn get_usage_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<crate::domain::usage::UsageRecord>, AdminError> {
    let _session = require_session(&state.admin, &headers)?;

    let record = state.usage.get_detail(&request_id)
        .map_err(|e| AdminError::internal(format!("DB query failed: {}", e)))?
        .ok_or_else(|| AdminError::not_found("Usage record not found"))?;

    Ok(Json(record))
}

// ── Router ────────────────────────────────────────────────────────

pub fn admin_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/admin/api/login", axum::routing::post(admin_login))
        .route("/admin/api/dashboard", axum::routing::get(admin_dashboard))
        .route("/admin/api/dashboard/aggregations", axum::routing::get(dashboard_aggregations))

        // Current user
        .route("/admin/api/me/password", axum::routing::post(change_my_password))
        .route("/admin/api/me/keys", axum::routing::get(my_keys).post(create_my_key))
        .route("/admin/api/me/keys/{key_val}", axum::routing::delete(delete_my_key))

        // Users
        .route("/admin/api/users", axum::routing::get(list_users).post(create_user))
        .route(
            "/admin/api/users/{id}",
            axum::routing::get(get_user_detail).put(update_user).delete(delete_user),
        )
        // User API keys (admin)
        .route("/admin/api/users/{user_id}/keys", axum::routing::get(list_user_keys).post(create_user_key))
        .route("/admin/api/users/{user_id}/keys/{key_val}", axum::routing::delete(delete_user_key))

        // Channels
        .route("/admin/api/channels", axum::routing::get(list_channels).post(create_channel))
        .route(
            "/admin/api/channels/{id}",
            axum::routing::put(update_channel).delete(delete_channel),
        )

        // Models
        .route("/admin/api/models", axum::routing::get(list_models).post(create_model))
        .route("/admin/api/models/public", axum::routing::get(list_public_models))
        .route("/admin/api/models/{id}/publish", axum::routing::post(toggle_publish_model))
        .route(
            "/admin/api/models/{id}",
            axum::routing::put(update_model).delete(delete_model),
        )

        // Subscriptions
        .route("/admin/api/me/subscriptions", axum::routing::get(list_my_subscriptions))
        .route("/admin/api/me/subscriptions/{model_id}", axum::routing::post(subscribe_model).delete(unsubscribe_model))

        // Routing rules
        .route("/admin/api/rules", axum::routing::get(list_rules).post(create_rule))
        .route(
            "/admin/api/rules/{name}",
            axum::routing::put(update_rule).delete(delete_rule),
        )

        // Usage
        .route("/admin/api/usage", axum::routing::get(get_usage))
        .route("/admin/api/usage/{request_id}", axum::routing::get(get_usage_detail))
}
