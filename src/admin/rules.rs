use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::Value;

use crate::domain::routing::RoutingRule;
use crate::server::AppState;

use super::*;

// ── System Routing Rule CRUD (admin:rules) ─────────────────────────

pub(crate) async fn list_rules(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<RoutingRule>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:rules").await?;
    let rules = state.db.list_rules().await.map_err(db_err)?;
    Ok(Json(rules))
}

pub(crate) async fn create_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut rule): Json<RoutingRule>,
) -> Result<Json<RoutingRule>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:rules").await?;

    if rule.name.is_empty() {
        return Err(AdminError::bad_request("Rule name is required"));
    }
    if rule.id.is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    rule.scope = "system".to_string();
    let now = chrono::Utc::now().to_rfc3339();
    rule.created_at = now.clone();
    rule.updated_at = now;

    state.db.create_rule(&rule).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;

    tracing::info!(
        "admin={} action=create_rule target={}",
        session.user_id,
        rule.name
    );

    Ok(Json(rule))
}

pub(crate) async fn update_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(rule): Json<RoutingRule>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:rules").await?;

    let mut updated = rule;
    updated.id = id;
    updated.updated_at = chrono::Utc::now().to_rfc3339();
    state.db.update_rule(&updated).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;

    Ok(Json(serde_json::json!({ "updated": true })))
}

pub(crate) async fn delete_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:rules").await?;

    state.db.delete_rule(&id).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;

    tracing::info!("admin={} action=delete_rule target={}", session.user_id, id);

    Ok(Json(serde_json::json!({ "deleted": id })))
}
