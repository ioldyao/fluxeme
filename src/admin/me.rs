use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono_tz::Tz;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::routing::RoutingRule;
use crate::domain::user::ApiKey;
use crate::server::AppState;

use super::*;

// ── Current User ("Me") ───────────────────────────────────────────

#[derive(serde::Serialize)]
pub(crate) struct MySessionResponse {
    user_id: String,
    user_name: String,
    role: String,
    status: String,
    timezone: String,
    currency: String,
}

pub(crate) async fn get_my_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<MySessionResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let user = state
        .db
        .get_user(&session.user_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("User not found"))?;

    Ok(Json(MySessionResponse {
        user_id: user.id,
        user_name: user.name,
        role: user.role,
        status: user.status,
        timezone: user.timezone,
        currency: user.currency,
    }))
}

#[derive(Deserialize)]
pub(crate) struct ChangePasswordReq {
    current_password: String,
    new_password: String,
}

pub(crate) async fn change_my_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    validate_password(&req.new_password)?;

    // Verify current password
    let user = state
        .db
        .get_user_with_password(&session.user_id)
        .await
        .map_err(db_err)?;

    if let Some(u) = user {
        if let Some(ref hash) = u.password_hash {
            if !hash.is_empty() {
                match bcrypt::verify(&req.current_password, hash) {
                    Ok(true) => { /* correct password - continue */ }
                    Ok(false) => {
                        return Err(AdminError::bad_request("Current password is incorrect"));
                    }
                    Err(e) => {
                        tracing::error!("bcrypt verify error for user {}: {}", session.user_id, e);
                        return Err(AdminError::internal("Authentication error"));
                    }
                }
            } else {
                return Err(AdminError::bad_request(
                    "Cannot change password for this account",
                ));
            }
        } else {
            return Err(AdminError::bad_request(
                "Cannot change password for this account",
            ));
        }
    } else {
        return Err(AdminError::not_found("User not found"));
    }

    let new_hash =
        bcrypt::hash(&req.new_password, 10).map_err(|e| AdminError::internal(e.to_string()))?;

    state
        .db
        .update_user_admin_fields(&session.user_id, None, Some(new_hash), None, None, None)
        .await
        .map_err(db_err)?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct UpdateTimezoneReq {
    timezone: String,
}

pub(crate) async fn get_my_timezone(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    Ok(Json(serde_json::json!({ "timezone": tz })))
}

pub(crate) async fn update_my_timezone(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UpdateTimezoneReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    // Validate IANA timezone name
    if req.timezone.parse::<Tz>().is_err() {
        return Err(AdminError::bad_request("Invalid timezone"));
    }

    state
        .db
        .update_user_timezone(&session.user_id, &req.timezone)
        .await
        .map_err(db_err)?;

    Ok(Json(
        serde_json::json!({ "ok": true, "timezone": req.timezone }),
    ))
}

#[derive(Deserialize)]
pub(crate) struct UpdateCurrencyReq {
    currency: String,
}

pub(crate) async fn get_my_currency(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let cur = state
        .db
        .get_user_currency(&session.user_id)
        .await
        .map_err(db_err)?;
    Ok(Json(serde_json::json!({ "currency": cur })))
}

pub(crate) async fn update_my_currency(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UpdateCurrencyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    state
        .db
        .update_user_currency(&session.user_id, &req.currency)
        .await
        .map_err(db_err)?;
    Ok(Json(
        serde_json::json!({ "ok": true, "currency": req.currency }),
    ))
}

pub(crate) async fn my_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ApiKey>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let keys = state
        .db
        .list_api_keys(&session.user_id)
        .await
        .map_err(db_err)?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
pub(crate) struct CreateMyKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
    expires_at: Option<String>,
    #[serde(default, with = "rust_decimal::serde::float_option")]
    spend_limit: Option<Decimal>,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
    /// 访问范围 = 资源类型（model / skill / mcp）。缺省 model+skill。
    #[serde(default)]
    scopes: Option<Vec<String>>,
}

pub(crate) async fn create_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateMyKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let key_value = format!("sk-{}", uuid::Uuid::new_v4());
    let ak = ApiKey {
        key: key_value.clone(),
        user_id: session.user_id.clone(),
        name: req.name.unwrap_or_default(),
        enabled: req.enabled.unwrap_or(true),
        expires_at: req.expires_at,
        spend_limit: req.spend_limit,
        allowed_models: req.allowed_models,
        team_id: None,
        scopes: None,
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

    Ok(Json(serde_json::json!({
        "key": ak.key,
        "user_id": ak.user_id,
        "name": ak.name,
        "enabled": ak.enabled,
    })))
}

