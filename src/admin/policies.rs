use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::server::AppState;

use super::*;

#[derive(Serialize)]
pub(crate) struct PolicyRow {
    ptype: String,
    v0: String,
    v1: String,
}

#[derive(Deserialize)]
pub(crate) struct AddPolicyReq {
    role: String,
    permission: String,
}

#[derive(Deserialize)]
pub(crate) struct RemovePolicyReq {
    role: String,
    permission: String,
}

/// List all Casbin policies.
pub(crate) async fn list_policies(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<PolicyRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:policies").await?;

    let rows = state
        .db
        .casbin_list_policies()
        .await
        .map_err(db_err)?;
    let policies: Vec<PolicyRow> = rows
        .into_iter()
        .map(|(_ptype, v0, v1, _v2, _v3, _v4, _v5)| PolicyRow {
            ptype: _ptype,
            v0,
            v1,
        })
        .collect();
    Ok(Json(policies))
}

/// Add a new Casbin policy.
pub(crate) async fn add_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AddPolicyReq>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:policies").await?;

    if req.role.is_empty() || req.permission.is_empty() {
        return Err(AdminError::bad_request("role and permission are required"));
    }

    state
        .db
        .casbin_add_policy("p", &req.role, &req.permission, "", "", "", "")
        .await
        .map_err(db_err)?;

    // Reload Casbin enforcer from DB
    state
        .authz
        .reload(&state.db)
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Remove a Casbin policy.
pub(crate) async fn remove_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<RemovePolicyReq>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:policies").await?;

    if req.role.is_empty() || req.permission.is_empty() {
        return Err(AdminError::bad_request("role and permission are required"));
    }

    state
        .db
        .casbin_remove_policy("p", &req.role, &req.permission)
        .await
        .map_err(db_err)?;

    // Reload Casbin enforcer from DB
    state
        .authz
        .reload(&state.db)
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?;

    Ok(Json(serde_json::json!({ "ok": true })))
}
