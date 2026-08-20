use std::sync::Arc;

use axum::extract::{Path, Query, State};
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
    if !(2020..=2100).contains(&year) {
        return Err(AdminError::bad_request("Year out of range (2020-2100)"));
    }
    if !(1..=12).contains(&month) {
        return Err(AdminError::bad_request("Month must be between 1 and 12"));
    }
    Ok(())
}

fn month_bounds(year: i32, month: u32) -> (String, String) {
    let start = format!("{}-{:02}-01 00:00:00", year, month);
    let end = if month == 12 {
        format!("{}-01-01 00:00:00", year + 1)
    } else {
        format!("{}-{:02}-01 00:00:00", year, month + 1)
    };
    (start, end)
}

fn share_percentage(cost: Decimal, total_cost: Decimal) -> f64 {
    if total_cost > Decimal::ZERO {
        let ratio = cost / total_cost;
        let hundred = Decimal::from(100);
        let ten = Decimal::from(10);
        ((ratio * hundred * ten).round() / ten)
            .to_f64()
            .unwrap_or(0.0)
    } else {
        0.0
    }
}

fn map_model_cost_shares(rows: Vec<(String, Decimal)>, total_cost: Decimal) -> Vec<ModelCostShare> {
    rows.into_iter()
        .map(|(model, cost)| ModelCostShare {
            model,
            cost,
            percentage: share_percentage(cost, total_cost),
        })
        .collect()
}

fn map_channel_cost_shares(
    rows: Vec<(String, String, Decimal)>,
    total_cost: Decimal,
) -> Vec<ChannelCostShare> {
    rows.into_iter()
        .map(|(channel, name, cost)| ChannelCostShare {
            channel,
            name,
            cost,
            percentage: share_percentage(cost, total_cost),
        })
        .collect()
}

fn map_token_cost_breakdown(
    rows: Vec<(String, u64, Decimal)>,
    total_cost: Decimal,
) -> Vec<TokenCostBreakdownRow> {
    rows.into_iter()
        .map(
            |(token_type, total_tokens, total_cost_amount)| TokenCostBreakdownRow {
                token_type,
                total_tokens,
                total_cost: total_cost_amount,
                percentage: share_percentage(total_cost_amount, total_cost),
            },
        )
        .collect()
}

fn validate_scope(_team_id: Option<&str>, _user_id: Option<&str>) -> Result<(), AdminError> {
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

#[derive(Serialize)]
pub(crate) struct AdminBillingSummary {
    total_requests: u64,
    total_tokens: u64,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
}

pub(crate) async fn billing_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<BillingSummary>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let summaries = state
        .db
        .period_summary_for_user(&session.user_id)
        .await
        .map_err(db_err)?;
    let total_cost = summaries
        .iter()
        .map(|(_, cost, _, _)| *cost)
        .fold(Decimal::ZERO, |acc, cost| acc + cost);
    let total_requests = summaries.iter().map(|(_, _, requests, _)| *requests).sum();
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

pub(crate) async fn admin_billing_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AdminBillingSummary>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let summaries = state.db.period_summary_all().await.map_err(db_err)?;
    let total_cost = summaries
        .iter()
        .map(|(_, cost, _, _)| *cost)
        .fold(Decimal::ZERO, |acc, cost| acc + cost);
    let total_requests = summaries.iter().map(|(_, _, requests, _)| *requests).sum();
    let total_tokens = summaries.iter().map(|(_, _, _, tokens)| *tokens).sum();

    Ok(Json(AdminBillingSummary {
        total_requests,
        total_tokens,
        total_cost,
    }))
}

pub(crate) async fn admin_billing_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<AdminBillingActivity>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    validate_year_month(year, month)?;

    let (active_teams, active_users) = state
        .db
        .admin_billing_active_counts(year, month)
        .await
        .map_err(db_err)?;

    Ok(Json(AdminBillingActivity {
        year,
        month,
        active_teams,
        active_users,
    }))
}

