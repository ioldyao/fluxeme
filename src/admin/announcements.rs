use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;

use crate::db::AnnouncementRow;
use crate::server::AppState;

use super::*;

#[derive(Deserialize)]
pub(crate) struct AnnouncementInput {
    title: String,
    content: String,
    published: Option<bool>,
}

pub(crate) async fn list_announcements(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<AnnouncementRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:announcements").await?;
    let items = state.db.list_announcements().await.map_err(db_err)?;
    Ok(Json(items))
}

pub(crate) async fn list_published_announcements(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<AnnouncementRow>>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    let items = state
        .db
        .list_published_announcements()
        .await
        .map_err(db_err)?;
    Ok(Json(items))
}

pub(crate) async fn create_announcement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AnnouncementInput>,
) -> Result<Json<AnnouncementRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:announcements").await?;

    if req.title.trim().is_empty() {
        return Err(AdminError::bad_request("Title is required"));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let row = AnnouncementRow {
        id: uuid::Uuid::new_v4().to_string(),
        title: req.title,
        content: req.content,
        created_by: session.user_id,
        created_at: now.clone(),
        updated_at: now,
        published: req.published.unwrap_or(false),
    };
    state.db.create_announcement(&row).await.map_err(db_err)?;
    Ok(Json(row))
}

pub(crate) async fn update_announcement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<AnnouncementInput>,
) -> Result<Json<AnnouncementRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:announcements").await?;

    let existing = state
        .db
        .get_announcement(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Announcement not found"))?;

    if req.title.trim().is_empty() {
        return Err(AdminError::bad_request("Title is required"));
    }

    let row = AnnouncementRow {
        id: existing.id,
        title: req.title,
        content: req.content,
        created_by: existing.created_by,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
        published: req.published.unwrap_or(existing.published),
    };
    state.db.update_announcement(&row).await.map_err(db_err)?;
    Ok(Json(row))
}

pub(crate) async fn delete_announcement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:announcements").await?;

    state.db.delete_announcement(&id).await.map_err(db_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
