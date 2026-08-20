//! SkillHub HTTP handlers（组合根层）。
//!
//! 职责边界：只做会话/权限校验 + 请求/响应形状，业务逻辑全部委托给
//! `fluxeme_skillhub` 子系统。存储归属：目录/版本/安装为业务数据（PG），
//! 技能包 zip 落盘（PG 行存路径），观测/计费不在本阶段。

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, Method};
use axum::response::Response;
use axum::Json;
use serde::Deserialize;

use fluxeme_skillhub::domain::{
    CreateSkill, InstalledSkill, PackageStatus, SkillInstallRow, SkillRow, SkillVersionRow,
    UpdateSkill, Visibility,
};
use fluxeme_skillhub::SkillHubError;

use crate::server::AppState;

use super::*;

// ── 请求/响应形状 ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct CreateSkillInput {
    slug: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    tags: Vec<String>,
    /// public / internal / private，缺省 internal
    #[serde(default)]
    visibility: String,
}

#[derive(Deserialize)]
pub(crate) struct UpdateSkillInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    visibility: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct StatusInput {
    status: String,
}

#[derive(Deserialize)]
pub(crate) struct InstallInput {
    #[serde(default)]
    version: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct DownloadQuery {
    #[serde(default)]
    version: Option<String>,
}

fn skillhub_err(e: SkillHubError) -> AdminError {
    match e {
        SkillHubError::NotFound(m) => AdminError::not_found(m),
        SkillHubError::Invalid(m) => AdminError::bad_request(m),
        // AdminError 无 409 变体，冲突映射为 400
        SkillHubError::Conflict(m) => AdminError::bad_request(m),
        SkillHubError::Storage(m) => AdminError::internal(m),
        SkillHubError::Db(m) => AdminError::internal(m),
        SkillHubError::Internal(m) => AdminError::internal(m),
    }
}

// ── 管理端（admin:skillhub） ────────────────────────────────────────────

pub(crate) async fn list_skills(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Json<Vec<SkillRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    let items = state
        .skillhub
        .list_skills(q.status.as_deref(), q.visibility.as_deref())
        .await
        .map_err(skillhub_err)?;
    Ok(Json(items))
}

pub(crate) async fn create_skill(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateSkillInput>,
) -> Result<Json<SkillRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    let visibility = Visibility::parse(&req.visibility).unwrap_or(Visibility::Internal);
    let row = state
        .skillhub
        .create_skill(CreateSkill {
            slug: req.slug,
            name: req.name,
            description: req.description,
            category: req.category,
            tags: req.tags,
            visibility,
            author_id: session.user_id,
        })
        .await
        .map_err(skillhub_err)?;
    Ok(Json(row))
}

pub(crate) async fn get_skill(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<SkillRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    let row = state
        .skillhub
        .get_skill(&id)
        .await
        .map_err(skillhub_err)?
        .ok_or_else(|| AdminError::not_found("Skill not found"))?;
    Ok(Json(row))
}

pub(crate) async fn update_skill(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateSkillInput>,
) -> Result<Json<SkillRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    let visibility = req.visibility.as_deref().and_then(Visibility::parse);
    let row = state
        .skillhub
        .update_skill(
            &id,
            UpdateSkill {
                name: req.name,
                description: req.description,
                category: req.category,
                tags: req.tags,
                visibility,
            },
        )
        .await
        .map_err(skillhub_err)?;
    Ok(Json(row))
}

pub(crate) async fn delete_skill(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    state
        .skillhub
        .delete_skill(&id)
        .await
        .map_err(skillhub_err)?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub(crate) async fn set_skill_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<StatusInput>,
) -> Result<Json<SkillRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    let status = PackageStatus::parse(&req.status)
        .ok_or_else(|| AdminError::bad_request(format!("invalid status '{}'", req.status)))?;
    let row = state
        .skillhub
        .set_status(&id, status)
        .await
        .map_err(skillhub_err)?;
    Ok(Json(row))
}

pub(crate) async fn list_versions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<SkillVersionRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;
    let items = state
        .skillhub
        .list_versions(&id)
        .await
        .map_err(skillhub_err)?;
    Ok(Json(items))
}

