use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::db::DbError;
use crate::domain::billing_group::{BillingGroupRow, BillingPaymentMode};
use crate::server::AppState;

use super::*;

#[derive(Debug, Deserialize)]
pub(crate) struct CreateBillingGroupReq {
    name: String,
    payment_mode: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SetBillingGroupStatusReq {
    status: String,
}

fn parse_payment_mode(value: &str) -> Result<BillingPaymentMode, AdminError> {
    value
        .parse()
        .map_err(|_| AdminError::bad_request("payment_mode must be prepaid or postpaid"))
}

pub(crate) async fn list_billing_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<BillingGroupRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:billing-groups").await?;
    state
        .db
        .list_billing_groups(false)
        .await
        .map(Json)
        .map_err(db_err)
}

pub(crate) async fn list_active_billing_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<BillingGroupRow>>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    state
        .db
        .list_billing_groups(true)
        .await
        .map(Json)
        .map_err(db_err)
}

pub(crate) async fn create_billing_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateBillingGroupReq>,
) -> Result<Json<BillingGroupRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:billing-groups").await?;
    let name = req.name.trim();
    if name.is_empty() {
        return Err(AdminError::bad_request("Billing group name is required"));
    }
    let mode = parse_payment_mode(&req.payment_mode)?;
    let id = uuid::Uuid::new_v4().to_string();
    state
        .db
        .create_billing_group(&id, name, mode, &session.user_id)
        .await
        .map(Json)
        .map_err(db_err)
}

pub(crate) async fn set_billing_group_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<SetBillingGroupStatusReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:billing-groups").await?;
    if !matches!(req.status.as_str(), "active" | "inactive") {
        return Err(AdminError::bad_request("status must be active or inactive"));
    }
    state
        .db
        .set_billing_group_status(&id, &req.status)
        .await
        .map_err(|e: DbError| {
            if e.0.contains("protected") {
                AdminError::bad_request(e.0)
            } else {
                db_err(e)
            }
        })?;
    Ok(Json(serde_json::json!({ "ok": true, "status": req.status })))
}
