use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::Value;

use crate::domain::model::{Model, Pricing};
use crate::server::AppState;

use super::*;

// ── Model CRUD ────────────────────────────────────────────────────

pub(crate) async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;
    let models = state.db.list_models().await.map_err(db_err)?;
    // Admin page handles visual grouping on frontend; return raw entries
    Ok(Json(models))
}

pub(crate) async fn create_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut model): Json<Model>,
) -> Result<Json<Model>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;

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

pub(crate) async fn update_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(old_id): Path<String>,
    Json(mut model): Json<Model>,
) -> Result<Json<Model>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;

    state.db.update_model(&old_id, &model).await.map_err(db_err)?;
    state.routing.reload().await;

    tracing::info!(
        "admin={} action=update_model target={}",
        session.user_id,
        old_id
    );

    Ok(Json(model))
}

pub(crate) async fn delete_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;

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

pub(crate) async fn list_public_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    require_session(&state.admin, &headers).await?;
    let models = state.db.list_published_models().await.map_err(db_err)?;
    Ok(Json(merge_same_named_models(models)))
}

pub(crate) async fn toggle_publish_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;
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

pub(crate) async fn update_model_pricing(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(pricing): Json<Pricing>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:model-pricing").await?;
    state.db.set_model_pricing(&id, &pricing).await.map_err(db_err)?;

    tracing::info!(
        "admin={} action=update_model_pricing target={}",
        session.user_id,
        id
    );

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Model Health Check ─────────────────────────────────────────────

pub(crate) async fn model_health_check(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:health").await?;

    let results = state
        .health_probe
        .probe_model(&model_id)
        .await
        .map_err(|e| AdminError::internal(e))?;

    Ok(Json(serde_json::json!({
        "model_id": model_id,
        "channel_results": results,
    })))
}

// ── Probe Results ─────────────────────────────────────────────────

pub(crate) async fn list_probe_results(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::db::ProbeResultRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:health").await?;
    let results = state.health_probe.all_latest_probes().await.map_err(|e| AdminError::internal(e))?;
    Ok(Json(results))
}
