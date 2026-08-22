use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::sso::SsoOrg;
use crate::domain::user::{ApiKey, User, USER_STATUS_ACTIVE, USER_STATUS_SUSPENDED};
use crate::server::AppState;

use super::*;

// ── User CRUD ─────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ListUsersQuery {
    status: Option<String>,
}

fn parse_user_status_filter(status: Option<&str>) -> Result<Option<&'static str>, AdminError> {
    match status.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("all") => Ok(None),
        Some(USER_STATUS_ACTIVE) => Ok(Some(USER_STATUS_ACTIVE)),
        Some(USER_STATUS_SUSPENDED) => Ok(Some(USER_STATUS_SUSPENDED)),
        Some(_) => Err(AdminError::bad_request("Invalid user status filter")),
    }
}

async fn ensure_not_last_active_admin(
    state: &AppState,
    user: &User,
    action: &str,
) -> Result<(), AdminError> {
    if user.role != "admin" || user.status != USER_STATUS_ACTIVE {
        return Ok(());
    }

    let active_admin_count = state
        .db
        .count_admins(Some(USER_STATUS_ACTIVE))
        .await
        .map_err(db_err)?;
    if active_admin_count <= 1 {
        return Err(AdminError::bad_request(format!(
            "Cannot {action} the last active admin"
        )));
    }

    Ok(())
}

/// User row for the admin user list, enriched with the IdP organizations
/// (Keycloak Organizations) the user belongs to.
#[derive(Serialize)]
pub(crate) struct UserListRow {
    #[serde(flatten)]
    user: User,
    #[serde(default)]
    sso_orgs: Vec<SsoOrg>,
}

pub(crate) async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListUsersQuery>,
) -> Result<Json<Vec<UserListRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;
    let status = parse_user_status_filter(query.status.as_deref())?;
    let users = state.db.list_users(status).await.map_err(db_err)?;

    // Load all SSO user orgs once and map by user_id (avoids N+1).
    let orgs_map: HashMap<String, Vec<SsoOrg>> = state
        .db
        .list_sso_user_orgs()
        .await
        .map_err(db_err)?
        .into_iter()
        .filter_map(|(uid, json)| {
            serde_json::from_str::<Vec<SsoOrg>>(&json)
                .ok()
                .map(|orgs| (uid, orgs))
        })
        .collect();

    let rows = users
        .into_iter()
        .map(|user| UserListRow {
            sso_orgs: orgs_map.get(&user.id).cloned().unwrap_or_default(),
            user,
        })
        .collect();
    Ok(Json(rows))
}

#[derive(Serialize)]
pub(crate) struct UserDetail {
    #[serde(flatten)]
    user: User,
    keys: Vec<ApiKey>,
}

pub(crate) async fn get_user_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<UserDetail>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;
    let user = state
        .db
        .get_user(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;
    let keys = state.db.list_api_keys(&id).await.map_err(db_err)?;
    Ok(Json(UserDetail { user, keys }))
}

#[derive(Deserialize)]
pub(crate) struct CreateUserReq {
    id: String,
    name: String,
    password: Option<String>,
    rate_limits: Option<crate::domain::user::RateLimit>,
    role: Option<String>,
    #[serde(default = "default_concurrency")]
    concurrency_limit: u32,
}

fn default_concurrency() -> u32 {
    2000
}

pub(crate) async fn create_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateUserReq>,
) -> Result<Json<User>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    if req.id.is_empty() {
        return Err(AdminError::bad_request("User ID is required"));
    }

    let password_hash = if let Some(ref pw) = req.password {
        if pw.is_empty() {
            None
        } else {
            validate_password(pw)?;
            Some(bcrypt::hash(pw, 10).map_err(|e| AdminError::internal(e.to_string()))?)
        }
    } else {
        None
    };

    let user = User {
        id: req.id,
        name: req.name,
        password_hash,
        rate_limits: req.rate_limits,
        timezone: "UTC".to_string(),
        token_version: 0,
        role: req.role.unwrap_or_else(|| "user".to_string()),
        concurrency_limit: req.concurrency_limit,
        currency: "usd".to_string(),
        status: "active".to_string(),
        suspended_at: None,
    };

    state.db.create_user(&user).await.map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=create_user target={}",
        session.user_id,
        user.id
    );

    Ok(Json(User {
        password_hash: None,
        ..user
    }))
}

