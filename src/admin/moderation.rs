use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::Value;

use crate::domain::moderation::ContentFilterRule;
use crate::server::AppState;

use super::*;

// ── Content Moderation Handlers ─────────────────────────────────────

pub(crate) async fn get_content_moderation_enabled(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:moderation").await?;
    let value = state.db.get_setting("content_moderation_enabled").await.map_err(db_err)?;
    let enabled = value.as_deref() != Some("false");
    Ok(Json(serde_json::json!({ "enabled": enabled })))
}

pub(crate) async fn set_content_moderation_enabled(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:moderation").await?;
    let enabled = body.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    state.db.set_setting("content_moderation_enabled", if enabled { "true" } else { "false" })
        .await.map_err(db_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub(crate) async fn list_filter_rules(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:moderation").await?;
    let rules = state.db.list_filter_rules().await.map_err(db_err)?;
    Ok(Json(serde_json::to_value(rules).unwrap_or_default()))
}

pub(crate) async fn create_filter_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut rule): Json<ContentFilterRule>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:moderation").await?;
    if rule.id.is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    let now = chrono::Utc::now().to_rfc3339();
    if rule.created_at.is_empty() {
        rule.created_at.clone_from(&now);
    }
    rule.updated_at = now;
    state.db.create_filter_rule(&rule).await.map_err(db_err)?;
    state.content_filter.reload().await;
    Ok(Json(serde_json::json!({ "ok": true, "id": rule.id })))
}

pub(crate) async fn update_filter_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(mut rule): Json<ContentFilterRule>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:moderation").await?;
    rule.id = id;
    rule.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.update_filter_rule(&rule).await.map_err(db_err)?;
    state.content_filter.reload().await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub(crate) async fn delete_filter_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:moderation").await?;
    state.db.delete_filter_rule(&id).await.map_err(db_err)?;
    state.content_filter.reload().await;
    Ok(Json(serde_json::json!({ "ok": true })))
}
