use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::user::{ApiKey, User};
use crate::server::AppState;

use super::*;

// ── User CRUD ─────────────────────────────────────────────────────

pub(crate) async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<User>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;
    let users = state.db.list_users().await.map_err(db_err)?;
    Ok(Json(users))
}

#[derive(Serialize)]
pub(crate) struct UserDetail {
    #[serde(flatten)]
    user: User,
    keys: Vec<ApiKey>,
}

pub(crate) async fn get_user_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<UserDetail>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;
    let user = state
        .db
        .get_user(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;
    let keys = state.db.list_api_keys(&id).await.map_err(db_err)?;
    Ok(Json(UserDetail { user, keys }))
}

#[derive(Deserialize)]
pub(crate) struct CreateUserReq {
    id: String,
    name: String,
    password: Option<String>,
    rate_limits: Option<crate::domain::user::RateLimit>,
    role: Option<String>,
    #[serde(default = "default_concurrency")]
    concurrency_limit: u32,
}

fn default_concurrency() -> u32 {
    2000
}

pub(crate) async fn create_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateUserReq>,
) -> Result<Json<User>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

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
        role: req.role.unwrap_or_else(|| "user".to_string()),
        concurrency_limit: req.concurrency_limit,
        currency: "usd".to_string(),
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
pub(crate) struct UpdateUserReq {
    name: Option<String>,
    password: Option<String>,
    rate_limits: Option<crate::domain::user::RateLimit>,
    role: Option<String>,
    #[serde(default)]
    concurrency_limit: Option<u32>,
}

pub(crate) async fn update_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserReq>,
) -> Result<Json<User>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    let existing = state
        .db
        .get_user(&id)
        .await
        .map_err(db_err)?
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
        role: req.role.unwrap_or(existing.role),
        concurrency_limit: req.concurrency_limit.unwrap_or(existing.concurrency_limit),
        currency: existing.currency.clone(),
    };

    state.db.update_user(&user).await.map_err(db_err)?;
    state.auth.reload().await;

    Ok(Json(User {
        password_hash: None,
        ..user
    }))
}

pub(crate) async fn delete_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    state.db.delete_user(&id).await.map_err(db_err)?;
    state.auth.reload().await;

    tracing::info!("admin={} action=delete_user target={}", session.user_id, id);

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── API Key CRUD (admin manages any user's keys) ──────────────────

pub(crate) async fn list_user_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<ApiKey>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;
    let keys = state.db.list_api_keys(&user_id).await.map_err(db_err)?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
pub(crate) struct CreateKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
    expires_at: Option<String>,
    #[serde(default)]
    spend_limit: Option<f64>,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
}

pub(crate) async fn create_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

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

pub(crate) async fn update_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, key_val)): Path<(String, String)>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

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

pub(crate) async fn delete_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((_user_id, key_val)): Path<(String, String)>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    state.db.delete_api_key(&key_val).await.map_err(db_err)?;
    state.auth.reload().await;

    tracing::info!(
        "admin={} action=delete_api_key target={}",
        session.user_id,
        key_val
    );

    Ok(Json(serde_json::json!({ "deleted": key_val })))
}

// ── Toggle User Key (admin) ───────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct ToggleKeyReq {
    enabled: bool,
}

pub(crate) async fn toggle_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, key_val)): Path<(String, String)>,
    Json(req): Json<ToggleKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

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
