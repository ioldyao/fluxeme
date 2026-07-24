use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::Value;

use crate::domain::routing::RoutingRule;
use crate::server::AppState;

use super::*;

// ── Routing Rule CRUD ─────────────────────────────────────────────

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
    Json(rule): Json<RoutingRule>,
) -> Result<Json<RoutingRule>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:rules").await?;

    if rule.name.is_empty() {
        return Err(AdminError::bad_request("Rule name is required"));
    }

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
    Path(name): Path<String>,
    Json(mut rule): Json<RoutingRule>,
) -> Result<Json<RoutingRule>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:rules").await?;

    rule.name = name;
    state.db.update_rule(&rule).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;

    Ok(Json(rule))
}

pub(crate) async fn delete_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:rules").await?;

    state.db.delete_rule(&name).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;

    tracing::info!(
        "admin={} action=delete_rule target={}",
        session.user_id,
        name
    );

    Ok(Json(serde_json::json!({ "deleted": name })))
}
