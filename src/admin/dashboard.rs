use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::server::AppState;

use super::*;

// ── Dashboard ─────────────────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct DashboardResp {
    users: usize,
    channels: usize,
    models: usize,
    rules: usize,
    api_keys: usize,
    endpoints: usize,
    total_requests: usize,
}

// Importers/callers: exposed from src/admin/mod.rs as GET /api/dashboard and
// consumed by admin observability views like ui/src/pages/FlowTowerContent.tsx.
// Affected API/data schema: DashboardResp { users, channels, models, rules,
// api_keys, endpoints, total_requests }. User instruction: "`网关运行总览`
// 这个前端页面中，哪些还有计算全部用户的，统一修改只看当前个人用户的
// 数据,admin登陆也只看自己的数据". This route remains the shared admin/global
// summary; the personal dashboard uses the dedicated /api/dashboard/self routes
// below so other admin pages keep their existing overview behavior.
pub(crate) async fn admin_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DashboardResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    if state.authz.enforce(&session.role, "admin:dashboard").await {
        let users = state.db.list_users(Some("active")).await.map_err(db_err)?;
        let channels = state.db.list_channels().await.map_err(db_err)?;
        let models = state.db.list_models().await.map_err(db_err)?;
        let rules = state.db.list_rules().await.map_err(db_err)?;

        let endpoint_count: usize = channels.iter().map(|c| c.endpoints.len()).sum();
        let ch = state
            .ch
            .as_ref()
            .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
        let total_requests = ch
            .count_usage(&crate::domain::usage::UsageFilter::default())
            .await
            .map_err(AdminError::internal)?;
        let api_key_count = state.db.all_api_keys().await.map(|k| k.len()).unwrap_or(0);

        Ok(Json(DashboardResp {
            users: users.len(),
            channels: channels.len(),
            models: models.len(),
            rules: rules.len(),
            api_keys: api_key_count,
            endpoints: endpoint_count,
            total_requests,
        }))
    } else {
        let api_keys = state
            .db
            .list_api_keys(&session.user_id)
            .await
            .map_err(db_err)?;
        let ch = state
            .ch
            .as_ref()
            .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
        let user_requests = ch
            .count_usage(&crate::domain::usage::UsageFilter {
                user_id: Some(session.user_id.clone()),
                ..Default::default()
            })
            .await
            .map_err(AdminError::internal)?;

        Ok(Json(DashboardResp {
            users: 0,
            channels: 0,
            models: 0,
            rules: 0,
            api_keys: api_keys.len(),
            endpoints: 0,
            total_requests: user_requests,
        }))
    }
}

#[derive(Serialize)]
pub(crate) struct SelfDashboardResp {
    api_keys: usize,
    total_requests: usize,
}

// Importers/callers: exposed from src/admin/mod.rs as GET /api/dashboard/self
// and consumed only by ui/src/pages/Dashboard.tsx via ui/src/api/dashboard.ts.
// Affected API/data schema: SelfDashboardResp { api_keys, total_requests }.
// User instruction: "`网关运行总览` 这个前端页面中，哪些还有计算全部用户的，
// 统一修改只看当前个人用户的数据,admin登陆也只看自己的数据".
pub(crate) async fn self_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<SelfDashboardResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    let api_keys = state
        .db
        .list_api_keys(&session.user_id)
        .await
        .map_err(db_err)?;
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let user_requests = ch
        .count_usage(&crate::domain::usage::UsageFilter {
            user_id: Some(session.user_id.clone()),
            ..Default::default()
        })
        .await
        .map_err(AdminError::internal)?;

    Ok(Json(SelfDashboardResp {
        api_keys: api_keys.len(),
        total_requests: user_requests,
    }))
}

#[derive(Serialize)]
pub(crate) struct TopModel {
    model: String,
    count: u64,
    percentage: f64,
}

#[derive(Serialize)]
pub(crate) struct DashboardAggregations {
    total_requests: u64,
    #[serde(with = "rust_decimal::serde::float")]
    total_cost: Decimal,
    requests_24h: u64,
    #[serde(with = "rust_decimal::serde::float")]
    cost_24h: Decimal,
    success_rate_24h: f64,
    avg_latency_ms_24h: f64,
    total_tokens_24h: u64,
    top_models_24h: Vec<TopModel>,
}

