use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};

use crate::ch_backend::normalize_clickhouse_datetime;
use crate::domain::usage::UsageFilter;
use crate::server::AppState;

use super::*;

// ── Usage Logs ────────────────────────────────────────────────────

#[derive(Clone, Deserialize)]
pub(crate) struct UsageQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    user_id: Option<String>,
    team_id: Option<String>,
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

fn validate_usage_datetime(
    value: Option<String>,
    field_name: &str,
) -> Result<Option<String>, AdminError> {
    value
        .filter(|candidate| !candidate.is_empty())
        .map(|candidate| {
            normalize_clickhouse_datetime(&candidate)
                .map(|_| candidate)
                .map_err(|_| {
                    AdminError::bad_request(format!("{field_name} must be a valid datetime"))
                })
        })
        .transpose()
}

fn build_usage_filter(
    user_id: Option<String>,
    query: UsageQuery,
) -> Result<UsageFilter, AdminError> {
    Ok(UsageFilter {
        user_id,
        team_id: query.team_id,
        model: query.model,
        api_key_name: query.api_key,
        api_format: query.api_format,
        start_date: validate_usage_datetime(query.start_date, "start_date")?,
        end_date: validate_usage_datetime(query.end_date, "end_date")?,
    })
}

// Importers/callers: shared by ui/src/pages/Usage.tsx via /api/usage and the
// new self-only dashboard routes below via /api/me/usage. Affected API/data
// schema: UsageResponse { records, total } and query params limit, offset,
// user_id, model, api_key, api_format, start_date, end_date. User instruction:
// "`网关运行总览` 这个前端页面中，哪些还有计算全部用户的，统一修改只看当前个人用户的数据,admin登陆也只看自己的数据".
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
        q.user_id.clone()
    };

    let filter = build_usage_filter(user_filter, q)?;

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let total = ch.count_usage(&filter).await.map_err(|e| {
        tracing::error!("CH usage count failed: {}", e);
        AdminError::internal("Internal server error")
    })?;

    let records = ch.query_usage(limit, offset, &filter).await.map_err(|e| {
        tracing::error!("CH usage query failed: {}", e);
        AdminError::internal("Internal server error")
    })?;

    Ok(Json(UsageResponse { records, total }))
}

pub(crate) async fn get_my_usage(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageQuery>,
) -> Result<Json<UsageResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let limit = q.limit.unwrap_or(50);
    let offset = q.offset.unwrap_or(0);
    let filter = build_usage_filter(Some(session.user_id.clone()), q)?;

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let total = ch.count_usage(&filter).await.map_err(|e| {
        tracing::error!("CH self usage count failed: {}", e);
        AdminError::internal("Internal server error")
    })?;

    let records = ch.query_usage(limit, offset, &filter).await.map_err(|e| {
        tracing::error!("CH self usage query failed: {}", e);
        AdminError::internal("Internal server error")
    })?;

    Ok(Json(UsageResponse { records, total }))
}

#[derive(Deserialize)]
pub(crate) struct UsageBillingQuery {
    request_ids: Option<String>,
}

fn parse_usage_request_ids(query: UsageBillingQuery) -> Result<Vec<String>, AdminError> {
    let request_ids = query
        .request_ids
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|request_id| !request_id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if request_ids.len() > 200 {
        return Err(AdminError::bad_request("Too many request IDs"));
    }
    Ok(request_ids)
}

pub(crate) async fn get_usage_billing(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UsageBillingQuery>,
) -> Result<Json<Vec<crate::db::UsageBillingRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let request_ids = parse_usage_request_ids(query)?;

    state
        .db
        .usage_billing(&session.user_id, &request_ids)
        .await
        .map(Json)
        .map_err(db_err)
}

pub(crate) async fn get_admin_usage_billing(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<UsageBillingQuery>,
) -> Result<Json<Vec<crate::db::UsageBillingRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:usage").await?;
    let request_ids = parse_usage_request_ids(query)?;

    state
        .db
        .usage_billing_for_requests(&request_ids)
        .await
        .map(Json)
        .map_err(db_err)
}