pub(crate) async fn admin_billing_team_spend_ranking(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<BillingRankingQuery>,
) -> Result<Json<TeamSpendRankingResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(10).max(1).min(100);
    validate_year_month(year, month)?;

    let items = state
        .db
        .admin_billing_team_spend_ranking(year, month, limit)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(
            |(team_id, team_name, total_cost, total_requests, total_tokens, active_users)| {
                TeamSpendRankItem {
                    team_id,
                    team_name,
                    total_cost,
                    total_requests,
                    total_tokens,
                    active_users,
                }
            },
        )
        .collect();

    Ok(Json(TeamSpendRankingResponse { items }))
}

pub(crate) async fn admin_billing_teams(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<BillingTeamsQuery>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(20).max(1).min(100);
    let offset = q.offset.unwrap_or(0);
    validate_year_month(year, month)?;

    let (items, total) = state
        .db
        .admin_billing_teams_page(
            year,
            month,
            q.search.as_deref(),
            q.sort_by.as_deref(),
            q.sort_order.as_deref(),
            limit,
            offset,
        )
        .await
        .map_err(db_err)?;

    let items: Vec<TeamBillingRow> = items
        .into_iter()
        .map(
            |(
                team_id,
                team_name,
                owner_id,
                total_cost,
                total_requests,
                total_tokens,
                active_users,
                last_billed_at,
            )| TeamBillingRow {
                team_id,
                team_name,
                owner_id,
                total_cost,
                total_requests,
                total_tokens,
                active_users,
                last_billed_at,
            },
        )
        .collect();

    Ok(Json(serde_json::json!({ "items": items, "total": total })))
}

pub(crate) async fn admin_billing_team_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Query(q): Query<BillingTeamsQuery>,
) -> Result<Json<TeamBillingUsersResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(20).max(1).min(100);
    let offset = q.offset.unwrap_or(0);
    validate_year_month(year, month)?;

    let team = state
        .db
        .get_team(&team_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Team not found"))?;

    let (items, total) = state
        .db
        .admin_billing_team_users_page(&team_id, year, month, limit, offset)
        .await
        .map_err(db_err)?;

    let items = items
        .into_iter()
        .map(
            |(user_id, user_name, total_cost, total_requests, total_tokens, last_billed_at)| {
                TeamBillingUsersRow {
                    user_id,
                    user_name,
                    total_cost,
                    total_requests,
                    total_tokens,
                    last_billed_at,
                }
            },
        )
        .collect();

    Ok(Json(TeamBillingUsersResponse {
        team: TeamRef {
            team_id: team.id,
            team_name: team.name,
        },
        year,
        month,
        items,
        total,
    }))
}

pub(crate) async fn admin_billing_team_requests(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Query(q): Query<BillingTeamRequestsQuery>,
) -> Result<Json<BillingUsageResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(50).max(1).min(200);
    let offset = q.offset.unwrap_or(0);
    validate_year_month(year, month)?;

    state
        .db
        .get_team(&team_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Team not found"))?;

    let (start, end) = month_bounds(year, month);
    let filter = crate::domain::usage::UsageFilter {
        user_id: q.user_id,
        team_id: Some(team_id),
        model: q.model,
        api_key_name: q.api_key_name,
        api_format: q.api_format,
        start_date: Some(start),
        end_date: Some(end),
    };

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let total = ch.count_usage(&filter).await.map_err(|e| {
        tracing::error!("CH team billing usage count failed: {}", e);
        AdminError::internal("Internal server error")
    })?;
    let records = ch.query_usage(limit, offset, &filter).await.map_err(|e| {
        tracing::error!("CH team billing usage query failed: {}", e);
        AdminError::internal("Internal server error")
    })?;

    Ok(Json(BillingUsageResponse { records, total }))
}

