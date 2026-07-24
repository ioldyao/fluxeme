use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::config::types::GatewayRuntimeConfig;
use crate::server::AppState;

use super::*;

// ── Settings ──────────────────────────────────────────────────────

pub(crate) async fn get_allow_private_ips(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let value = state
        .db
        .get_setting("allow_private_ips")
        .await
        .map_err(db_err)?;
    // Default to true when no setting is stored (matches AtomicBool default)
    let enabled = value.as_deref() != Some("false");
    Ok(Json(serde_json::json!({ "enabled": enabled })))
}

#[derive(Deserialize)]
pub(crate) struct AllowPrivateIpsReq {
    enabled: bool,
}

pub(crate) async fn set_allow_private_ips(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AllowPrivateIpsReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let value = if req.enabled { "true" } else { "false" };
    state
        .db
        .set_setting("allow_private_ips", value)
        .await
        .map_err(db_err)?;
    crate::provider::set_allow_private_ips(req.enabled);
    Ok(Json(serde_json::json!({ "enabled": req.enabled })))
}

// ── Gateway Config ──────────────────────────────────────────────────

pub(crate) async fn get_gateway_config_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<GatewayRuntimeConfig>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:gateway").await?;
    let config = state.db.get_gateway_config().await.map_err(db_err)?;
    Ok(Json(config))
}

pub(crate) async fn set_gateway_config_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(config): Json<GatewayRuntimeConfig>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:gateway").await?;
    // Validate and persist
    state.db.set_gateway_config(&config).await.map_err(db_err)?;
    // Update in-memory config
    *state.gateway_config.write().unwrap() = config;
    Ok(Json(serde_json::json!({ "ok": true })))
}
