use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono_tz::Tz;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::user::{ApiKey, User};
use crate::server::AppState;

use super::*;

// ── Current User ("Me") ───────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct ChangePasswordReq {
    current_password: String,
    new_password: String,
}

pub(crate) async fn change_my_password(
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
        concurrency_limit: existing.concurrency_limit,
        currency: existing.currency.clone(),
    };
    state.db.update_user(&updated).await.map_err(db_err)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct UpdateTimezoneReq {
    timezone: String,
}

pub(crate) async fn get_my_timezone(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    Ok(Json(serde_json::json!({ "timezone": tz })))
}

pub(crate) async fn update_my_timezone(
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

#[derive(Deserialize)]
pub(crate) struct UpdateCurrencyReq {
    currency: String,
}

pub(crate) async fn get_my_currency(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let cur = state.db.get_user_currency(&session.user_id).await.map_err(db_err)?;
    Ok(Json(serde_json::json!({ "currency": cur })))
}

pub(crate) async fn update_my_currency(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UpdateCurrencyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    state.db.update_user_currency(&session.user_id, &req.currency).await.map_err(db_err)?;
    Ok(Json(serde_json::json!({ "ok": true, "currency": req.currency })))
}

pub(crate) async fn my_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKey>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let keys = state.db.list_api_keys(&session.user_id).await.map_err(db_err)?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
pub(crate) struct CreateMyKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
    expires_at: Option<String>,
    #[serde(default)]
    spend_limit: Option<f64>,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
}

pub(crate) async fn create_my_key(
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
pub(crate) struct UpdateMyKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
    expires_at: Option<String>,
    #[serde(default)]
    spend_limit: Option<f64>,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
}

pub(crate) async fn update_my_key(
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

pub(crate) async fn delete_my_key(
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
pub(crate) struct ToggleKeyReq {
    enabled: bool,
}

pub(crate) async fn toggle_my_key(
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

/// List all granted permissions for the current session.
pub(crate) async fn my_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let all_known = [
        "admin:dashboard",
        "admin:users",
        "admin:channels",
        "admin:models",
        "admin:model-pricing",
        "admin:rules",
        "admin:usage",
        "admin:bills",
        "admin:recharge-keys",
        "admin:health",
        "admin:settings",
        "admin:gateway",
    ];
    let mut granted = Vec::new();
    for perm in &all_known {
        if state.authz.enforce(&session.role, perm).await {
            granted.push(perm.to_string());
        }
    }
    Ok(Json(granted))
}