#[derive(Deserialize)]
pub(crate) struct UpdateUserReq {
    name: Option<String>,
    password: Option<String>,
    rate_limits: Option<crate::domain::user::RateLimit>,
    role: Option<String>,
    #[serde(default)]
    concurrency_limit: Option<u32>,
}

pub(crate) async fn update_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserReq>,
) -> Result<Json<User>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    let existing = state
        .db
        .get_user(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;

    if req
        .role
        .as_deref()
        .is_some_and(|role| role != existing.role)
    {
        ensure_not_last_active_admin(&state, &existing, "demote").await?;
    }

    let next_password_hash = if let Some(pw) = req.password {
        if pw.is_empty() {
            None // keep existing
        } else {
            validate_password(&pw)?;
            Some(bcrypt::hash(pw, 10).map_err(|e| AdminError::internal(e.to_string()))?)
        }
    } else {
        None // keep existing
    };

    let user = state
        .db
        .update_user_admin_fields(
            &id,
            req.name,
            next_password_hash,
            req.rate_limits,
            req.role,
            req.concurrency_limit,
        )
        .await
        .map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    Ok(Json(User {
        password_hash: None,
        ..user
    }))
}

pub(crate) async fn suspend_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<User>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    let existing = state
        .db
        .get_user(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;
    ensure_not_last_active_admin(&state, &existing, "suspend").await?;

    let suspended_at = Utc::now();
    let user = state
        .db
        .suspend_user(&id, &suspended_at)
        .await
        .map_err(db_err_bad_request)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=suspend_user target={}",
        session.user_id,
        id
    );

    Ok(Json(User {
        password_hash: None,
        ..user
    }))
}

pub(crate) async fn restore_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<User>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    let existing = state
        .db
        .get_user(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;
    if existing.status != USER_STATUS_SUSPENDED {
        return Err(AdminError::bad_request(
            "Only suspended users can be restored",
        ));
    }

    let user = state
        .db
        .restore_user(&id)
        .await
        .map_err(db_err_bad_request)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=restore_user target={}",
        session.user_id,
        id
    );

    Ok(Json(User {
        password_hash: None,
        ..user
    }))
}

pub(crate) async fn delete_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    let existing = state
        .db
        .get_user(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;
    ensure_not_last_active_admin(&state, &existing, "delete").await?;

    state
        .db
        .delete_user(&id)
        .await
        .map_err(db_err_bad_request)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    tracing::info!("admin={} action=delete_user target={}", session.user_id, id);

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── API Key CRUD (admin manages any user's keys) ──────────────────

pub(crate) async fn list_user_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
) -> Result<Json<Vec<ApiKey>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;
    let keys = state.db.list_api_keys(&user_id).await.map_err(db_err)?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
pub(crate) struct CreateKeyReq {
    pub(crate) name: Option<String>,
    pub(crate) enabled: Option<bool>,
    pub(crate) expires_at: Option<String>,
    #[serde(default, with = "rust_decimal::serde::float_option")]
    pub(crate) spend_limit: Option<Decimal>,
    #[serde(default)]
    pub(crate) allowed_models: Option<Vec<String>>,
    /// Team scope for this key. `Some` = team-shared key, `None` = personal.
    #[serde(default)]
    pub(crate) team_id: Option<String>,
    /// 访问范围 = 资源类型（model / skill / mcp）。缺省 model+skill。
    #[serde(default)]
    pub(crate) scopes: Option<Vec<String>>,
    pub(crate) billing_group_id: Option<String>,
}

pub(crate) async fn create_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;
    let billing_group_id = req
        .billing_group_id
        .as_deref()
        .ok_or_else(|| AdminError::bad_request("billing_group_id is required"))?;
    let billing_group = state
        .db
        .get_billing_group(billing_group_id)
        .await
        .map_err(db_err)?
        .filter(|group| group.is_active())
        .ok_or_else(|| AdminError::bad_request("Billing group is not active"))?;
    let key_value = format!("sk-{}", uuid::Uuid::new_v4());
    let ak = ApiKey {
        key: key_value.clone(),
        user_id: user_id.clone(),
        name: req.name.unwrap_or_default(),
        enabled: req.enabled.unwrap_or(true),
        expires_at: req.expires_at,
        spend_limit: req.spend_limit,
        allowed_models: req.allowed_models,
        team_id: req.team_id,
        scopes: None,
        billing_group_id: billing_group.id.clone(),
        billing_payment_mode: billing_group.payment_mode,
    };

    state.db.create_api_key(&ak).await.map_err(db_err)?;
    // 访问范围：缺省 model+skill；写入 api_key_scopes(resource_id='*')。
    let scopes = req
        .scopes
        .clone()
        .unwrap_or_else(|| vec!["model".to_string(), "skill".to_string()]);
    for scope in scopes {
        if matches!(scope.as_str(), "model" | "skill" | "mcp") {
            state
                .db
                .add_api_key_scope(&ak.key, &scope, "*", "invoke")
                .await
                .map_err(db_err)?;
        }
    }
    state.auth.reload().await;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=create_api_key target={} user={}",
        session.user_id,
        ak.key,
        user_id
    );

    Ok(Json(serde_json::json!({
        "key": ak.key,
        "user_id": ak.user_id,
        "name": ak.name,
        "enabled": ak.enabled,
    })))
}

