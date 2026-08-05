use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::team::{Team, TeamMember};
use crate::server::AppState;

use super::*;

// ── Admin Team Management ─────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct TeamWithRole {
    #[serde(flatten)]
    team: Team,
    /// Current session's role in this team ("owner"/"admin"/"member").
    role: String,
}

pub(crate) async fn list_all_teams(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<TeamWithRole>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
    let teams = state.db.list_all_teams().await.map_err(db_err)?;
    let mut out = Vec::with_capacity(teams.len());
    for team in teams {
        let role = state
            .db
            .get_team_member(&team.id, &session.user_id)
            .await
            .ok()
            .flatten()
            .map(|m| m.role)
            .unwrap_or_default();
        out.push(TeamWithRole { team, role });
    }
    Ok(Json(out))
}

pub(crate) async fn get_team_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Team>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
    let team = state
        .db
        .get_team(&team_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Team not found"))?;
    Ok(Json(team))
}

#[derive(Deserialize)]
pub(crate) struct CreateTeamReq {
    name: String,
    /// The team's actual manager (owner). The gateway admin creating the team
    /// is NOT automatically a member/owner — the owner is a separate role.
    owner_id: String,
}

pub(crate) async fn create_team(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateTeamReq>,
) -> Result<Json<Team>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
    if req.name.trim().is_empty() {
        return Err(AdminError::bad_request("Team name is required"));
    }
    let owner_id = req.owner_id.trim();
    if owner_id.is_empty() {
        return Err(AdminError::bad_request("owner_id is required"));
    }
    // The owner must be an existing user (they will be added as a member).
    if state.db.get_user(owner_id).await.map_err(db_err)?.is_none() {
        return Err(AdminError::bad_request("Owner user not found"));
    }
    let now = Utc::now();
    let team = Team {
        id: uuid::Uuid::new_v4().to_string(),
        name: req.name.trim().to_string(),
        owner_id: owner_id.to_string(),
        created_at: now,
        updated_at: now,
    };
    state
        .db
        .create_team(&team, owner_id)
        .await
        .map_err(db_err)?;
    // Sync team roles into the team Casbin enforcer.
    let members = state
        .db
        .list_team_members(&team.id)
        .await
        .map_err(db_err)?;
    state.team_authz.sync_team_roles(&team.id, &members).await;
    tracing::info!("admin={} action=create_team team={}", session.user_id, team.id);
    Ok(Json(team))
}

#[derive(Deserialize)]
pub(crate) struct UpdateTeamReq {
    name: String,
}

pub(crate) async fn update_team(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(req): Json<UpdateTeamReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
    if req.name.trim().is_empty() {
        return Err(AdminError::bad_request("Team name is required"));
    }
    state
        .db
        .update_team(&team_id, req.name.trim())
        .await
        .map_err(db_err)?;
    Ok(Json(serde_json::json!({ "updated": team_id })))
}

pub(crate) async fn delete_team(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
    state
        .db
        .delete_team(&team_id)
        .await
        .map_err(db_err)?;
    // Clear the team's role bindings from the team enforcer.
    state.team_authz.sync_team_roles(&team_id, &[]).await;
    Ok(Json(serde_json::json!({ "deleted": team_id })))
}

// ── Team Members (admin) ──────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct AddMemberReq {
    user_id: String,
    role: Option<String>,
}

pub(crate) async fn add_team_member(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(req): Json<AddMemberReq>,
) -> Result<Json<TeamMember>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
    let role = match req.role.as_deref() {
        Some("admin") => "admin",
        Some("member") | None => "member",
        Some("owner") | Some(_) => {
            return Err(AdminError::bad_request("Invalid member role"));
        }
    };
    state
        .db
        .add_team_member(&team_id, &req.user_id, role)
        .await
        .map_err(db_err)?;
    // Resync roles after membership change.
    let members = state
        .db
        .list_team_members(&team_id)
        .await
        .map_err(db_err)?;
    state.team_authz.sync_team_roles(&team_id, &members).await;
    tracing::info!(
        "admin={} action=add_team_member team={} user={} role={}",
        session.user_id,
        team_id,
        req.user_id,
        role
    );
    let member = state
        .db
        .get_team_member(&team_id, &req.user_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Member not found"))?;
    Ok(Json(member))
}

pub(crate) async fn list_team_members(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Vec<TeamMember>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
    let members = state
        .db
        .list_team_members(&team_id)
        .await
        .map_err(db_err)?;
    Ok(Json(members))
}

#[derive(Deserialize)]
pub(crate) struct SetRoleReq {
    role: String,
}

pub(crate) async fn set_team_member_role(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((team_id, user_id)): Path<(String, String)>,
    Json(req): Json<SetRoleReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
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

pub(crate) async fn remove_team_member(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((team_id, user_id)): Path<(String, String)>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
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


// ── Team Wallet (admin) ───────────────────────────────────────────

pub(crate) async fn get_team_wallet_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
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

#[derive(Deserialize)]
pub(crate) struct CreditTeamWalletReq {
    amount: f64,
}

pub(crate) async fn credit_team_wallet(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Json(req): Json<CreditTeamWalletReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
    if req.amount <= 0.0 {
        return Err(AdminError::bad_request("Amount must be positive"));
    }
    state
        .db
        .add_team_wallet_balance(&team_id, req.amount)
        .await
        .map_err(db_err)?;
    tracing::info!(
        "admin={} action=credit_team_wallet team={} amount={}",
        session.user_id,
        team_id,
        req.amount
    );
    Ok(Json(serde_json::json!({ "credited": req.amount })))
}

pub(crate) async fn list_team_wallet_transactions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Query(query): Query<ListTxQuery>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:teams").await?;
    let page = query.page.unwrap_or(1);
    let size = query.size.unwrap_or(20);
    let (items, total) = state
        .db
        .list_team_wallet_transactions(&team_id, page, size)
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

#[derive(Deserialize)]
pub(crate) struct ListTxQuery {
    page: Option<usize>,
    size: Option<usize>,
}
