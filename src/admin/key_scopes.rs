//! API Key Scope 管理（skill 资源）——Platform API Key 的一等公民模型。
//!
//! 挂 `admin:skillhub` 权限；scope = `skill:{slug}:invoke`，让指定 API Key
//! 可以调用某个已发布技能的数据面端点。

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;

use crate::server::AppState;

use super::*;

#[derive(Deserialize)]
pub(crate) struct AddScopeInput {
    /// API Key（sk_...）
    key: String,
    /// invoke / connect（skill 资源用 invoke）
    #[serde(default = "default_action")]
    action: String,
}

fn default_action() -> String {
    "invoke".into()
}

fn valid_action(action: &str) -> bool {
    matches!(action, "invoke" | "connect")
}

pub(crate) async fn list_skill_scopes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    let scopes = state
        .db
        .list_scopes_by_resource("skill", &slug)
        .await
        .map_err(db_err)?;
    let out = scopes
        .into_iter()
        .map(|(s, key_name)| {
            serde_json::json!({
                "id": s.id,
                "api_key_id": s.api_key_id,
                "key_name": key_name,
                "resource_type": s.resource_type,
                "resource_id": s.resource_id,
                "action": s.action,
                "created_at": s.created_at,
            })
        })
        .collect();
    Ok(Json(out))
}

pub(crate) async fn add_skill_scope(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(req): Json<AddScopeInput>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    if req.key.trim().is_empty() {
        return Err(AdminError::bad_request("API key is required"));
    }
    if !valid_action(&req.action) {
        return Err(AdminError::bad_request(
            "action must be 'invoke' or 'connect'",
        ));
    }
    if state
        .db
        .lookup_key(req.key.trim())
        .await
        .map_err(db_err)?
        .is_none()
    {
        return Err(AdminError::bad_request("API key not found"));
    }
    state
        .db
        .add_api_key_scope(req.key.trim(), "skill", &slug, &req.action)
        .await
        .map_err(db_err)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "api_key_id": req.key.trim(),
        "resource_type": "skill",
        "resource_id": slug,
        "action": req.action,
    })))
}

pub(crate) async fn delete_skill_scope(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((slug, scope_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    state
        .db
        .delete_api_key_scope(&scope_id)
        .await
        .map_err(db_err)?;
    Ok(Json(serde_json::json!({ "ok": true, "skill": slug })))
}