#[derive(Deserialize)]
pub(crate) struct UpdateMyKeyReq {
    name: Option<String>,
    enabled: Option<bool>,
    expires_at: Option<String>,
    #[serde(default, with = "rust_decimal::serde::float_option")]
    spend_limit: Option<Decimal>,
    #[serde(default)]
    allowed_models: Option<Vec<String>>,
}

pub(crate) async fn update_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_val): Path<String>,
    Json(req): Json<UpdateMyKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let keys = state
        .db
        .list_api_keys(&session.user_id)
        .await
        .map_err(db_err)?;
    let existing = keys
        .iter()
        .find(|k| k.key == key_val)
        .ok_or_else(|| AdminError::not_found("Key not found"))?;

    let ak = ApiKey {
        key: key_val.clone(),
        user_id: session.user_id.clone(),
        name: req.name.unwrap_or(existing.name.clone()),
        enabled: req.enabled.unwrap_or(existing.enabled),
        expires_at: req.expires_at.or(existing.expires_at.clone()),
        spend_limit: req.spend_limit.or(existing.spend_limit),
        allowed_models: req.allowed_models.or(existing.allowed_models.clone()),
        team_id: existing.team_id.clone(),
        scopes: None,
    };

    state.db.update_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    Ok(Json(serde_json::json!({ "key": key_val, "updated": true })))
}

pub(crate) async fn delete_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_val): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    // Verify the key belongs to the current user
    let keys = state
        .db
        .list_api_keys(&session.user_id)
        .await
        .map_err(db_err)?;
    if !keys.iter().any(|k| k.key == key_val) {
        return Err(AdminError::not_found("Key not found"));
    }

    state.db.delete_api_key(&key_val).await.map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    Ok(Json(serde_json::json!({ "deleted": key_val })))
}

#[derive(Deserialize)]
pub(crate) struct ToggleKeyReq {
    enabled: bool,
}

pub(crate) async fn toggle_my_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(key_val): Path<String>,
    Json(req): Json<ToggleKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let keys = state
        .db
        .list_api_keys(&session.user_id)
        .await
        .map_err(db_err)?;
    if !keys.iter().any(|k| k.key == key_val) {
        return Err(AdminError::not_found("Key not found"));
    }

    let ak = ApiKey {
        key: key_val.clone(),
        user_id: session.user_id.clone(),
        name: String::new(),
        enabled: req.enabled,
        expires_at: None,
        spend_limit: None,
        allowed_models: None,
        team_id: None,
        scopes: None,
    };
    state.db.update_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    Ok(Json(
        serde_json::json!({ "key": key_val, "enabled": req.enabled }),
    ))
}

/// List all granted permissions for the current session.
pub(crate) async fn my_permissions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let all_known = [
        "admin:dashboard",
        "admin:users",
        "admin:channels",
        "admin:models",
        "admin:model-pricing",
        "admin:rules",
        "admin:moderation",
        "admin:usage",
        "admin:bills",
        "admin:recharge-keys",
        "admin:health",
        "admin:settings",
        "admin:gateway",
        "admin:policies",
        "admin:announcements",
        "admin:teams",
        "admin:skillhub",
    ];
    let mut granted = Vec::new();
    for perm in &all_known {
        if state.authz.enforce(&session.role, perm).await {
            granted.push(perm.to_string());
        }
    }
    Ok(Json(granted))
}

// ── My Teams (self-service) ───────────────────────────────────────

pub(crate) async fn my_teams(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::domain::team::Team>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let teams = state
        .db
        .list_teams_for_user(&session.user_id)
        .await
        .map_err(db_err)?;
    Ok(Json(teams))
}

pub(crate) async fn my_team_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<crate::domain::team::Team>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    // Must be a member to view.
    if state
        .db
        .get_team_member(&team_id, &session.user_id)
        .await
        .map_err(db_err)?
        .is_none()
    {
        return Err(AdminError::not_found("Team not found"));
    }
    let team = state
        .db
        .get_team(&team_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Team not found"))?;
    Ok(Json(team))
}

pub(crate) async fn my_team_members(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Vec<crate::domain::team::TeamMember>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    if state
        .db
        .get_team_member(&team_id, &session.user_id)
        .await
        .map_err(db_err)?
        .is_none()
    {
        return Err(AdminError::not_found("Team not found"));
    }
    let members = state
        .db
        .list_team_members(&team_id)
        .await
        .map_err(db_err)?;
    Ok(Json(members))
}

pub(crate) async fn my_team_wallet(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    if state
        .db
        .get_team_member(&team_id, &session.user_id)
        .await
        .map_err(db_err)?
        .is_none()
    {
        return Err(AdminError::not_found("Team not found"));
    }
    let (balance, frozen) = state
        .db
        .get_team_wallet(&team_id)
        .await
        .map_err(db_err)?
        .unwrap_or((0.0, 0.0));
    Ok(Json(serde_json::json!({
        "team_id": team_id,
        "balance": balance,
        "frozen": frozen,
    })))
}