pub(crate) async fn admin_billing_team_user_api_keys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((team_id, user_id)): Path<(String, String)>,
    Query(q): Query<BillingTeamRequestsQuery>,
) -> Result<Json<BillingApiKeyActivityResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(50).max(1).min(200);
    let offset = q.offset.unwrap_or(0);
    validate_year_month(year, month)?;

    let team = state
        .db
        .get_team(&team_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Team not found"))?;

    let (start, end) = month_bounds(year, month);
    let filter = crate::domain::usage::UsageFilter {
        user_id: Some(user_id.clone()),
        team_id: Some(team_id.clone()),
        model: q.model,
        api_key_name: None,
        api_format: q.api_format,
        start_date: Some(start),
        end_date: Some(end),
    };

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let total = ch.count_api_key_activity(&filter).await.map_err(|e| {
        tracing::error!("CH team billing api-key count failed: {}", e);
        AdminError::internal("Internal server error")
    })?;
    let items = ch
        .query_api_key_activity(&filter, limit, offset)
        .await
        .map_err(|e| {
            tracing::error!("CH team billing api-key query failed: {}", e);
            AdminError::internal("Internal server error")
        })?;

    let items = items
        .into_iter()
        .map(
            |(api_key_name, total_requests, total_tokens, last_request_at)| {
                BillingApiKeyActivityRow {
                    api_key_name,
                    total_requests,
                    total_tokens,
                    last_request_at: Some(last_request_at),
                }
            },
        )
        .collect();

    Ok(Json(BillingApiKeyActivityResponse {
        team: TeamRef {
            team_id: team.id,
            team_name: team.name,
        },
        user_id,
        year,
        month,
        stable_key_identity: false,
        grouping_field: "api_key_name",
        items,
        total,
    }))
}