/// 上传技能包 zip（multipart：version / changelog / file）。
pub(crate) async fn upload_artifact(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(skill_id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<SkillVersionRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:skillhub").await?;

    let mut version: Option<String> = None;
    let mut changelog: Option<String> = None;
    let mut file_bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AdminError::bad_request(format!("multipart: {e}")))?
    {
        match field.name() {
            Some("version") => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| AdminError::bad_request(format!("version field: {e}")))?;
                version = Some(v.trim().to_string());
            }
            Some("changelog") => {
                changelog = field.text().await.ok().map(|s| s.trim().to_string());
            }
            Some("file") => {
                let b = field
                    .bytes()
                    .await
                    .map_err(|e| AdminError::bad_request(format!("file field: {e}")))?;
                file_bytes = Some(b.to_vec());
            }
            _ => {}
        }
    }
    let version =
        version.ok_or_else(|| AdminError::bad_request("missing 'version' field in multipart"))?;
    let bytes =
        file_bytes.ok_or_else(|| AdminError::bad_request("missing 'file' field in multipart"))?;

    let row = state
        .skillhub
        .upload_artifact(&skill_id, &version, changelog.as_deref(), &session.user_id, bytes)
        .await
        .map_err(|e| {
            tracing::warn!(skill_id, version, "skillhub upload failed: {e}");
            skillhub_err(e)
        })?;
    Ok(Json(row))
}

// ── 用户端（登录即可） ──────────────────────────────────────────────────

pub(crate) async fn list_published_skills(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<SkillRow>>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    let items = state
        .skillhub
        .list_published_skills()
        .await
        .map_err(skillhub_err)?;
    Ok(Json(items))
}

pub(crate) async fn get_published_skill(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<SkillRow>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    let row = state
        .skillhub
        .get_published_skill(&slug)
        .await
        .map_err(skillhub_err)?
        .ok_or_else(|| AdminError::not_found("Skill not found"))?;
    Ok(Json(row))
}

/// 发布态技能的版本列表（用户端详情页）。
pub(crate) async fn list_published_versions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<Vec<SkillVersionRow>>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    let items = state
        .skillhub
        .list_published_versions(&slug)
        .await
        .map_err(skillhub_err)?;
    Ok(Json(items))
}

/// 下载技能包 zip（attachment）。版本缺省 = 当前发布版本。
pub(crate) async fn download_skill(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Query(q): Query<DownloadQuery>,
) -> Result<Response, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    let payload = state
        .skillhub
        .download(&slug, q.version.as_deref())
        .await
        .map_err(skillhub_err)?;
    let mut resp = Response::new(Body::from(payload.bytes));
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&payload.content_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    resp.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", payload.filename))
            .map_err(|e| AdminError::internal(format!("bad content-disposition: {e}")))?,
    );
    resp.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&payload.size.to_string())
            .map_err(|e| AdminError::internal(format!("bad content-length: {e}")))?,
    );
    Ok(resp)
}

/// 记录安装（开通）。`published AND 包存在` 门禁，UPSERT 幂等。
pub(crate) async fn install_skill(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    body: Option<Json<InstallInput>>,
) -> Result<Json<SkillInstallRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let version = body.and_then(|b| b.0.version);
    let row = state
        .skillhub
        .record_install(&session.user_id, &slug, version.as_deref())
        .await
        .map_err(skillhub_err)?;
    Ok(Json(row))
}

pub(crate) async fn my_skills(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<InstalledSkill>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let items = state
        .skillhub
        .my_skills(&session.user_id)
        .await
        .map_err(skillhub_err)?;
    Ok(Json(items))
}

/// 技能级运行状态（组合根：调 skill-backing 的 runtime_statuses）。
pub(crate) async fn runtime_statuses(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<fluxeme_skill_backing::domain::SkillRuntimeStatus>>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    let items = state
        .skill_backing
        .runtime_statuses()
        .await
        .map_err(|e| AdminError::internal(e.0))?;
    Ok(Json(items))
}

/// Skill Runtime 数据面入口：挂 `/api/skills/{slug}/{*rest}`。
/// 只做提取/委托，业务在 skill-backing 子系统（Backing 不感知 AppState）。
pub(crate) async fn runtime_proxy(
    State(state): State<Arc<AppState>>,
    Path(path): Path<fluxeme_skill_backing::RuntimePath>,
    method: Method,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    state
        .skill_backing
        .handle_runtime_request(&path.slug, &path.rest, &method, &headers, body)
        .await
}
