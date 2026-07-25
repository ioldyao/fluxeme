use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Datelike;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::server::AppState;

use super::*;

fn validate_year_month(year: i32, month: u32) -> Result<(), AdminError> {
    if year == 0 && month == 0 {
        return Ok(()); // unset/placeholder — return empty results
    }
    if year < 2020 || year > 2100 {
        return Err(AdminError::bad_request("Year out of range (2020-2100)"));
    }
    if month < 1 || month > 12 {
        return Err(AdminError::bad_request("Month must be between 1 and 12"));
    }
    Ok(())
}

#[derive(Serialize)]
pub(crate) struct BillingSummary {
    total_requests: u64,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    #[serde(with = "rust_decimal::serde::float")]
    balance: Decimal,
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
        .await
        .map_err(AdminError::internal)?;
    let total_cost = records
        .iter()
        .fold(Decimal::ZERO, |acc, r| {
            let pp = if r.prompt_price > Decimal::ZERO {
                r.prompt_price
            } else {
                Decimal::ZERO
            };
            let cp = if r.completion_price > Decimal::ZERO {
                r.completion_price
            } else {
                Decimal::ZERO
            };
            acc + (Decimal::from(r.prompt_tokens) / Decimal::from(1000000) * pp)
                + (Decimal::from(r.completion_tokens) / Decimal::from(1000000) * cp)
                + (Decimal::from(r.cache_hit_input_tokens) / Decimal::from(1000000) * r.cache_read_price)
        });
    let total_requests = records.len() as u64;
    let (balance, _) = state
        .db
        .get_wallet_balance(&session.user_id)
        .await
        .map_err(db_err)?;
    Ok(Json(BillingSummary {
        total_requests,
        total_cost,
        balance,
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
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    total_requests: u64,
    total_tokens: u64,
    by_model: Vec<ModelCostShare>,
    by_channel: Vec<ChannelCostShare>,
}

#[derive(Serialize)]
pub(crate) struct ModelCostShare {
    model: String,
    #[serde(with = "rust_decimal::serde::float")]
    cost: Decimal,
    percentage: f64,
}

#[derive(Serialize)]
pub(crate) struct ChannelCostShare {
    channel: String,
    name: String,
    #[serde(with = "rust_decimal::serde::float")]
    cost: Decimal,
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
    validate_year_month(year, month)?;
    let can_view_all = state.authz.enforce(&session.role, "admin:bills").await;
    let user_filter: Option<&str> = if can_view_all {
        None
    } else {
        Some(&session.user_id)
    };

    let (total_cost, total_requests, total_tokens) = state
        .db
        .period_summary(year, month, user_filter)
        .await
        .map_err(db_err)?;

    let by_model = state
        .db
        .period_model_breakdown(year, month, user_filter)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|(model, cost)| {
            let pct = if total_cost > Decimal::ZERO {
                let ratio = cost / total_cost;
                let hundred = Decimal::from(100);
                let ten = Decimal::from(10);
                (ratio * hundred * ten).round() / ten
            } else {
                Decimal::ZERO
            };
            ModelCostShare {
                model,
                cost,
                percentage: pct.to_f64().unwrap_or(0.0),
            }
        })
        .collect();

    let by_channel = state
        .db
        .period_channel_breakdown(year, month, user_filter)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|(channel, name, cost)| {
            let pct = if total_cost > Decimal::ZERO {
                let ratio = cost / total_cost;
                let hundred = Decimal::from(100);
                let ten = Decimal::from(10);
                (ratio * hundred * ten).round() / ten
            } else {
                Decimal::ZERO
            };
            ChannelCostShare {
                channel,
                name,
                cost,
                percentage: pct.to_f64().unwrap_or(0.0),
            }
        })
        .collect();

    Ok(Json(PeriodSummary {
        year,
        month,
        total_cost,
        total_requests,
        total_tokens,
        by_model,
        by_channel,
    }))
}

#[derive(Serialize)]
pub(crate) struct DeductionRecord {
    time: String,
    #[serde(with = "rust_decimal::serde::float")]
    amount: Decimal,
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

    let total = state
        .db
        .count_daily_deductions(year, month, user_filter)
        .await
        .map_err(db_err)?;
    let records = state
        .db
        .daily_deductions_paginated(year, month, user_filter, limit, offset)
        .await
        .map_err(db_err)?;
    let items: Vec<DeductionRecord> = records
        .into_iter()
        .map(|(day, amount, _count)| DeductionRecord {
            time: format!("{}T00:00:00", day),
            amount: -amount,
            method: "按量计费".to_string(),
        })
        .collect();

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
        state
            .db
            .billing_months_for_user(&session.user_id)
            .await
            .map_err(db_err)?
    };
    Ok(Json(months))
}

#[derive(Serialize)]
pub(crate) struct MonthSummary {
    month: String,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
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
        state
            .db
            .period_summary_for_user(&session.user_id)
            .await
            .map_err(db_err)?
    };
    Ok(Json(
        records
            .into_iter()
            .map(|(month, cost, req, tok)| MonthSummary {
                month,
                total_cost: cost,
                total_requests: req,
                total_tokens: tok,
            })
            .collect(),
    ))
}