#[derive(Deserialize)]
pub(crate) struct PeriodQuery {
    year: Option<i32>,
    month: Option<u32>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct BillingScopeQuery {
    year: Option<i32>,
    month: Option<u32>,
    team_id: Option<String>,
    user_id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BillingRankingQuery {
    year: Option<i32>,
    month: Option<u32>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub(crate) struct BillingTeamsQuery {
    year: Option<i32>,
    month: Option<u32>,
    limit: Option<usize>,
    offset: Option<usize>,
    search: Option<String>,
    sort_by: Option<String>,
    sort_order: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BillingTeamRequestsQuery {
    year: Option<i32>,
    month: Option<u32>,
    limit: Option<usize>,
    offset: Option<usize>,
    user_id: Option<String>,
    api_key_name: Option<String>,
    model: Option<String>,
    api_format: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct BillingApiKeyDetailQuery {
    year: Option<i32>,
    month: Option<u32>,
    limit: Option<usize>,
    offset: Option<usize>,
    api_format: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct BillingActivityResponse {
    activities: Vec<serde_json::Value>,
    total: usize,
}

#[derive(Serialize)]
pub(crate) struct BillingUsageResponse {
    records: Vec<crate::domain::usage::UsageRecord>,
    total: usize,
}

#[derive(Serialize)]
pub(crate) struct BillingApiKeyActivityRow {
    api_key_name: Option<String>,
    total_requests: u64,
    total_tokens: u64,
    last_request_at: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct BillingApiKeyActivityResponse {
    team: TeamRef,
    user_id: String,
    year: i32,
    month: u32,
    stable_key_identity: bool,
    grouping_field: &'static str,
    items: Vec<BillingApiKeyActivityRow>,
    total: usize,
}

#[derive(Serialize)]
pub(crate) struct AdminBillingActivity {
    year: i32,
    month: u32,
    active_teams: u64,
    active_users: u64,
}

#[derive(Serialize)]
pub(crate) struct TeamSpendRankItem {
    team_id: String,
    team_name: String,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    total_requests: u64,
    total_tokens: u64,
    active_users: u64,
}

#[derive(Serialize)]
pub(crate) struct TeamSpendRankingResponse {
    items: Vec<TeamSpendRankItem>,
}

#[derive(Serialize)]
pub(crate) struct TeamBillingRow {
    team_id: String,
    team_name: String,
    owner_id: String,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    total_requests: u64,
    total_tokens: u64,
    active_users: u64,
    last_billed_at: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct TeamBillingUsersRow {
    user_id: String,
    user_name: String,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    total_requests: u64,
    total_tokens: u64,
    last_billed_at: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct TeamRef {
    team_id: String,
    team_name: String,
}

#[derive(Serialize)]
pub(crate) struct TeamBillingUsersResponse {
    team: TeamRef,
    year: i32,
    month: u32,
    items: Vec<TeamBillingUsersRow>,
    total: usize,
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
    token_cost_breakdown: Vec<TokenCostBreakdownRow>,
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

#[derive(Serialize)]
pub(crate) struct TokenCostBreakdownRow {
    token_type: String,
    total_tokens: u64,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    percentage: f64,
}

#[derive(Serialize)]
pub(crate) struct BillingTrendPoint {
    date: String,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    total_requests: u64,
    total_tokens: u64,
}

#[derive(Serialize)]
pub(crate) struct AdminBillingUserSpendRow {
    team_id: Option<String>,
    team_name: Option<String>,
    team_count: u64,
    multi_team: bool,
    user_id: String,
    user_name: String,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    total_requests: u64,
    total_tokens: u64,
    api_key_count: u64,
    last_billed_at: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AdminBillingUserSpendRankingResponse {
    items: Vec<AdminBillingUserSpendRow>,
}

#[derive(Serialize)]
pub(crate) struct AdminBillingUserApiKeyCostRow {
    api_key_name: Option<String>,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    total_requests: u64,
    total_tokens: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    cache_hit_input_tokens: u64,
    primary_model: Option<String>,
    last_request_at: Option<String>,
    #[serde(default)]
    team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct AdminBillingUserApiKeyCostResponse {
    team: Option<TeamRef>,
    user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_name: Option<String>,
    year: i32,
    month: u32,
    stable_key_identity: bool,
    grouping_field: &'static str,
    items: Vec<AdminBillingUserApiKeyCostRow>,
    total: usize,
}

#[derive(Serialize)]
pub(crate) struct AdminBillingApiKeyDetailModelRow {
    model: String,
    total_requests: u64,
    total_tokens: u64,
}

#[derive(Serialize)]
pub(crate) struct AdminBillingApiKeyDetailChannelRow {
    channel_id: String,
    total_requests: u64,
}

#[derive(Serialize)]
pub(crate) struct AdminBillingApiKeyDetailResponse {
    team: Option<TeamRef>,
    user_id: String,
    api_key_name: String,
    year: i32,
    month: u32,
    stable_key_identity: bool,
    grouping_field: &'static str,
    total_requests: u64,
    total_tokens: u64,
    top_models: Vec<AdminBillingApiKeyDetailModelRow>,
    top_channels: Vec<AdminBillingApiKeyDetailChannelRow>,
    recent_requests: Vec<crate::domain::usage::UsageRecord>,
}

pub(crate) async fn billing_activities(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<BillingActivityResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    validate_year_month(year, month)?;
    let start = format!("{}-{:02}-01T00:00:00", year, month);
    let end = if month == 12 {
        format!("{}-01-01T00:00:00", year + 1)
    } else {
        format!("{}-{:02}-01T00:00:00", year, month + 1)
    };
    let activities = state
        .db
        .list_billing_activities(
            &start,
            &end,
            Some(&session.user_id),
            q.limit.unwrap_or(50),
            q.offset.unwrap_or(0),
        )
        .await
        .map_err(db_err)?;
    let total = state
        .db
        .count_billing_activities(&start, &end, Some(&session.user_id))
        .await
        .map_err(db_err)?;
    Ok(Json(BillingActivityResponse { activities: activities.into_iter().map(|a| serde_json::json!({
        "timestamp": a.timestamp, "request_id": a.request_id, "model": a.model, "channel_id": a.channel_id,
        "activity_status": a.activity_status, "status_reason": a.status_reason, "status_code": a.status_code, "success": a.success,
        "prompt_tokens": a.prompt_tokens, "completion_tokens": a.completion_tokens, "cache_hit_input_tokens": a.cache_hit_input_tokens,
        "cache_write_tokens": a.cache_write_tokens, "total_tokens": a.total_tokens, "package_units": a.package_units,
        "package_grant_id": a.package_grant_id, "wallet_amount": a.wallet_amount, "priced_cost_amount": a.priced_cost_amount,
        "charge_source": a.charge_source, "account_type": a.account_type, "team_id": a.team_id, "api_key_name": a.api_key_name,
        "latency_ms": a.latency_ms, "reservation_id": a.reservation_id,
    })).collect(), total }))
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
    let uid: &str = &session.user_id;

    let (total_cost, total_requests, total_tokens) = state
        .db
        .period_summary(year, month, Some(uid))
        .await
        .map_err(db_err)?;
    let token_cost_breakdown = map_token_cost_breakdown(
        state
            .db
            .period_token_breakdown(year, month, Some(uid))
            .await
            .map_err(db_err)?,
        total_cost,
    );

    let by_model = map_model_cost_shares(
        state
            .db
            .period_model_breakdown(year, month, Some(uid))
            .await
            .map_err(db_err)?,
        total_cost,
    );

    let by_channel = map_channel_cost_shares(
        state
            .db
            .period_channel_breakdown(year, month, Some(uid))
            .await
            .map_err(db_err)?,
        total_cost,
    );

    Ok(Json(PeriodSummary {
        year,
        month,
        total_cost,
        total_requests,
        total_tokens,
        by_model,
        by_channel,
        token_cost_breakdown,
    }))
}

pub(crate) async fn admin_billing_activities(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<BillingActivityResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;
    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    validate_year_month(year, month)?;
    let start = format!("{}-{:02}-01T00:00:00", year, month);
    let end = if month == 12 {
        format!("{}-01-01T00:00:00", year + 1)
    } else {
        format!("{}-{:02}-01T00:00:00", year, month + 1)
    };
    let activities = state
        .db
        .list_billing_activities(
            &start,
            &end,
            None,
            q.limit.unwrap_or(100),
            q.offset.unwrap_or(0),
        )
        .await
        .map_err(db_err)?;
    let total = state
        .db
        .count_billing_activities(&start, &end, None)
        .await
        .map_err(db_err)?;
    Ok(Json(BillingActivityResponse { activities: activities.into_iter().map(|a| serde_json::json!({
        "timestamp": a.timestamp, "request_id": a.request_id, "user_id": a.user_id, "user_name": a.user_name,
        "model": a.model, "channel_id": a.channel_id, "activity_status": a.activity_status, "status_reason": a.status_reason,
        "status_code": a.status_code, "success": a.success, "prompt_tokens": a.prompt_tokens, "completion_tokens": a.completion_tokens,
        "cache_hit_input_tokens": a.cache_hit_input_tokens, "cache_write_tokens": a.cache_write_tokens, "total_tokens": a.total_tokens,
        "package_units": a.package_units, "package_grant_id": a.package_grant_id, "wallet_amount": a.wallet_amount,
        "priced_cost_amount": a.priced_cost_amount, "charge_source": a.charge_source, "account_type": a.account_type,
        "team_id": a.team_id, "api_key_name": a.api_key_name, "latency_ms": a.latency_ms, "reservation_id": a.reservation_id,
    })).collect(), total }))
}

pub(crate) async fn admin_billing_period_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PeriodQuery>,
) -> Result<Json<PeriodSummary>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    validate_year_month(year, month)?;

    let (total_cost, total_requests, total_tokens) = state
        .db
        .period_summary(year, month, None)
        .await
        .map_err(db_err)?;
    let token_cost_breakdown = map_token_cost_breakdown(
        state
            .db
            .period_token_breakdown(year, month, None)
            .await
            .map_err(db_err)?,
        total_cost,
    );

    let by_model = map_model_cost_shares(
        state
            .db
            .period_model_breakdown(year, month, None)
            .await
            .map_err(db_err)?,
        total_cost,
    );

    let by_channel = map_channel_cost_shares(
        state
            .db
            .period_channel_breakdown(year, month, None)
            .await
            .map_err(db_err)?,
        total_cost,
    );

    Ok(Json(PeriodSummary {
        year,
        month,
        total_cost,
        total_requests,
        total_tokens,
        by_model,
        by_channel,
        token_cost_breakdown,
    }))
}

pub(crate) async fn admin_billing_scoped_period_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<BillingScopeQuery>,
) -> Result<Json<PeriodSummary>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    validate_year_month(year, month)?;
    validate_scope(q.team_id.as_deref(), q.user_id.as_deref())?;

    let (total_cost, total_requests, total_tokens, token_cost_rows) = state
        .db
        .admin_billing_scoped_period_summary(
            year,
            month,
            q.team_id.as_deref(),
            q.user_id.as_deref(),
        )
        .await
        .map_err(db_err)?;
    let token_cost_breakdown = map_token_cost_breakdown(token_cost_rows, total_cost);

    let by_model = map_model_cost_shares(
        state
            .db
            .admin_billing_scoped_model_breakdown(
                year,
                month,
                q.team_id.as_deref(),
                q.user_id.as_deref(),
            )
            .await
            .map_err(db_err)?,
        total_cost,
    );

    let by_channel = map_channel_cost_shares(
        state
            .db
            .admin_billing_scoped_channel_breakdown(
                year,
                month,
                q.team_id.as_deref(),
                q.user_id.as_deref(),
            )
            .await
            .map_err(db_err)?,
        total_cost,
    );

    Ok(Json(PeriodSummary {
        year,
        month,
        total_cost,
        total_requests,
        total_tokens,
        by_model,
        by_channel,
        token_cost_breakdown,
    }))
}

pub(crate) async fn admin_billing_daily_trend(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<BillingScopeQuery>,
) -> Result<Json<Vec<BillingTrendPoint>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    validate_year_month(year, month)?;
    validate_scope(q.team_id.as_deref(), q.user_id.as_deref())?;

    let items = state
        .db
        .admin_billing_daily_trend(year, month, q.team_id.as_deref(), q.user_id.as_deref())
        .await
        .map_err(db_err)?
        .into_iter()
        .map(
            |(date, total_cost, total_requests, total_tokens)| BillingTrendPoint {
                date,
                total_cost,
                total_requests,
                total_tokens,
            },
        )
        .collect();

    Ok(Json(items))
}

pub(crate) async fn admin_billing_user_spend_ranking_scoped(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<BillingRankingQuery>,
) -> Result<Json<AdminBillingUserSpendRankingResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(10).max(1).min(100);
    validate_year_month(year, month)?;

    let items = state
        .db
        .admin_billing_user_spend_ranking(year, month, limit)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(
            |(
                team_id,
                team_name,
                team_count,
                multi_team,
                user_id,
                user_name,
                total_cost,
                total_requests,
                total_tokens,
                api_key_count,
                last_billed_at,
            )| {
                AdminBillingUserSpendRow {
                    team_id,
                    team_name,
                    team_count,
                    multi_team,
                    user_id,
                    user_name,
                    total_cost,
                    total_requests,
                    total_tokens,
                    api_key_count,
                    last_billed_at,
                }
            },
        )
        .collect();

    Ok(Json(AdminBillingUserSpendRankingResponse { items }))
}

pub(crate) async fn admin_billing_user_api_key_costs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((team_id, user_id)): Path<(String, String)>,
    Query(q): Query<BillingTeamsQuery>,
) -> Result<Json<AdminBillingUserApiKeyCostResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(20).max(1).min(100);
    let offset = q.offset.unwrap_or(0);
    validate_year_month(year, month)?;

    let team = state
        .db
        .get_team(&team_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Team not found"))?;

    let (items, total) = state
        .db
        .admin_billing_user_api_keys_page(Some(&team_id), &user_id, year, month, limit, offset)
        .await
        .map_err(db_err)?;

    let items = items
        .into_iter()
        .map(
            |(
                api_key_name,
                total_cost,
                total_requests,
                total_tokens,
                prompt_tokens,
                completion_tokens,
                cache_hit_input_tokens,
                primary_model,
                last_request_at,
                _team_id,
                api_key_enabled,
                api_key,
            )| {
                AdminBillingUserApiKeyCostRow {
                    api_key_name,
                    total_cost,
                    total_requests,
                    total_tokens,
                    prompt_tokens,
                    completion_tokens,
                    cache_hit_input_tokens,
                    primary_model,
                    last_request_at,
                    team_id: _team_id,
                    api_key_enabled,
                    api_key,
                }
            },
        )
        .collect();

    let user_name = state
        .db
        .get_user(&user_id)
        .await
        .map_err(db_err)?
        .map(|u| u.name);

    Ok(Json(AdminBillingUserApiKeyCostResponse {
        team: Some(TeamRef {
            team_id: team.id,
            team_name: team.name,
        }),
        user_id,
        user_name,
        year,
        month,
        stable_key_identity: false,
        grouping_field: "api_key_name",
        items,
        total,
    }))
}

pub(crate) async fn admin_billing_api_key_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((team_id, user_id, api_key_name)): Path<(String, String, String)>,
    Query(q): Query<BillingApiKeyDetailQuery>,
) -> Result<Json<AdminBillingApiKeyDetailResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(20).max(1).min(100);
    let offset = q.offset.unwrap_or(0);
    validate_year_month(year, month)?;

    let team = state
        .db
        .get_team(&team_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Team not found"))?;

    let (start, end) = month_bounds(year, month);
    let filter = crate::domain::usage::UsageFilter {
        user_id: Some(user_id.clone()),
        team_id: Some(team_id.clone()),
        model: None,
        api_key_name: Some(api_key_name.clone()),
        api_format: q.api_format,
        start_date: Some(start),
        end_date: Some(end),
    };

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let (total_requests, total_tokens, top_models, top_channels, recent_requests) = ch
        .query_api_key_detail(&filter, limit, offset)
        .await
        .map_err(|e| {
            tracing::error!("CH billing api-key detail query failed: {}", e);
            AdminError::internal("Internal server error")
        })?;

    Ok(Json(AdminBillingApiKeyDetailResponse {
        team: Some(TeamRef {
            team_id: team.id,
            team_name: team.name,
        }),
        user_id,
        api_key_name,
        year,
        month,
        stable_key_identity: false,
        grouping_field: "api_key_name",
        total_requests,
        total_tokens,
        top_models: top_models
            .into_iter()
            .map(
                |(model, total_requests, total_tokens)| AdminBillingApiKeyDetailModelRow {
                    model,
                    total_requests,
                    total_tokens,
                },
            )
            .collect(),
        top_channels: top_channels
            .into_iter()
            .map(
                |(channel_id, total_requests)| AdminBillingApiKeyDetailChannelRow {
                    channel_id,
                    total_requests,
                },
            )
            .collect(),
        recent_requests,
    }))
}

pub(crate) async fn admin_billing_user_api_key_costs_global(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(user_id): Path<String>,
    Query(q): Query<BillingTeamsQuery>,
) -> Result<Json<AdminBillingUserApiKeyCostResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(20).max(1).min(100);
    let offset = q.offset.unwrap_or(0);
    validate_year_month(year, month)?;

    let (items, total) = state
        .db
        .admin_billing_user_api_keys_page(None, &user_id, year, month, limit, offset)
        .await
        .map_err(db_err)?;

    let items = items
        .into_iter()
        .map(
            |(
                api_key_name,
                total_cost,
                total_requests,
                total_tokens,
                prompt_tokens,
                completion_tokens,
                cache_hit_input_tokens,
                primary_model,
                last_request_at,
                _team_id,
                api_key_enabled,
                api_key,
            )| {
                AdminBillingUserApiKeyCostRow {
                    api_key_name,
                    total_cost,
                    total_requests,
                    total_tokens,
                    prompt_tokens,
                    completion_tokens,
                    cache_hit_input_tokens,
                    primary_model,
                    last_request_at,
                    team_id: _team_id,
                    api_key_enabled,
                    api_key,
                }
            },
        )
        .collect();

    let user_name = state
        .db
        .get_user(&user_id)
        .await
        .map_err(db_err)?
        .map(|u| u.name);

    Ok(Json(AdminBillingUserApiKeyCostResponse {
        team: None,
        user_id,
        user_name,
        year,
        month,
        stable_key_identity: false,
        grouping_field: "api_key_name",
        items,
        total,
    }))
}

pub(crate) async fn admin_billing_api_key_detail_global(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((user_id, api_key_name)): Path<(String, String)>,
    Query(q): Query<BillingApiKeyDetailQuery>,
) -> Result<Json<AdminBillingApiKeyDetailResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(20).max(1).min(100);
    let offset = q.offset.unwrap_or(0);
    validate_year_month(year, month)?;

    let (start, end) = month_bounds(year, month);
    let filter = crate::domain::usage::UsageFilter {
        user_id: Some(user_id.clone()),
        team_id: None,
        model: None,
        api_key_name: Some(api_key_name.clone()),
        api_format: q.api_format,
        start_date: Some(start),
        end_date: Some(end),
    };

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let (total_requests, total_tokens, top_models, top_channels, recent_requests) = ch
        .query_api_key_detail(&filter, limit, offset)
        .await
        .map_err(|e| {
            tracing::error!("CH billing api-key detail query failed: {}", e);
            AdminError::internal("Internal server error")
        })?;

    Ok(Json(AdminBillingApiKeyDetailResponse {
        team: None,
        user_id,
        api_key_name,
        year,
        month,
        stable_key_identity: false,
        grouping_field: "api_key_name",
        total_requests,
        total_tokens,
        top_models: top_models
            .into_iter()
            .map(
                |(model, total_requests, total_tokens)| AdminBillingApiKeyDetailModelRow {
                    model,
                    total_requests,
                    total_tokens,
                },
            )
            .collect(),
        top_channels: top_channels
            .into_iter()
            .map(
                |(channel_id, total_requests)| AdminBillingApiKeyDetailChannelRow {
                    channel_id,
                    total_requests,
                },
            )
            .collect(),
        recent_requests,
    }))
}

pub(crate) async fn admin_billing_request_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<crate::domain::usage::UsageRecord>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let record = ch
        .get_usage_detail(&request_id)
        .await
        .map_err(|e| {
            tracing::error!("CH billing request detail query failed: {}", e);
            AdminError::internal("Internal server error")
        })?
        .ok_or_else(|| AdminError::not_found("Usage record not found"))?;

    Ok(Json(record))
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

#[derive(Deserialize)]
pub(crate) struct ScopedDeductionQuery {
    year: Option<i32>,
    month: Option<u32>,
    limit: Option<usize>,
    offset: Option<usize>,
    team_id: Option<String>,
    user_id: Option<String>,
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
    let uid: &str = &session.user_id;

    let total = state
        .db
        .count_daily_deductions(year, month, Some(uid))
        .await
        .map_err(db_err)?;
    let records = state
        .db
        .daily_deductions_paginated(year, month, Some(uid), limit, offset)
        .await
        .map_err(db_err)?;
    let items: Vec<DeductionRecord> = records
        .into_iter()
        .map(|(day, amount, _count)| DeductionRecord {
            time: format!("{}T00:00:00", day),
            amount: -amount,
            method: "usage".to_string(),
        })
        .collect();

    Ok(Json(serde_json::json!({ "items": items, "total": total })))
}

pub(crate) async fn admin_billing_deductions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ScopedDeductionQuery>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let now = chrono::Utc::now();
    let year = q.year.unwrap_or_else(|| now.year());
    let month = q.month.unwrap_or_else(|| now.month());
    let limit = q.limit.unwrap_or(DEFAULT_DEDUCTION_PAGE_SIZE);
    let offset = q.offset.unwrap_or(0);
    validate_year_month(year, month)?;
    validate_scope(q.team_id.as_deref(), q.user_id.as_deref())?;

    let total = state
        .db
        .admin_billing_scoped_count_daily_deductions(
            year,
            month,
            q.team_id.as_deref(),
            q.user_id.as_deref(),
        )
        .await
        .map_err(db_err)?;
    let records = state
        .db
        .admin_billing_scoped_daily_deductions_paginated(
            year,
            month,
            q.team_id.as_deref(),
            q.user_id.as_deref(),
            limit,
            offset,
        )
        .await
        .map_err(db_err)?;
    let items: Vec<DeductionRecord> = records
        .into_iter()
        .map(|(day, amount, _count)| DeductionRecord {
            time: format!("{}T00:00:00", day),
            amount: -amount,
            method: "usage".to_string(),
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
    let months = state
        .db
        .billing_months_for_user(&session.user_id)
        .await
        .map_err(db_err)?;
    Ok(Json(months))
}

pub(crate) async fn admin_billing_months(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;

    let months = state.db.billing_months().await.map_err(db_err)?;
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
    let records = state
        .db
        .period_summary_for_user(&session.user_id)
        .await
        .map_err(db_err)?;
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

pub(crate) async fn admin_billing_period_summary_all(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<BillingScopeQuery>,
) -> Result<Json<Vec<MonthSummary>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:bills").await?;
    validate_scope(q.team_id.as_deref(), q.user_id.as_deref())?;

    let records = state
        .db
        .admin_billing_scoped_period_summary_all(q.team_id.as_deref(), q.user_id.as_deref())
        .await
        .map_err(db_err)?;
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
