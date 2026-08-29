use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::db::ManagementApiKey;
use crate::server::AppState;

use super::*;

fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

fn metadata(key: &ManagementApiKey) -> Value {
    serde_json::json!({
        "id": key.id,
        "key_prefix": key.key_prefix,
        "name": key.name,
        "enabled": key.enabled,
        "created_by": key.created_by,
        "created_at": key.created_at,
        "expires_at": key.expires_at,
        "last_used_at": key.last_used_at,
    })
}

fn is_management_credential(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| value.starts_with("mk-"))
        || extract_cookie_value(headers, HOST_SESSION_COOKIE_NAME)
            .is_some_and(|value| value.starts_with("mk-"))
        || extract_cookie_value(headers, SESSION_COOKIE_NAME)
            .is_some_and(|value| value.starts_with("mk-"))
}

async fn authorize(state: &AppState, headers: &HeaderMap) -> Result<SessionInfo, AdminError> {
    // Management-key lifecycle operations require the browser session. A
    // management key may call backend control-plane APIs, but cannot mint,
    // rotate, disable, or delete another management key.
    if is_management_credential(headers) {
        return Err(AdminError::forbidden(
            "Management keys cannot manage management-key lifecycle",
        ));
    }
    let session = require_session(&state.admin, headers).await?;
    check_perm(&state.authz, &session, "admin:management-keys").await?;
    Ok(session)
}

pub(crate) async fn list_management_api_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Value>>, AdminError> {
    authorize(&state, &headers).await?;
    let keys = state.db.list_management_api_keys().await.map_err(db_err)?;
    Ok(Json(keys.iter().map(metadata).collect()))
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateManagementApiKeyReq {
    #[serde(default)]
    name: String,
    expires_at: Option<String>,
}

pub(crate) async fn create_management_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateManagementApiKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = authorize(&state, &headers).await?;
    let name = req.name.trim().to_string();
    if name.len() > 100 {
        return Err(AdminError::bad_request("Management key name is too long"));
    }
    let expires_at = req
        .expires_at
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(ref value) = expires_at {
        let expires = DateTime::parse_from_rfc3339(value)
            .map_err(|_| AdminError::bad_request("expires_at must be RFC3339"))?;
        if expires.with_timezone(&Utc) <= Utc::now() {
            return Err(AdminError::bad_request("expires_at must be in the future"));
        }
    }

    let raw_key = format!("mk-{}", uuid::Uuid::new_v4());
    let key = ManagementApiKey {
        id: uuid::Uuid::new_v4().to_string(),
        key_hash: hash_key(&raw_key),
        key_prefix: format!("{}...", &raw_key[..11]),
        name,
        enabled: true,
        created_by: session.user_id.clone(),
        created_at: Utc::now().to_rfc3339(),
        expires_at,
        last_used_at: None,
    };

    state
        .db
        .create_management_api_key(&key)
        .await
        .map_err(db_err)?;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=create_management_api_key target={}",
        session.user_id,
        key.id
    );

    Ok(Json(serde_json::json!({
        "key": raw_key,
        "metadata": metadata(&key),
    })))
}

#[derive(Debug, Deserialize)]
pub(crate) struct SetManagementApiKeyEnabledReq {
    enabled: bool,
}

pub(crate) async fn set_management_api_key_enabled(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<SetManagementApiKeyEnabledReq>,
) -> Result<Json<Value>, AdminError> {
    let session = authorize(&state, &headers).await?;
    let updated = state
        .db
        .set_management_api_key_enabled(&id, req.enabled)
        .await
        .map_err(db_err)?;
    if !updated {
        return Err(AdminError::not_found("Management key not found"));
    }
    notify_config_changed(&state).await;
    tracing::info!(
        "admin={} action=set_management_api_key_enabled target={} enabled={}",
        session.user_id,
        id,
        req.enabled
    );
    Ok(Json(
        serde_json::json!({ "id": id, "enabled": req.enabled }),
    ))
}

pub(crate) async fn delete_management_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = authorize(&state, &headers).await?;
    let deleted = state
        .db
        .delete_management_api_key(&id)
        .await
        .map_err(db_err)?;
    if !deleted {
        return Err(AdminError::not_found("Management key not found"));
    }
    notify_config_changed(&state).await;
    tracing::info!(
        "admin={} action=delete_management_api_key target={}",
        session.user_id,
        id
    );
    Ok(Json(serde_json::json!({ "deleted": id })))
}

pub(crate) async fn authenticate_management_key(
    admin: &AdminModule,
    presented: &str,
) -> Result<SessionInfo, AdminError> {
    if !presented.starts_with("mk-") {
        return Err(AdminError::unauthorized("Invalid management API key"));
    }
    let presented_hash = hash_key(presented);
    admin
        .rate_limiter
        .check_rpm(&format!("management-key:{presented_hash}"), 120)
        .await
        .map_err(|_| AdminError::too_many_requests("Too many management API requests"))?;
    let key = admin
        .db
        .lookup_management_api_key(&presented_hash)
        .await
        .map_err(|_| AdminError::unauthorized("Invalid management API key"))?
        .ok_or_else(|| AdminError::unauthorized("Invalid management API key"))?;
    if !key.enabled {
        return Err(AdminError::unauthorized("Management API key is disabled"));
    }
    if let Some(expires_at) = key.expires_at.as_deref() {
        let expires = DateTime::parse_from_rfc3339(expires_at)
            .map_err(|_| AdminError::unauthorized("Management API key has invalid expiration"))?;
        if expires.with_timezone(&Utc) <= Utc::now() {
            return Err(AdminError::unauthorized("Management API key has expired"));
        }
    }
    let creator = admin
        .db
        .get_user(&key.created_by)
        .await
        .map_err(|_| AdminError::unauthorized("Management API key is unavailable"))?
        .ok_or_else(|| AdminError::unauthorized("Management API key is unavailable"))?;
    if creator.status != crate::domain::user::USER_STATUS_ACTIVE || creator.role != "admin" {
        return Err(AdminError::unauthorized(
            "Management API key is unavailable",
        ));
    }
    if let Err(error) = admin
        .db
        .touch_management_api_key(&key.id, &Utc::now().to_rfc3339())
        .await
    {
        tracing::warn!(key_id = %key.id, %error, "Failed to update management key last-used timestamp");
    }
    Ok(SessionInfo {
        user_id: creator.id,
        user_name: creator.name,
        role: creator.role,
        token_version: creator.token_version,
    })
}