pub(crate) async fn get_usage_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Result<Json<crate::domain::usage::UsageRecord>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let record = ch
        .get_usage_detail(&request_id)
        .await
        .map_err(|e| {
            tracing::error!("CH usage detail query failed: {}", e);
            AdminError::internal("Internal server error")
        })?
        .ok_or_else(|| AdminError::not_found("Usage record not found"))?;

    // Request details (full request/response bodies) are admin-only.
    if !state.authz.enforce(&session.role, "admin:usage").await {
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
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);

    let can_view_all = state.authz.enforce(&session.role, "admin:usage").await;
    let user_filter: Option<&str> = if can_view_all {
        None
    } else {
        Some(&session.user_id)
    };

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let records: Vec<DailyUsage> = ch
        .query_daily_usage_counts(&since, user_filter, offset)
        .await
        .map_err(AdminError::internal)?
        .into_iter()
        .map(|(date, count)| DailyUsage {
            date,
            count: i64::try_from(count).unwrap_or(i64::MAX),
        })
        .collect();

    Ok(Json(records))
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
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);

    let can_view_all = state.authz.enforce(&session.role, "admin:usage").await;
    let user_filter: Option<&str> = if can_view_all {
        q.user_id.as_deref()
    } else {
        Some(&session.user_id)
    };

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let records: Vec<DailyAggregate> = ch
        .query_daily_usage_stats(&since, user_filter, offset)
        .await
        .map_err(AdminError::internal)?
        .into_iter()
        .map(
            |(date, count, pt, ct, tt, sc, lat, ch_tok)| DailyAggregate {
                date,
                count,
                prompt_tokens: pt,
                completion_tokens: ct,
                total_tokens: tt,
                success_count: sc,
                latency_ms: lat,
                cache_hit_tokens: ch_tok,
            },
        )
        .collect();

    Ok(Json(records))
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsageAnalyticsQuery {
    days: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct UsageAnalyticsBucket {
    pub date: String,
    pub requests: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub latency_ms: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct UsageAnalyticsModel {
    pub model: String,
    pub requests: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct UsageAnalyticsTotals {
    pub requests: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub latency_ms: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct UsageAnalyticsResponse {
    pub schema_version: u8,
    pub days: i64,
    pub buckets: Vec<UsageAnalyticsBucket>,
    pub totals: UsageAnalyticsTotals,
    pub models: Vec<UsageAnalyticsModel>,
}

fn validate_usage_analytics_days(days: Option<i64>) -> Result<i64, AdminError> {
    let days = days.unwrap_or(7);
    if !(1..=30).contains(&days) {
        return Err(AdminError::bad_request("days must be between 1 and 30"));
    }
    Ok(days)
}

pub(crate) async fn my_usage_analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageAnalyticsQuery>,
) -> Result<Json<UsageAnalyticsResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let days = validate_usage_analytics_days(q.days)?;
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);
    let user_filter = Some(session.user_id.as_str());
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;

    let bucket_rows = ch
        .query_daily_usage_stats(&since, user_filter, offset)
        .await
        .map_err(AdminError::internal)?;
    let bucket_by_date: HashMap<_, _> = bucket_rows
        .into_iter()
        .map(
            |(
                date,
                requests,
                input_tokens,
                output_tokens,
                total_tokens,
                succeeded,
                latency_ms,
                cache_read_tokens,
            )| {
                (
                    date.clone(),
                    UsageAnalyticsBucket {
                        date,
                        requests,
                        succeeded,
                        failed: requests.saturating_sub(succeeded),
                        input_tokens,
                        cache_read_tokens,
                        output_tokens,
                        total_tokens,
                        latency_ms,
                    },
                )
            },
        )
        .collect();
    let local_today = (Utc::now() + ChronoDuration::seconds(offset)).date_naive();
    let buckets = (0..days)
        .rev()
        .map(|days_ago| {
            let date = local_today - ChronoDuration::days(days_ago);
            let date_key = date.format("%Y-%m-%d").to_string();
            bucket_by_date
                .get(&date_key)
                .cloned()
                .unwrap_or(UsageAnalyticsBucket {
                    date: date_key,
                    requests: 0,
                    succeeded: 0,
                    failed: 0,
                    input_tokens: 0,
                    cache_read_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    latency_ms: 0,
                })
        })
        .collect::<Vec<_>>();

    let models = ch
        .query_model_activity(&since, user_filter)
        .await
        .map_err(AdminError::internal)?
        .into_iter()
        .map(
            |(
                model,
                requests,
                input_tokens,
                output_tokens,
                succeeded,
                failed,
                cache_read_tokens,
            )| {
                UsageAnalyticsModel {
                    model,
                    requests,
                    succeeded,
                    failed,
                    input_tokens,
                    cache_read_tokens,
                    output_tokens,
                }
            },
        )
        .collect::<Vec<_>>();

    let totals = buckets.iter().fold(
        UsageAnalyticsTotals {
            requests: 0,
            succeeded: 0,
            failed: 0,
            input_tokens: 0,
            cache_read_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            latency_ms: 0,
        },
        |mut totals, bucket| {
            totals.requests += bucket.requests;
            totals.succeeded += bucket.succeeded;
            totals.failed += bucket.failed;
            totals.input_tokens += bucket.input_tokens;
            totals.cache_read_tokens += bucket.cache_read_tokens;
            totals.output_tokens += bucket.output_tokens;
            totals.total_tokens += bucket.total_tokens;
            totals.latency_ms += bucket.latency_ms;
            totals
        },
    );

    Ok(Json(UsageAnalyticsResponse {
        schema_version: 1,
        days,
        buckets,
        totals,
        models,
    }))
}

pub(crate) async fn my_usage_aggregate(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageAggregateQuery>,
) -> Result<Json<Vec<DailyAggregate>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let days = q.days.unwrap_or(14);
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);
    let user_filter: Option<&str> = Some(&session.user_id);

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let records: Vec<DailyAggregate> = ch
        .query_daily_usage_stats(&since, user_filter, offset)
        .await
        .map_err(AdminError::internal)?
        .into_iter()
        .map(
            |(date, count, pt, ct, tt, sc, lat, ch_tok)| DailyAggregate {
                date,
                count,
                prompt_tokens: pt,
                completion_tokens: ct,
                total_tokens: tt,
                success_count: sc,
                latency_ms: lat,
                cache_hit_tokens: ch_tok,
            },
        )
        .collect();

    Ok(Json(records))
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
    let days = q.days.unwrap_or(7);
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);
    let can_view_all = state.authz.enforce(&session.role, "admin:usage").await;
    let user_filter: Option<&str> = if can_view_all {
        q.user_id.as_deref()
    } else {
        Some(&session.user_id)
    };
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let records: Vec<ModelActivity> = ch
        .query_model_activity(&since, user_filter)
        .await
        .map_err(AdminError::internal)?
        .into_iter()
        .map(|(model, total, pt, ct, sc, fc, ch_tok)| ModelActivity {
            model,
            total_requests: total,
            prompt_tokens: pt,
            completion_tokens: ct,
            cache_hit_tokens: ch_tok,
            success_count: sc,
            failure_count: fc,
        })
        .collect();

    Ok(Json(records))
}

