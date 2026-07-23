use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
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

pub(crate) async fn admin_dashboard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DashboardResp>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    if state.authz.enforce(&session.role, "admin:dashboard").await {
        let users = state.db.list_users().await.map_err(db_err)?;
        let channels = state.db.list_channels().await.map_err(db_err)?;
        let models = state.db.list_models().await.map_err(db_err)?;
        let rules = state.db.list_rules().await.map_err(db_err)?;

        let endpoint_count: usize = channels.iter().map(|c| c.endpoints.len()).sum();
        let total_requests = state.usage.count().await.unwrap_or(0);
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
        let api_keys = state.db.list_api_keys(&session.user_id).await.map_err(db_err)?;
        let user_requests = state.usage.count_by_user(&session.user_id).await.unwrap_or(0);

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
pub(crate) struct TopModel {
    model: String,
    count: u64,
    percentage: f64,
}

#[derive(Serialize)]
pub(crate) struct DashboardAggregations {
    total_requests: u64,
    total_cost: f64,
    requests_24h: u64,
    cost_24h: f64,
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
    let tz = state.db.get_user_timezone(&session.user_id).await.map_err(db_err)?;
    let offset = tz_offset_seconds(Some(&tz));
    let since_24h = since_local_days_ago(1, offset);

    let user_filter: Option<&str> = if state.authz.enforce(&session.role, "admin:dashboard").await {
        None
    } else {
        Some(&session.user_id)
    };

    // Load model pricing map once
    let models = state.db.list_models().await.unwrap_or_default();
    let mut pricing: std::collections::HashMap<String, (f64, f64, f64)> =
        std::collections::HashMap::new();
    for m in &models {
        pricing.insert(
            m.name.clone(),
            (m.pricing.prompt_price, m.pricing.completion_price, m.pricing.cache_read_price),
        );
        pricing.insert(
            m.model_pattern.clone(),
            (m.pricing.prompt_price, m.pricing.completion_price, m.pricing.cache_read_price),
        );
    }

    // Build sorted prefix list for glob pattern matching (O(log n) per lookup)
    let mut prefix_prices: Vec<(&str, (f64, f64, f64))> = pricing
        .iter()
        .filter_map(|(k, v)| k.strip_suffix('*').map(|p| (p, *v)))
        .collect();
    prefix_prices.sort_by_key(|b| std::cmp::Reverse(b.0.len())); // most specific first

    fn lookup_price<'a>(
        model_name: &str,
        pricing: &'a std::collections::HashMap<String, (f64, f64, f64)>,
        prefix_prices: &'a [(&str, (f64, f64, f64))],
    ) -> (f64, f64, f64) {
        if let Some(price) = pricing.get(model_name) {
            return *price;
        }
        for (prefix, price) in prefix_prices {
            if model_name.starts_with(prefix) {
                return *price;
            }
        }
        (0.0, 0.0, 0.0)
    }

    // All-time totals: use COUNT SQL aggregate instead of loading all rows
    let total_requests = match user_filter {
        Some(uid) => state.usage.count_by_user(uid).await.unwrap_or(0),
        None => state.usage.count().await.unwrap_or(0),
    } as u64;

    // 24h stats: use SQL aggregates
    let (requests_24h, success_count, total_latency, total_tokens_24h) = state
        .usage
        .stats_since(&since_24h, user_filter)
        .await
        .unwrap_or((0, 0, 0, 0));


    if requests_24h == 0 {
        return Ok(Json(DashboardAggregations {
            total_requests,
            total_cost: 0.0,
            requests_24h: 0,
            cost_24h: 0.0,
            success_rate_24h: 0.0,
            avg_latency_ms_24h: 0.0,
            total_tokens_24h: 0,
            top_models_24h: vec![],
        }));
    }

    // Compute cost from 24h records (loads only token + model columns)
    let records = state
        .usage
        .cost_rows_since(&since_24h, user_filter)
        .await.map_err(AdminError::internal)?;
    let mut total_cost_24h = 0.0_f64;
    let mut model_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for r in &records {
        let (pp, cp, crp) = if r.prompt_price > 0.0 || r.completion_price > 0.0 {
            (r.prompt_price, r.completion_price, r.cache_read_price)
        } else {
            lookup_price(&r.model, &pricing, &prefix_prices)
        };
        let cost = (r.prompt_tokens as f64 / 1000000.0 * pp)
            + (r.completion_tokens as f64 / 1000000.0 * cp)
            + (r.cache_hit_input_tokens as f64 / 1000000.0 * crp);
        total_cost_24h += cost;
        *model_counts.entry(r.model.clone()).or_default() += 1;
    }

    // All-time cost: load records with stored pricing
    let all_records = state
        .usage
        .cost_rows_since("1970-01-01T00:00:00", user_filter)
        .await.map_err(AdminError::internal)?;
    let total_cost: f64 = all_records
        .iter()
        .map(|r| {
            let (pp, cp, crp) = if r.prompt_price > 0.0 || r.completion_price > 0.0 {
                (r.prompt_price, r.completion_price, r.cache_read_price)
            } else {
                lookup_price(&r.model, &pricing, &prefix_prices)
            };
            (r.prompt_tokens as f64 / 1000000.0 * pp)
                + (r.completion_tokens as f64 / 1000000.0 * cp)
                + (r.cache_hit_input_tokens as f64 / 1000000.0 * crp)
        })
        .sum();

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
    top_models.sort_by(|a, b| b.count.cmp(&a.count));
    top_models.truncate(10);

    Ok(Json(DashboardAggregations {
        total_requests,
        total_cost: (total_cost * 100.0).round() / 100.0,
        requests_24h,
        cost_24h: (total_cost_24h * 100.0).round() / 100.0,
        success_rate_24h: (success_rate * 100.0).round() / 100.0,
        avg_latency_ms_24h: (avg_latency * 100.0).round() / 100.0,
        total_tokens_24h,
        top_models_24h: top_models,
    }))
}