pub(crate) async fn update_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, key_val)): Path<(String, String)>,
    Json(req): Json<CreateKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    let keys = state.db.list_api_keys(&user_id).await.map_err(db_err)?;
    let existing = keys
        .iter()
        .find(|k| k.key == key_val)
        .ok_or_else(|| AdminError::not_found("Key not found"))?;
    let billing_group = if let Some(group_id) = req.billing_group_id.as_deref() {
        state
            .db
            .get_billing_group(group_id)
            .await
            .map_err(db_err)?
            .filter(|group| group.is_active())
            .ok_or_else(|| AdminError::bad_request("Billing group is not active"))?
    } else {
        state
            .db
            .get_billing_group(&existing.billing_group_id)
            .await
            .map_err(db_err)?
            .ok_or_else(|| AdminError::bad_request("Billing group not found"))?
    };

    let ak = ApiKey {
        key: key_val.clone(),
        user_id: user_id.clone(),
        name: req.name.unwrap_or(existing.name.clone()),
        enabled: req.enabled.unwrap_or(existing.enabled),
        expires_at: req.expires_at.or(existing.expires_at.clone()),
        spend_limit: req.spend_limit.or(existing.spend_limit),
        allowed_models: req.allowed_models.or(existing.allowed_models.clone()),
        team_id: req.team_id.or(existing.team_id.clone()),
        scopes: None,
        billing_group_id: billing_group.id,
        billing_payment_mode: billing_group.payment_mode,
    };

    state.db.update_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=update_api_key target={} user={}",
        session.user_id,
        key_val,
        user_id
    );

    Ok(Json(serde_json::json!({ "key": key_val, "updated": true })))
}

pub(crate) async fn delete_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((_user_id, key_val)): Path<(String, String)>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    state.db.delete_api_key(&key_val).await.map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=delete_api_key target={}",
        session.user_id,
        key_val
    );

    Ok(Json(serde_json::json!({ "deleted": key_val })))
}

// ── Toggle User Key (admin) ───────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct ToggleKeyReq {
    enabled: bool,
}

pub(crate) async fn toggle_user_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, key_val)): Path<(String, String)>,
    Json(req): Json<ToggleKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:users").await?;

    let keys = state.db.list_api_keys(&user_id).await.map_err(db_err)?;
    let existing = keys
        .iter()
        .find(|k| k.key == key_val)
        .ok_or_else(|| AdminError::not_found("Key not found"))?;
    let mut ak = existing.clone();
    ak.enabled = req.enabled;
    state.db.update_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    Ok(Json(
        serde_json::json!({ "key": key_val, "enabled": req.enabled }),
    ))
}