pub(crate) async fn dashboard_aggregations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DashboardAggregations>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since_24h = since_local_days_ago(1, offset);

    let user_filter: Option<&str> = if state.authz.enforce(&session.role, "admin:dashboard").await {
        None
    } else {
        Some(&session.user_id)
    };

    // All-time totals remain on PostgreSQL billing metadata because ClickHouse
    // data is TTL-limited. This does not read the PostgreSQL observability API.
    let billing_months = if let Some(uid) = user_filter {
        state
            .db
            .period_summary_for_user(uid)
            .await
            .map_err(db_err)?
    } else {
        state.db.period_summary_all().await.map_err(db_err)?
    };
    let total_requests = billing_months
        .iter()
        .map(|(_, _, requests, _)| *requests)
        .sum();
    let total_cost = billing_months
        .iter()
        .map(|(_, cost, _, _)| *cost)
        .fold(Decimal::ZERO, |acc, cost| acc + cost);

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let (requests_24h, success_count, total_latency, total_tokens_24h) = ch
        .query_usage_stats_since(&since_24h, user_filter)
        .await
        .map_err(AdminError::internal)?;

    if requests_24h == 0 {
        return Ok(Json(DashboardAggregations {
            total_requests,
            total_cost: Decimal::ZERO,
            requests_24h: 0,
            cost_24h: Decimal::ZERO,
            success_rate_24h: 0.0,
            avg_latency_ms_24h: 0.0,
            total_tokens_24h: 0,
            top_models_24h: vec![],
        }));
    }

    // Cost is a PostgreSQL billing fact. ClickHouse supplies only telemetry;
    // never re-price historical or package-backed requests with current model prices.
    let total_cost_24h = state
        .db
        .period_summary_since(&since_24h, user_filter)
        .await
        .map_err(db_err)?;
    let records = ch
        .query_usage_since(&since_24h, user_filter)
        .await
        .map_err(AdminError::internal)?;
    let mut model_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for r in &records {
        *model_counts.entry(r.model.clone()).or_default() += 1;
    }

    let success_rate = if requests_24h > 0 {
        success_count as f64 / requests_24h as f64 * 100.0
    } else {
        0.0
    };
    let avg_latency = if requests_24h > 0 {
        total_latency as f64 / requests_24h as f64
    } else {
        0.0
    };

    let mut top_models: Vec<TopModel> = model_counts
        .into_iter()
        .map(|(model, count)| TopModel {
            percentage: (count as f64 / requests_24h as f64 * 100.0 * 100.0).round() / 100.0,
            count,
            model,
        })
        .collect();
    top_models.sort_by_key(|model| std::cmp::Reverse(model.count));
    top_models.truncate(10);

    Ok(Json(DashboardAggregations {
        total_requests,
        total_cost,
        requests_24h,
        cost_24h: total_cost_24h,
        success_rate_24h: (success_rate * 100.0).round() / 100.0,
        avg_latency_ms_24h: (avg_latency * 100.0).round() / 100.0,
        total_tokens_24h,
        top_models_24h: top_models,
    }))
}

// Importers/callers: exposed from src/admin/mod.rs as GET
// /api/dashboard/self/aggregations and consumed only by ui/src/pages/Dashboard.tsx
// via ui/src/api/dashboard.ts. Affected API/data schema: DashboardAggregations.
// User instruction: "`网关运行总览` 这个前端页面中，哪些还有计算全部用户的，
// 统一修改只看当前个人用户的数据,admin登陆也只看自己的数据".
pub(crate) async fn self_dashboard_aggregations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DashboardAggregations>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let tz = state
        .db
        .get_user_timezone(&session.user_id)
        .await
        .map_err(|e| {
            tracing::error!(user_id = %session.user_id, error = %e.0, "Dashboard self: timezone lookup failed");
            db_err(e)
        })?;
    let offset = tz_offset_seconds(Some(&tz));
    let since_24h = since_local_days_ago(1, offset);
    let user_filter: Option<&str> = Some(&session.user_id);

    let billing_months = state
        .db
        .period_summary_for_user(&session.user_id)
        .await
        .map_err(|e| {
            tracing::error!(user_id = %session.user_id, error = %e.0, "Dashboard self: billing summary failed");
            db_err(e)
        })?;
    let total_requests = billing_months
        .iter()
        .map(|(_, _, requests, _)| *requests)
        .sum();
    let total_cost = billing_months
        .iter()
        .map(|(_, cost, _, _)| *cost)
        .fold(Decimal::ZERO, |acc, cost| acc + cost);

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let (requests_24h, success_count, total_latency, total_tokens_24h) = ch
        .query_usage_stats_since(&since_24h, user_filter)
        .await
        .map_err(|e| {
            tracing::error!(user_id = %session.user_id, since = %since_24h, error = %e, "Dashboard self: 24h usage stats failed");
            AdminError::internal(e)
        })?;

    if requests_24h == 0 {
        return Ok(Json(DashboardAggregations {
            total_requests,
            total_cost,
            requests_24h: 0,
            cost_24h: Decimal::ZERO,
            success_rate_24h: 0.0,
            avg_latency_ms_24h: 0.0,
            total_tokens_24h: 0,
            top_models_24h: vec![],
        }));
    }

    let total_cost_24h = state
        .db
        .period_summary_since(&since_24h, user_filter)
        .await
        .map_err(|e| {
            tracing::error!(user_id = %session.user_id, since = %since_24h, error = %e.0, "Dashboard self: 24h billing summary failed");
            db_err(e)
        })?;
    let records = ch
        .query_usage_since(&since_24h, user_filter)
        .await
        .map_err(|e| {
            tracing::error!(user_id = %session.user_id, since = %since_24h, error = %e, "Dashboard self: 24h usage rows failed");
            AdminError::internal(e)
        })?;
    let mut model_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for r in &records {
        *model_counts.entry(r.model.clone()).or_default() += 1;
    }

    let success_rate = if requests_24h > 0 {
        success_count as f64 / requests_24h as f64 * 100.0
    } else {
        0.0
    };
    let avg_latency = if requests_24h > 0 {
        total_latency as f64 / requests_24h as f64
    } else {
        0.0
    };

    let mut top_models: Vec<TopModel> = model_counts
        .into_iter()
        .map(|(model, count)| TopModel {
            percentage: (count as f64 / requests_24h as f64 * 100.0 * 100.0).round() / 100.0,
            count,
            model,
        })
        .collect();
    top_models.sort_by_key(|model| std::cmp::Reverse(model.count));
    top_models.truncate(10);

    Ok(Json(DashboardAggregations {
        total_requests,
        total_cost,
        requests_24h,
        cost_24h: total_cost_24h,
        success_rate_24h: (success_rate * 100.0).round() / 100.0,
        avg_latency_ms_24h: (avg_latency * 100.0).round() / 100.0,
        total_tokens_24h,
        top_models_24h: top_models,
    }))
}
