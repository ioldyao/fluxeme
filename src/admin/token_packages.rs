use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::domain::token_package::TokenPackageGrantRow;
use crate::server::AppState;

use super::*;

#[derive(Debug, Deserialize)]
pub(crate) struct PlanRequest {
    pub code: String,
    pub name: String,
    pub accounting_mode: String,
    pub display_token_amount: i64,
    pub total_units: Option<i64>,
    #[serde(default = "default_factor")]
    pub input_credit_factor: f64,
    #[serde(default = "default_factor")]
    pub output_credit_factor: f64,
    #[serde(default)]
    pub cache_credit_factor: f64,
    #[serde(default = "default_policy")]
    pub exhaustion_policy: String,
    #[serde(default)]
    pub priority: i32,
    pub validity_days: Option<i32>,
}

fn default_factor() -> f64 { 1.0 }
fn default_policy() -> String { "package_then_wallet".to_string() }

#[derive(Debug, Deserialize)]
pub(crate) struct GrantRequest {
    pub plan_id: String,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct GrantResponse {
    pub grant: TokenPackageGrantRow,
}

pub(crate) async fn create_plan(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PlanRequest>,
) -> Result<Json<crate::domain::token_package::TokenPackagePlanRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:billing").await?;
    let total_units = req.total_units.unwrap_or(req.display_token_amount);
    let plan = state.db.create_token_package_plan(
        &uuid::Uuid::new_v4().to_string(), &req.code, &req.name, &req.accounting_mode,
        req.display_token_amount, total_units,
        rust_decimal::Decimal::try_from(req.input_credit_factor).unwrap_or(rust_decimal::Decimal::ONE),
        rust_decimal::Decimal::try_from(req.output_credit_factor).unwrap_or(rust_decimal::Decimal::ONE),
        rust_decimal::Decimal::try_from(req.cache_credit_factor).unwrap_or(rust_decimal::Decimal::ZERO),
        &req.exhaustion_policy, req.priority, req.validity_days, &session.user_id,
    ).await.map_err(db_err)?;
    Ok(Json(plan))
}

pub(crate) async fn list_plans(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::domain::token_package::TokenPackagePlanRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:billing").await?;
    Ok(Json(state.db.list_token_package_plans().await.map_err(db_err)?))
}

pub(crate) async fn list_grants(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<TokenPackageGrantRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:billing").await?;
    let grants = state.db.list_token_package_grants(None, None).await.map_err(db_err)?;
    Ok(Json(grants))
}

pub(crate) async fn create_grant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<GrantRequest>,
) -> Result<Json<GrantResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:billing").await?;
    if req.user_id.is_none() == req.team_id.is_none() {
        return Err(AdminError::bad_request("Exactly one of user_id or team_id is required"));
    }
    if let Some(expires_at) = req.expires_at.as_deref() {
        chrono::DateTime::parse_from_rfc3339(expires_at)
            .map_err(|_| AdminError::bad_request("expires_at must be RFC3339"))?;
    }
    let grant = state
        .db
        .create_token_package_grant(
            &uuid::Uuid::new_v4().to_string(),
            &req.plan_id,
            req.user_id.as_deref(),
            req.team_id.as_deref(),
            "admin_grant",
            &req.note,
            req.expires_at.as_deref(),
        )
        .await
        .map_err(db_err)?;
    Ok(Json(GrantResponse { grant }))
}

pub(crate) async fn list_my_grants(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<TokenPackageGrantRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let grants = state
        .db
        .list_token_package_grants(Some(&session.user_id), None)
        .await
        .map_err(db_err)?;
    Ok(Json(grants))
}

pub(crate) async fn get_grant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<TokenPackageGrantRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:billing").await?;
    let grant = state
        .db
        .list_token_package_grants(None, None)
        .await
        .map_err(db_err)?
        .into_iter()
        .find(|grant| grant.id == id)
        .ok_or_else(|| AdminError::not_found("Token package grant not found"))?;
    Ok(Json(grant))
}
