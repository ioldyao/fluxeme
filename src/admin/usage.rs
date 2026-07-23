use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::domain::usage::UsageFilter;
use crate::server::AppState;

use super::*;

// ── Usage Logs ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct UsageQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    user_id: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    api_format: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct UsageResponse {
    records: Vec<crate::domain::usage::UsageRecord>,
    total: usize,
}

pub(crate) async fn get_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageQuery>,
) -> Result<Json<UsageResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let limit = q.limit.unwrap_or(50);
    let offset = q.offset.unwrap_or(0);

    // Regular users can only see their own usage
    let can_view_all = state.authz.enforce(&session.role, "admin:usage").await;
    let user_filter: Option<String> = if !can_view_all {
        Some(session.user_id.clone())
    } else {
        q.user_id
    };

    let filter = UsageFilter {
        user_id: user_filter,
        model: q.model,
        api_key_name: q.api_key,
        api_format: q.api_format,
        start_date: q.start_date,
        end_date: q.end_date,
    };

    let total = state
        .usage
        .count_filtered(&filter)
        .await
        .map_err(|e| {
            tracing::error!("Usage count failed: {}", e);
            AdminError::internal("Internal server error")
        })?;

    let records = state
        .usage
        .query(limit, offset, &filter)
        .await
        .map_err(|e| {
            tracing::error!("Usage query failed: {}", e);
            AdminError::internal("Internal server error")
        })?;

    Ok(Json(UsageResponse { records, total }))
}

pub(crate) async fn get_usage_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<crate::domain::usage::UsageRecord>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let record = state
        .usage
        .get_detail(&request_id)
        .await
        .map_err(|e| {
            tracing::error!("Usage detail query failed: {}", e);
            AdminError::internal("Internal server error")
        })?
        .ok_or_else(|| AdminError::not_found("Usage record not found"))?;

    if !state.authz.enforce(&session.role, "admin:usage").await && record.user_id != session.user_id {
        return Err(AdminError::not_found("Usage record not found"));
    }

    Ok(Json(record))
}

#[derive(Serialize)]
pub(crate) struct DailyUsage {
    date: String,
    count: i64,
}

pub(crate) async fn daily_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageQuery>,
) -> Result<Json<Vec<DailyUsage>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let days = q.limit.unwrap_or(14) as i64;
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);

    let can_view_all = state.authz.enforce(&session.role, "admin:usage").await;
    let user_filter: Option<&str> = if can_view_all {
        None
    } else {
        Some(&session.user_id)
    };

    let records = state
        .usage
        .daily_counts(&since, user_filter, offset)
        .await.map_err(AdminError::internal)?;

    Ok(Json(
        records
            .into_iter()
            .map(|(date, count)| DailyUsage { date, count })
            .collect(),
    ))
}

// ── Usage Aggregation ─────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct UsageAggregateQuery {
    days: Option<i64>,
    user_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct DailyAggregate {
    date: String,
    count: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    success_count: u64,
    latency_ms: u64,
    cache_hit_tokens: u64,
}

pub(crate) async fn usage_aggregate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageAggregateQuery>,
) -> Result<Json<Vec<DailyAggregate>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let days = q.days.unwrap_or(14);
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);

    let can_view_all = state.authz.enforce(&session.role, "admin:usage").await;
    let user_filter: Option<&str> = if can_view_all {
        q.user_id.as_deref()
    } else {
        Some(&session.user_id)
    };

    let records = state
        .usage
        .daily_stats(&since, user_filter, offset)
        .await.map_err(AdminError::internal)?;

    Ok(Json(
        records
            .into_iter()
            .map(|(date, count, pt, ct, tt, sc, lat, ch)| DailyAggregate {
                date,
                count,
                prompt_tokens: pt,
                completion_tokens: ct,
                total_tokens: tt,
                success_count: sc,
                latency_ms: lat,
                cache_hit_tokens: ch,
            })
            .collect(),
    ))
}

// ── Model Activity ────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct ModelActivity {
    model: String,
    total_requests: u64,
    prompt_tokens: u64,
    completion_tokens: u64,
    cache_hit_tokens: u64,
    success_count: u64,
    failure_count: u64,
}

pub(crate) async fn model_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageAggregateQuery>,
) -> Result<Json<Vec<ModelActivity>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let days = q.days.unwrap_or(7) as i64;
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);
    let can_view_all = state.authz.enforce(&session.role, "admin:usage").await;
    let user_filter: Option<&str> = if can_view_all {
        q.user_id.as_deref()
    } else {
        Some(&session.user_id)
    };
    let records = state
        .db
        .model_activity(&since, user_filter)
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?;
    Ok(Json(
        records
            .into_iter()
            .map(|(model, total, pt, ct, sc, fc, ch)| ModelActivity {
                model,
                total_requests: total,
                prompt_tokens: pt,
                completion_tokens: ct,
                cache_hit_tokens: ch,
                success_count: sc,
                failure_count: fc,
            })
            .collect(),
    ))
}