// ── Team resources (self-service) ─────────────────────────────────

/// Ensure the session user is a member of the team. Returns the member row.
async fn require_team_member(
    state: &AppState,
    session: &SessionInfo,
    team_id: &str,
) -> Result<crate::domain::team::TeamMember, AdminError> {
    state
        .db
        .get_team_member(team_id, &session.user_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Team not found"))
}

pub(crate) async fn my_team_api_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Vec<ApiKey>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_member(&state, &session, &team_id).await?;
    let keys = state
        .db
        .list_team_api_keys(&team_id)
        .await
        .map_err(db_err)?;
    Ok(Json(keys))
}

pub(crate) async fn create_my_team_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(req): Json<CreateMyKeyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_member(&state, &session, &team_id).await?;
    // Only owner/admin can create team keys.
    if !state
        .team_authz
        .enforce(&team_id, &session.user_id, "team:key:manage")
        .await
    {
        return Err(AdminError::forbidden("Insufficient team permissions"));
    }
    let key_value = format!("sk-{}", uuid::Uuid::new_v4());
    let ak = ApiKey {
        key: key_value.clone(),
        user_id: session.user_id.clone(),
        name: req.name.unwrap_or_default(),
        enabled: req.enabled.unwrap_or(true),
        expires_at: req.expires_at,
        spend_limit: req.spend_limit,
        allowed_models: req.allowed_models,
        team_id: Some(team_id.clone()),
        scopes: None,
    };
    state.db.create_api_key(&ak).await.map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;
    Ok(Json(serde_json::json!({
        "key": ak.key,
        "team_id": team_id,
        "name": ak.name,
        "enabled": ak.enabled,
    })))
}

pub(crate) async fn my_team_wallet_transactions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_member(&state, &session, &team_id).await?;
    let (items, total) = state
        .db
        .list_team_wallet_transactions(&team_id, 1, 50)
        .await
        .map_err(db_err)?;
    let items: Vec<serde_json::Value> = items
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "user_id": t.user_id,
                "tx_type": t.tx_type,
                "amount": t.amount,
                "balance_before": t.balance_before,
                "balance_after": t.balance_after,
                "method": t.method,
                "status": t.status,
                "note": t.note,
                "created_at": t.created_at,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "items": items, "total": total })))
}

pub(crate) async fn my_team_rules(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Vec<RoutingRule>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_member(&state, &session, &team_id).await?;
    let rules = state
        .db
        .list_team_rules(&team_id)
        .await
        .map_err(db_err)?;
    Ok(Json(rules))
}

pub(crate) async fn create_my_team_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(mut rule): Json<RoutingRule>,
) -> Result<Json<RoutingRule>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_member(&state, &session, &team_id).await?;
    if !state
        .team_authz
        .enforce(&team_id, &session.user_id, "team:rule:manage")
        .await
    {
        return Err(AdminError::forbidden("Insufficient team permissions"));
    }
    if rule.source_model.is_empty() || rule.target_model.is_empty() {
        return Err(AdminError::bad_request("source_model and target_model are required"));
    }
    rule.id = uuid::Uuid::new_v4().to_string();
    rule.scope = "user".to_string();
    rule.team_id = Some(team_id.clone());
    rule.user_id = session.user_id.clone();
    rule.channel_id.clear();
    rule.upstream_model.clear();
    let now = chrono::Utc::now().to_rfc3339();
    rule.created_at = now.clone();
    rule.updated_at = now;
    state.db.create_rule(&rule).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;
    Ok(Json(rule))
}

pub(crate) async fn delete_my_team_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((team_id, rule_id)): Path<(String, String)>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_member(&state, &session, &team_id).await?;
    let rules = state
        .db
        .list_team_rules(&team_id)
        .await
        .map_err(db_err)?;
    if !rules.iter().any(|r| r.id == rule_id) {
        return Err(AdminError::not_found("Rule not found"));
    }
    state.db.delete_rule(&rule_id).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;
    Ok(Json(serde_json::json!({ "deleted": rule_id })))
}

// ── Team management (self-service, for team owner/admin) ──────────

