use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::cache::compute_gate_status;
use crate::server::AppState;

use super::*;

// ── Wallet ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct WalletOverview {
    balance: f64,
    frozen: f64,
    total_consumed: f64,
    total_recharged: f64,
}

pub(crate) async fn wallet_overview(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<WalletOverview>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let user_id = &session.user_id;
    let (balance, frozen) = state.db.get_wallet_balance(user_id).await.map_err(db_err)?;
    let total_consumed = state.db.get_total_consumed(user_id).await.map_err(db_err)?;
    let total_recharged = state.db.get_total_recharged(user_id).await.map_err(db_err)?;
    Ok(Json(WalletOverview { balance, frozen, total_consumed, total_recharged }))
}

#[derive(Deserialize)]
pub(crate) struct RechargeReq {
    amount: f64,
}

#[derive(Serialize)]
pub(crate) struct RechargeResp {
    transaction_id: String,
    amount: f64,
    balance: f64,
}

#[derive(Deserialize)]
pub(crate) struct WalletCreateKeyReq {
    amount: f64,
    expires_at: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreateKeyResp {
    key: String,
    amount: f64,
    expires_at: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct RedeemKeyReq {
    key: String,
}

#[derive(Serialize)]
pub(crate) struct RedeemKeyResp {
    amount: f64,
    balance: f64,
}

pub(crate) async fn wallet_recharge(
    State(_state): State<Arc<AppState>>,
    _headers: HeaderMap,
    _req: Json<RechargeReq>,
) -> Result<Json<RechargeResp>, AdminError> {
    return Err(AdminError::bad_request("Recharge is under development"));
}

pub(crate) async fn wallet_create_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<WalletCreateKeyReq>,
) -> Result<Json<CreateKeyResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:recharge-keys").await?;
    if req.amount <= 0.0 {
        return Err(AdminError::bad_request("Amount must be positive"));
    }
    let key = uuid::Uuid::new_v4().to_string();
    state.db.create_recharge_key(&key, req.amount, &session.user_id, req.expires_at.as_deref()).await.map_err(db_err)?;
    Ok(Json(CreateKeyResp { key, amount: req.amount, expires_at: req.expires_at }))
}

pub(crate) async fn wallet_redeem_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<RedeemKeyReq>,
) -> Result<Json<RedeemKeyResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let amount = state.db.redeem_recharge_key(&req.key, &session.user_id).await.map_err(db_err_bad_request)?;
    let (balance, frozen) = state.db.get_wallet_balance(&session.user_id).await.map_err(db_err)?;

    // Sync to Redis gate cache
    let status = compute_gate_status(balance, frozen);
    if let Err(e) = state.cache.set_gate_and_balance(&session.user_id, status, balance).await {
        tracing::warn!(user_id = &session.user_id, "Failed to sync redeem to Redis: {}", e);
    }

    Ok(Json(RedeemKeyResp { amount, balance }))
}

#[derive(Deserialize)]
pub(crate) struct KeyListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    search: Option<String>,
    status: Option<String>,
    used_by: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct RevokeKeyReq {
    key: String,
}

const DEFAULT_KEY_PAGE_SIZE: usize = 20;

pub(crate) async fn wallet_list_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<KeyListQuery>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    if !state.authz.enforce(&session.role, "admin:recharge-keys").await {
        return Ok(Json(serde_json::json!({ "items": [], "total": 0 })));
    }
    let limit = q.limit.unwrap_or(DEFAULT_KEY_PAGE_SIZE);
    let offset = q.offset.unwrap_or(0);
    let total = state.db.count_recharge_keys_filtered(
        q.search.as_deref(),
        q.status.as_deref(),
        q.used_by.as_deref(),
    ).await.map_err(db_err)?;
    let items = state.db.list_recharge_keys_filtered(
        limit, offset,
        q.search.as_deref(),
        q.status.as_deref(),
        q.used_by.as_deref(),
    ).await.map_err(db_err)?;
    Ok(Json(serde_json::json!({ "items": items, "total": total })))
}

pub(crate) async fn wallet_revoke_key(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<RevokeKeyReq>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:recharge-keys").await?;
    state.db.revoke_recharge_key(&req.key).await.map_err(db_err_bad_request)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
pub(crate) struct WalletTxQuery {
    page: Option<usize>,
    size: Option<usize>,
    since: Option<String>,
    until: Option<String>,
    tx_type: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct WalletTxResp {
    items: Vec<WalletTxItem>,
    total_dates: usize,
}

#[derive(Serialize)]
pub(crate) struct WalletTxItem {
    id: String,
    tx_type: String,
    amount: f64,
    balance_before: f64,
    balance_after: f64,
    method: String,
    status: String,
    note: String,
    created_at: String,
}

pub(crate) async fn wallet_transactions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<WalletTxQuery>,
) -> Result<Json<WalletTxResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let page = q.page.unwrap_or(1);
    let size = q.size.unwrap_or(15).min(31);
    let can_view_all = state.authz.enforce(&session.role, "admin:bills").await;
    let uid_filter: Option<&str> = if can_view_all { None } else { Some(&session.user_id) };
    let (rows, total_dates) = state.db.list_wallet_tx_by_dates(
        uid_filter, page, size, q.since.as_deref(), q.until.as_deref(), q.tx_type.as_deref(),
    ).await.map_err(db_err)?;
    let items = rows.into_iter().map(|r| WalletTxItem {
        id: r.id,
        tx_type: r.tx_type,
        amount: r.amount,
        balance_before: r.balance_before,
        balance_after: r.balance_after,
        method: r.method,
        status: r.status,
        note: r.note,
        created_at: r.created_at,
    }).collect();
    Ok(Json(WalletTxResp { items, total_dates }))
}

#[derive(Serialize)]
pub(crate) struct EstimatedDaysResp {
    days: Option<f64>,
}

pub(crate) async fn wallet_estimated_days(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<EstimatedDaysResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let days = state.db.get_wallet_estimated_days(&session.user_id).await.map_err(db_err)?;
    Ok(Json(EstimatedDaysResp { days }))
}
