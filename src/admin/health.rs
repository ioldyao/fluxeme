use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Serialize;

use crate::server::AppState;

use super::*;

// ── Health Check ──────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct HealthCheckResult {
    models_updated: usize,
    channels_checked: usize,
    channels_failed: usize,
}

pub(crate) async fn health_check_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<HealthCheckResult>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:health").await?;
    let (models_updated, channels_checked, channels_failed) = state
        .health
        .check_all_channels()
        .await
        .map_err(AdminError::internal)?;
    Ok(Json(HealthCheckResult {
        models_updated,
        channels_checked,
        channels_failed,
    }))
}

pub(crate) async fn health_check_channel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::service::health::ChannelHealthResult>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:channels").await?;
    let result = state
        .health
        .check_channel(&id)
        .await
        .map_err(AdminError::internal)?;
    Ok(Json(result))
}