/// Require the session user to have `perm` within the team.
async fn require_team_perm(
    state: &AppState,
    session: &SessionInfo,
    team_id: &str,
    perm: &str,
) -> Result<(), AdminError> {
    require_team_member(state, session, team_id).await?;
    if !state.team_authz.enforce(team_id, &session.user_id, perm).await {
        return Err(AdminError::forbidden("Insufficient team permissions"));
    }
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct AddMyTeamMemberReq {
    user_id: String,
    role: Option<String>,
}

pub(crate) async fn add_my_team_member(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(req): Json<AddMyTeamMemberReq>,
) -> Result<Json<crate::domain::team::TeamMember>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_perm(&state, &session, &team_id, "team:member:manage").await?;
    let role = match req.role.as_deref() {
        Some("admin") => "admin",
        Some("member") | None => "member",
        _ => return Err(AdminError::bad_request("Invalid member role")),
    };
    state
        .db
        .add_team_member(&team_id, &req.user_id, role)
        .await
        .map_err(db_err)?;
    let members = state
        .db
        .list_team_members(&team_id)
        .await
        .map_err(db_err)?;
    state.team_authz.sync_team_roles(&team_id, &members).await;
    let member = state
        .db
        .get_team_member(&team_id, &req.user_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Member not found"))?;
    Ok(Json(member))
}

pub(crate) async fn remove_my_team_member(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((team_id, user_id)): Path<(String, String)>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_perm(&state, &session, &team_id, "team:member:manage").await?;
    state
        .db
        .remove_team_member(&team_id, &user_id)
        .await
        .map_err(db_err)?;
    let members = state
        .db
        .list_team_members(&team_id)
        .await
        .map_err(db_err)?;
    state.team_authz.sync_team_roles(&team_id, &members).await;
    Ok(Json(serde_json::json!({ "removed": true })))
}

#[derive(Deserialize)]
pub(crate) struct SetMyTeamRoleReq {
    role: String,
}

pub(crate) async fn set_my_team_member_role(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((team_id, user_id)): Path<(String, String)>,
    Json(req): Json<SetMyTeamRoleReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_perm(&state, &session, &team_id, "team:member:manage").await?;
    let role = match req.role.as_str() {
        "admin" => "admin",
        "member" => "member",
        _ => return Err(AdminError::bad_request("Invalid member role")),
    };
    state
        .db
        .set_team_member_role(&team_id, &user_id, role)
        .await
        .map_err(db_err)?;
    let members = state
        .db
        .list_team_members(&team_id)
        .await
        .map_err(db_err)?;
    state.team_authz.sync_team_roles(&team_id, &members).await;
    Ok(Json(serde_json::json!({ "updated": true })))
}

#[derive(Deserialize)]
pub(crate) struct CreditMyTeamWalletReq {
    amount: f64,
}

pub(crate) async fn credit_my_team_wallet(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(req): Json<CreditMyTeamWalletReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_perm(&state, &session, &team_id, "team:wallet:manage").await?;
    if req.amount <= 0.0 {
        return Err(AdminError::bad_request("Amount must be positive"));
    }
    state
        .db
        .add_team_wallet_balance(&team_id, req.amount)
        .await
        .map_err(db_err)?;
    Ok(Json(serde_json::json!({ "credited": req.amount })))
}

pub(crate) async fn delete_my_team_api_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((team_id, key_val)): Path<(String, String)>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    require_team_perm(&state, &session, &team_id, "team:key:manage").await?;
    let keys = state
        .db
        .list_team_api_keys(&team_id)
        .await
        .map_err(db_err)?;
    if !keys.iter().any(|k| k.key == key_val) {
        return Err(AdminError::not_found("Key not found"));
    }
    state.db.delete_api_key(&key_val).await.map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;
    Ok(Json(serde_json::json!({ "deleted": key_val })))
}

// ── User-level routing rules (self-service) ────────────────────────

pub(crate) async fn list_my_rules(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<RoutingRule>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let rules = state
        .db
        .list_user_rules(&session.user_id)
        .await
        .map_err(db_err)?;
    Ok(Json(rules))
}

pub(crate) async fn create_my_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut rule): Json<RoutingRule>,
) -> Result<Json<RoutingRule>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    if rule.source_model.is_empty() {
        return Err(AdminError::bad_request("source_model is required"));
    }
    if rule.target_model.is_empty() {
        return Err(AdminError::bad_request("target_model is required"));
    }
    if rule.id.is_empty() {
        rule.id = uuid::Uuid::new_v4().to_string();
    }
    if rule.name.is_empty() {
        rule.name = format!("{}→{}", rule.source_model, rule.target_model);
    }
    rule.scope = "user".to_string();
    rule.user_id = session.user_id.clone();
    rule.channel_id.clear();
    rule.upstream_model.clear();
    let now = chrono::Utc::now().to_rfc3339();
    rule.created_at = now.clone();
    rule.updated_at = now;
    state.db.create_rule(&rule).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;
    Ok(Json(rule))
}

pub(crate) async fn delete_my_rule(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    // Only allow deleting own rules
    let rules = state
        .db
        .list_user_rules(&session.user_id)
        .await
        .map_err(db_err)?;
    if !rules.iter().any(|r| r.id == id) {
        return Err(AdminError::not_found("Rule not found"));
    }
    state.db.delete_rule(&id).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;
    Ok(Json(serde_json::json!({ "deleted": id })))
}
