use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::channel::Channel;
use crate::server::AppState;

use super::*;

// ── Channel CRUD ──────────────────────────────────────────────────

pub(crate) async fn list_channels(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Channel>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:channels").await?;
    let channels = state.db.list_channels().await.map_err(db_err)?;
    Ok(Json(channels))
}

pub(crate) async fn create_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut ch): Json<Channel>,
) -> Result<Json<Channel>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:channels").await?;

    if ch.id.is_empty() {
        return Err(AdminError::bad_request("Channel ID is required"));
    }
    if ch.provider.is_empty() {
        return Err(AdminError::bad_request("Provider is required"));
    }

    // Encrypt endpoint API keys before storing
    let secret = state.admin.secret.clone();
    for ep in &mut ch.endpoints {
        if !ep.api_key.is_empty() {
            ep.api_key = crate::crypto::encrypt_store(&ep.api_key, &secret);
        }
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

pub(crate) async fn update_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut ch): Json<Channel>,
) -> Result<Json<Channel>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:channels").await?;

    // Encrypt endpoint API keys before storing
    let secret = state.admin.secret.clone();
    for ep in &mut ch.endpoints {
        if !ep.api_key.is_empty() && !ep.api_key.starts_with("enc:") {
            ep.api_key = crate::crypto::encrypt_store(&ep.api_key, &secret);
        }
    }

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

pub(crate) async fn delete_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:channels").await?;

    state.db.delete_channel(&id).await.map_err(db_err)?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=delete_channel target={}",
        session.user_id,
        id
    );

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── Load balancer/health API ─────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct ToggleEndpointBody {
    enabled: bool,
}

#[derive(Serialize)]
pub(crate) struct EndpointHealthItem {
    endpoint_id: i64,
    url: String,
    enabled: bool,
    available: bool,
}

use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct ChannelHealthResponse {
    channel_id: String,
    endpoints: Vec<EndpointHealthItem>,
    /// Last health-check probe success for this channel (None = never probed).
    #[serde(skip_serializing_if = "Option::is_none")]
    probe_success: Option<bool>,
    /// Last health-check probe latency in ms.
    #[serde(skip_serializing_if = "Option::is_none")]
    probe_latency_ms: Option<u64>,
}

pub(crate) async fn get_channel_health(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ChannelHealthResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:channels").await?;
    let eps = state.routing.channel_health(&id);
    let ch = state.db.get_channel(&id).await.map_err(db_err)?;
    let channel_id = ch.as_ref().map(|c| c.id.clone()).unwrap_or(id);
    // Read latest probe result from database (persisted across restarts)
    let latest_probe = state.db.all_latest_probe_results().await.map_err(db_err)?;
    let (probe_success, probe_latency_ms) = latest_probe
        .iter()
        .find(|r| r.channel_id == channel_id)
        .map(|r| (Some(r.success), Some(r.latency_ms)))
        .unwrap_or((None, None));
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
    Ok(Json(ChannelHealthResponse { channel_id, endpoints, probe_success, probe_latency_ms }))
}

pub(crate) async fn toggle_endpoint(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(body): Json<ToggleEndpointBody>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:channels").await?;
    state
        .db
        .update_endpoint_enabled(id, body.enabled)
        .await.map_err(db_err)?;
    state.routing.set_endpoint_enabled(id, body.enabled);
    Ok(Json(serde_json::json!({ "success": true })))
}

pub(crate) async fn list_upstream_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<crate::service::health::UpstreamModelInfo>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:channels").await?;
    let models = state
        .health
        .list_upstream_models(&id)
        .await
        .map_err(AdminError::internal)?;
    Ok(Json(models))
}
