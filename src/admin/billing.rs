use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Datelike;
use serde::{Deserialize, Serialize};

use crate::server::AppState;

use super::*;

#[derive(Serialize)]
pub(crate) struct BillingSummary {
    total_requests: u64,
    total_cost: f64,
    balance: f64,
}

pub(crate) async fn billing_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<BillingSummary>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let can_view_all = state.authz.enforce(&session.role, "admin:bills").await;
    let user_filter: Option<&str> = if can_view_all {
        None
    } else {
        Some(&session.user_id)
    };
    let records = state
        .usage
        .cost_rows_since("1970-01-01T00:00:00", user_filter)
        .await.map_err(AdminError::internal)?;
    let total_cost: f64 = records
        .iter()
        .map(|r| {
            let pp = if r.prompt_price > 0.0 { r.prompt_price } else { 0.0 };
            let cp = if r.completion_price > 0.0 { r.completion_price } else { 0.0 };
            (r.prompt_tokens as f64 / 1000000.0 * pp)
                + (r.completion_tokens as f64 / 1000000.0 * cp)
                + (r.cache_hit_input_tokens as f64 / 1000000.0 * r.cache_read_price)
        })
        .sum();
    let total_requests = records.len() as u64;
    Ok(Json(BillingSummary {
        total_requests,
        total_cost: (total_cost * 100.0).round() / 100.0,
        balance: 0.0,
    }))
}

#[derive(Deserialize)]
pub(crate) struct PeriodQuery {
    year: Option<i32>,
    month: Option<u32>,
}

#[derive(Serialize)]
pub(crate) struct PeriodSummary {
    year: i32,
    month: u32,
    total_cost: f64,
    total_requests: u64,
    total_tokens: u64,
    by_model: Vec<ModelCostShare>,
    by_channel: Vec<ChannelCostShare>,
}

#[derive(Serialize)]
pub(crate) struct ModelCostShare {
    model: String,
    cost: f64,
    percentage: f64,
}

#[derive(Serialize)]
pub(crate) struct ChannelCostShare {
    channel: String,
    name: String,
    cost: f64,
    percentage: f64,
}

pub(crate) async fn billing_period_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<PeriodSummary>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let can_view_all = state.authz.enforce(&session.role, "admin:bills").await;
    let user_filter: Option<&str> = if can_view_all {
        None
    } else {
        Some(&session.user_id)
    };

    let (total_cost, total_requests, total_tokens) = state.db.period_summary(year, month, user_filter)
        .await.map_err(db_err)?;

    let by_model = state.db.period_model_breakdown(year, month, user_filter)
        .await.map_err(db_err)?
        .into_iter()
        .map(|(model, cost)| {
            let pct = if total_cost > 0.0 { (cost / total_cost * 100.0 * 10.0).round() / 10.0 } else { 0.0 };
            ModelCostShare { model, cost: (cost * 100.0).round() / 100.0, percentage: pct }
        })
        .collect();

    let by_channel = state.db.period_channel_breakdown(year, month, user_filter)
        .await.map_err(db_err)?
        .into_iter()
        .map(|(channel, name, cost)| {
            let pct = if total_cost > 0.0 { (cost / total_cost * 100.0 * 10.0).round() / 10.0 } else { 0.0 };
            ChannelCostShare { channel, name, cost: (cost * 100.0).round() / 100.0, percentage: pct }
        })
        .collect();

    Ok(Json(PeriodSummary {
        year, month,
        total_cost: (total_cost * 100.0).round() / 100.0,
        total_requests,
        total_tokens,
        by_model,
        by_channel,
    }))
}

#[derive(Serialize)]
pub(crate) struct DeductionRecord {
    time: String,
    amount: f64,
    method: String,
}

#[derive(Deserialize)]
pub(crate) struct DeductionQuery {
    year: Option<i32>,
    month: Option<u32>,
    limit: Option<usize>,
    offset: Option<usize>,
}

const DEFAULT_DEDUCTION_PAGE_SIZE: usize = 15;

pub(crate) async fn billing_deductions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<DeductionQuery>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(DEFAULT_DEDUCTION_PAGE_SIZE);
    let offset = q.offset.unwrap_or(0);
    let can_view_all = state.authz.enforce(&session.role, "admin:bills").await;
    let user_filter: Option<&str> = if can_view_all {
        None
    } else {
        Some(&session.user_id)
    };

    let total = state.db.count_daily_deductions(year, month, user_filter)
        .await.map_err(db_err)?;
    let records = state.db.daily_deductions_paginated(year, month, user_filter, limit, offset)
        .await.map_err(db_err)?;
    let items: Vec<DeductionRecord> = records.into_iter().map(|(day, amount, _count)| DeductionRecord {
        time: format!("{}T00:00:00", day),
        amount: -((amount * 100.0).round() / 100.0),
        method: "按量计费".to_string(),
    }).collect();

    Ok(Json(serde_json::json!({ "items": items, "total": total })))
}

pub(crate) async fn billing_topups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    Ok(Json(vec![]))
}

pub(crate) async fn billing_invoices(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, AdminError> {
    let _session = require_session(&state.admin, &headers).await?;
    Ok(Json(vec![]))
}

pub(crate) async fn billing_months(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let can_view_all = state.authz.enforce(&session.role, "admin:bills").await;
    let months = if can_view_all {
        state.db.billing_months().await.map_err(db_err)?
    } else {
        state.db.billing_months_for_user(&session.user_id).await.map_err(db_err)?
    };
    Ok(Json(months))
}

#[derive(Serialize)]
pub(crate) struct MonthSummary {
    month: String,
    total_cost: f64,
    total_requests: u64,
    total_tokens: u64,
}

pub(crate) async fn billing_period_summary_all(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<MonthSummary>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let can_view_all = state.authz.enforce(&session.role, "admin:bills").await;
    let records = if can_view_all {
        state.db.period_summary_all().await.map_err(db_err)?
    } else {
        state.db.period_summary_for_user(&session.user_id).await.map_err(db_err)?
    };
    Ok(Json(records.into_iter().map(|(month, cost, req, tok)| MonthSummary {
        month,
        total_cost: (cost * 100.0).round() / 100.0,
        total_requests: req,
        total_tokens: tok,
    }).collect()))
}