pub(crate) async fn my_model_activity(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageAggregateQuery>,
) -> Result<Json<Vec<ModelActivity>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let days = q.days.unwrap_or(7);
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);
    let user_filter: Option<&str> = Some(&session.user_id);
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let records: Vec<ModelActivity> = ch
        .query_model_activity(&since, user_filter)
        .await
        .map_err(AdminError::internal)?
        .into_iter()
        .map(|(model, total, pt, ct, sc, fc, ch_tok)| ModelActivity {
            model,
            total_requests: total,
            prompt_tokens: pt,
            completion_tokens: ct,
            cache_hit_tokens: ch_tok,
            success_count: sc,
            failure_count: fc,
        })
        .collect();

    Ok(Json(records))
}

#[derive(Serialize)]
pub(crate) struct FunnelResponse {
    pub total: u64,
    pub success_count: u64,
    pub auth_fail_count: u64,
    pub rate_limit_count: u64,
    pub bad_request_count: u64,
    pub upstream_error_count: u64,
    pub timeout_count: u64,
    pub other_error_count: u64,
    pub p50_latency: f64,
    pub p95_latency: f64,
    pub p99_latency: f64,
    pub avg_latency: f64,
}

pub(crate) async fn usage_funnel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageAggregateQuery>,
) -> Result<Json<FunnelResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let days = q.days.unwrap_or(7);
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);
    let can_view_all = state.authz.enforce(&session.role, "admin:usage").await;
    let user_filter: Option<&str> = if can_view_all {
        q.user_id.as_deref()
    } else {
        Some(&session.user_id)
    };
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let stats = ch
        .query_funnel_stats(&since, user_filter)
        .await
        .map_err(AdminError::internal)?;
    Ok(Json(FunnelResponse {
        total: stats.total,
        success_count: stats.success_count,
        auth_fail_count: stats.auth_fail_count,
        rate_limit_count: stats.rate_limit_count,
        bad_request_count: stats.bad_request_count,
        upstream_error_count: stats.upstream_error_count,
        timeout_count: stats.timeout_count,
        other_error_count: stats.other_error_count,
        p50_latency: stats.p50_latency,
        p95_latency: stats.p95_latency,
        p99_latency: stats.p99_latency,
        avg_latency: stats.avg_latency,
    }))
}

pub(crate) async fn my_usage_funnel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsageAggregateQuery>,
) -> Result<Json<FunnelResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let days = q.days.unwrap_or(7);
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since = since_local_days_ago(days, offset);
    let user_filter: Option<&str> = Some(&session.user_id);
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let stats = ch
        .query_funnel_stats(&since, user_filter)
        .await
        .map_err(AdminError::internal)?;
    Ok(Json(FunnelResponse {
        total: stats.total,
        success_count: stats.success_count,
        auth_fail_count: stats.auth_fail_count,
        rate_limit_count: stats.rate_limit_count,
        bad_request_count: stats.bad_request_count,
        upstream_error_count: stats.upstream_error_count,
        timeout_count: stats.timeout_count,
        other_error_count: stats.other_error_count,
        p50_latency: stats.p50_latency,
        p95_latency: stats.p95_latency,
        p99_latency: stats.p99_latency,
        avg_latency: stats.avg_latency,
    }))
}
