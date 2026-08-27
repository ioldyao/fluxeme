use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
use chrono::Timelike;
use serde::{Deserialize, Serialize};

use crate::server::AppState;

use super::*;

// ── Health Routing Dashboard ──────────────────────────────────────

pub(crate) async fn routing_health(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:dashboard").await?;

    let models = state.db.list_published_models().await.map_err(db_err)?;
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let published_model_names: Vec<String> =
        models.iter().map(|model| model.name.clone()).collect();
    let usage = ch
        .query_channel_usage_24h(&published_model_names)
        .await
        .map_err(AdminError::internal)?;

    let mut usage_map: std::collections::HashMap<(String, String), (u64, u64, f64, f64)> =
        std::collections::HashMap::new();
    for (ch, md, req, suc, avg, p95) in &usage {
        usage_map.insert((ch.clone(), md.clone()), (*req, *suc, *avg, *p95));
    }

    let mut model_results = Vec::new();
    let mut total_requests_24h: u64 = 0;
    let mut total_success: u64 = 0;
    let mut active_channels = std::collections::HashSet::new();
    let mut broken_channels = std::collections::HashSet::new();

    for m in &models {
        let mut ch_results = Vec::new();
        let mut model_total: u64 = 0;

        for binding in &m.channels {
            let key = (binding.channel_id.clone(), m.name.clone());
            let (req, suc, avg, p95) = usage_map.get(&key).copied().unwrap_or((0, 0, 0.0, 0.0));
            if req > 0 {
                model_total += req;
            }

            let health = state.routing.channel_health(&binding.channel_id);
            let circuit_ok = health
                .iter()
                .any(|(_, enabled, available)| *enabled && *available);
            let any_enabled = health.iter().any(|(_, enabled, _)| *enabled);
            let circuit_enabled = any_enabled || health.is_empty();

            if req > 0 || any_enabled {
                let configured_channel = state.routing.get_channel(&binding.channel_id);
                let channel_enabled = configured_channel
                    .as_ref()
                    .is_some_and(|channel| channel.enabled);
                let ch_name = configured_channel
                    .map(|channel| {
                        if channel.name.is_empty() {
                            channel.id
                        } else {
                            channel.name
                        }
                    })
                    .unwrap_or_else(|| binding.channel_id.clone());

                if req > 0 {
                    total_requests_24h += req;
                    total_success += suc;
                    active_channels.insert(binding.channel_id.clone());
                    if !circuit_ok && circuit_enabled {
                        broken_channels.insert(binding.channel_id.clone());
                    }
                }

                let endpoints: Vec<serde_json::Value> = health
                    .iter()
                    .map(|(eid, enabled, available)| {
                        serde_json::json!({
                            "endpoint_id": eid,
                            "enabled": enabled,
                            "available": available,
                        })
                    })
                    .collect();

                ch_results.push(serde_json::json!({
                    "channel_id": binding.channel_id,
                    "channel_name": ch_name,
                    "enabled": channel_enabled,
                    "priority": binding.priority,
                    "provider": binding.provider,
                    "requests": req,
                    "success_rate": if req > 0 { suc as f64 / req as f64 } else { 0.0 },
                    "avg_latency_ms": avg,
                    "p95_latency_ms": p95,
                    "circuit_ok": circuit_ok,
                    "circuit_enabled": circuit_enabled,
                    "endpoints": endpoints,
                }));
            }
        }

        if !ch_results.is_empty() {
            model_results.push(serde_json::json!({
                "id": m.id,
                "name": m.name,
                "model_pattern": m.model_pattern,
                "category": m.category,
                "total_requests": model_total,
                "channels": ch_results,
            }));
        }
    }

    let overall_rate = if total_requests_24h > 0 {
        total_success as f64 / total_requests_24h as f64
    } else {
        0.0
    };

    Ok(Json(serde_json::json!({
        "models": model_results,
        "summary": {
            "total_requests_24h": total_requests_24h,
            "overall_success_rate": overall_rate,
            "active_channels": active_channels.len(),
            "broken_channels": broken_channels.len(),
        },
    })))
}

// ── Recent Request Paths ──────────────────────────────────────────

pub(crate) async fn recent_request_paths(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:dashboard").await?;

    let published_model_names = state
        .db
        .list_published_models()
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|model| model.name)
        .collect::<Vec<_>>();
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let paths: Vec<serde_json::Value> = ch
        .query_recent_request_paths(15, &published_model_names)
        .await
        .map_err(AdminError::internal)?
        .into_iter()
        .map(|(ts, m, ch, eid, eurl, lat, suc)| {
            serde_json::json!({
                "timestamp": ts,
                "model": m,
                "channel_id": ch,
                "endpoint_id": eid,
                "endpoint_url": eurl,
                "latency_ms": lat,
                "success": suc,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "paths": paths })))
}

// ── Routing Flow History ──────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct RoutingHistoryQuery {
    start: String,
    end: String,
    model: Option<String>,
}

fn routing_history_bucket_unit(start: &str, end: &str) -> Result<&'static str, AdminError> {
    let start_dt = chrono::DateTime::parse_from_rfc3339(start)
        .map_err(|_| AdminError::bad_request("Invalid start datetime"))?;
    let end_dt = chrono::DateTime::parse_from_rfc3339(end)
        .map_err(|_| AdminError::bad_request("Invalid end datetime"))?;
    if end_dt <= start_dt {
        return Err(AdminError::bad_request("end must be after start"));
    }
    Ok(if (end_dt - start_dt).num_seconds() < 172_800 {
        "hour"
    } else {
        "day"
    })
}

fn routing_history_bucket_axis(
    start: &str,
    end: &str,
    bucket_unit: &str,
) -> Result<Vec<String>, AdminError> {
    let start_dt = chrono::DateTime::parse_from_rfc3339(start)
        .map_err(|_| AdminError::bad_request("Invalid start datetime"))?
        .with_timezone(&chrono::Utc);
    let end_dt = chrono::DateTime::parse_from_rfc3339(end)
        .map_err(|_| AdminError::bad_request("Invalid end datetime"))?
        .with_timezone(&chrono::Utc);
    let mut cursor = if bucket_unit == "day" {
        start_dt
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
    } else {
        start_dt
            .date_naive()
            .and_hms_opt(start_dt.hour(), 0, 0)
            .unwrap()
            .and_utc()
    };
    let step = if bucket_unit == "day" {
        chrono::Duration::days(1)
    } else {
        chrono::Duration::hours(1)
    };
    let mut axis = Vec::new();
    while cursor <= end_dt {
        axis.push(cursor.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        cursor += step;
    }
    Ok(axis)
}

#[derive(Serialize)]
pub(crate) struct RoutingHistoryResponse {
    schema_version: u8,
    timezone: &'static str,
    bucket_unit: &'static str,
    buckets: Vec<String>,
    series: std::collections::HashMap<String, ChannelSeries>,
    totals: RoutingHistoryTotals,
    summary: Vec<ChannelSummary>,
}

#[derive(Serialize)]
pub(crate) struct RoutingHistoryTotals {
    requests: u64,
    successes: u64,
    success_rate_percent: Option<f64>,
    avg_latency_ms: Option<f64>,
    p95_latency_ms: Option<f64>,
    unattributed_requests: u64,
}

#[derive(Serialize)]
pub(crate) struct ChannelSeries {
    channel_name: String,
    requests: Vec<u64>,
    successes: Vec<u64>,
    success_rate_percent: Vec<Option<f64>>,
}

#[derive(Serialize)]
pub(crate) struct ChannelSummary {
    channel_id: String,
    requests: u64,
    successes: u64,
    success_rate_percent: Option<f64>,
    avg_latency_ms: Option<f64>,
    p95_latency_ms: Option<f64>,
    endpoints: Vec<EndptDetail>,
}

#[derive(Serialize)]
pub(crate) struct EndptDetail {
    endpoint_id: Option<i64>,
    url: Option<String>,
    url_status: &'static str,
    url_variant_count: u64,
    requests: u64,
    successes: u64,
    success_rate_percent: Option<f64>,
    avg_latency_ms: Option<f64>,
    p95_latency_ms: Option<f64>,
}

pub(crate) async fn routing_flow_snapshot_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<(String, String, Option<i64>, u64)>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:dashboard").await?;
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let published_model_names = state
        .db
        .list_published_models()
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|model| model.name)
        .collect::<Vec<_>>();
    ch.query_routing_flow_snapshot(24, &published_model_names)
        .await
        .map(Json)
        .map_err(AdminError::internal)
}

pub(crate) async fn routing_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<RoutingHistoryQuery>,
) -> Result<Json<RoutingHistoryResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:dashboard").await?;

    let requested_model: Option<&str> = q.model.as_deref().filter(|m| !m.is_empty() && *m != "all");
    let bucket_unit = routing_history_bucket_unit(&q.start, &q.end)?;
    let bucket_axis = routing_history_bucket_axis(&q.start, &q.end, bucket_unit)?;
    let published_model_names = state
        .db
        .list_published_models()
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|model| model.name)
        .collect::<Vec<_>>();

    tracing::info!(start = %q.start, end = %q.end, model = ?requested_model, bucket_unit, "routing_history query");

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let buckets = ch
        .query_routing_history_buckets_filtered(
            &q.start,
            &q.end,
            requested_model,
            &published_model_names,
            bucket_unit,
        )
        .await
        .map_err(|e| AdminError::internal(format!("routing history buckets: {e}")))?;
    let stats = ch
        .query_routing_history_stats_filtered(
            &q.start,
            &q.end,
            requested_model,
            &published_model_names,
        )
        .await
        .map_err(|e| AdminError::internal(format!("routing history stats: {e}")))?;
    let overall = ch
        .query_routing_history_overall_stats_filtered(
            &q.start,
            &q.end,
            requested_model,
            &published_model_names,
        )
        .await
        .map_err(|e| AdminError::internal(format!("routing history overall stats: {e}")))?;
    let details = ch
        .query_routing_history_endpoint_details(
            &q.start,
            &q.end,
            requested_model,
            &published_model_names,
        )
        .await
        .map_err(|e| AdminError::internal(format!("routing history endpoint details: {e}")))?;

    let mut points: std::collections::HashMap<
        String,
        std::collections::HashMap<String, (u64, u64)>,
    > = std::collections::HashMap::new();
    for bucket in buckets {
        let entry = points.entry(bucket.channel_id).or_default();
        let point = entry.entry(bucket.bucket).or_default();
        point.0 = point.0.saturating_add(bucket.requests);
        point.1 = point.1.saturating_add(bucket.successes);
    }

    let mut channel_ids: Vec<String> = stats.iter().map(|row| row.channel_id.clone()).collect();
    channel_ids.sort();
    channel_ids.dedup();
    let mut series = std::collections::HashMap::new();
    for channel_id in &channel_ids {
        let channel_name = state
            .routing
            .get_channel(channel_id)
            .map(|channel| channel.name)
            .unwrap_or_else(|| channel_id.clone());
        let channel_points = points.get(channel_id);
        let requests: Vec<u64> = bucket_axis
            .iter()
            .map(|bucket| {
                channel_points
                    .and_then(|p| p.get(bucket))
                    .map(|v| v.0)
                    .unwrap_or(0)
            })
            .collect();
        let successes: Vec<u64> = bucket_axis
            .iter()
            .map(|bucket| {
                channel_points
                    .and_then(|p| p.get(bucket))
                    .map(|v| v.1)
                    .unwrap_or(0)
            })
            .collect();
        let success_rate_percent = requests
            .iter()
            .zip(&successes)
            .map(|(request_count, success_count)| {
                (*request_count > 0)
                    .then(|| (*success_count as f64 / *request_count as f64) * 100.0)
            })
            .collect();
        series.insert(
            channel_id.clone(),
            ChannelSeries {
                channel_name,
                requests,
                successes,
                success_rate_percent,
            },
        );
    }

    let mut ep_by_channel: std::collections::HashMap<String, Vec<EndptDetail>> =
        std::collections::HashMap::new();
    for (channel_id, endpoint_id, url, url_variant_count, requests, successes, avg, p95) in details
    {
        let success_rate_percent =
            (requests > 0).then(|| (successes as f64 / requests as f64) * 100.0);
        ep_by_channel
            .entry(channel_id)
            .or_default()
            .push(EndptDetail {
                endpoint_id,
                url: url.clone(),
                url_status: if url_variant_count == 0 {
                    "missing"
                } else if url_variant_count == 1 {
                    "stable"
                } else {
                    "varied"
                },
                url_variant_count,
                requests,
                successes,
                success_rate_percent,
                avg_latency_ms: (requests > 0).then_some(avg),
                p95_latency_ms: (requests > 0).then_some(p95),
            });
    }
    for endpoints in ep_by_channel.values_mut() {
        endpoints.sort_by_key(|endpoint| (endpoint.endpoint_id.is_none(), endpoint.endpoint_id));
    }

    let summary: Vec<ChannelSummary> = stats
        .into_iter()
        .map(|stat| ChannelSummary {
            channel_id: stat.channel_id.clone(),
            requests: stat.requests,
            successes: stat.successes,
            success_rate_percent: (stat.requests > 0)
                .then(|| (stat.successes as f64 / stat.requests as f64) * 100.0),
            avg_latency_ms: (stat.requests > 0).then_some(stat.avg_latency),
            p95_latency_ms: (stat.requests > 0).then_some(stat.p95_latency),
            endpoints: ep_by_channel.remove(&stat.channel_id).unwrap_or_default(),
        })
        .collect();
    let (requests, successes, avg_latency, p95_latency) = overall;
    let avg_latency_ms = (requests > 0).then_some(avg_latency);
    let p95_latency_ms = (requests > 0).then_some(p95_latency);

    Ok(Json(RoutingHistoryResponse {
        schema_version: 2,
        timezone: "UTC",
        bucket_unit,
        buckets: bucket_axis,
        series,
        totals: RoutingHistoryTotals {
            requests,
            successes,
            success_rate_percent: (requests > 0)
                .then(|| (successes as f64 / requests as f64) * 100.0),
            avg_latency_ms,
            p95_latency_ms,
            unattributed_requests: summary
                .iter()
                .flat_map(|row| row.endpoints.iter())
                .filter(|endpoint| endpoint.endpoint_id.is_none())
                .map(|endpoint| endpoint.requests)
                .sum(),
        },
        summary,
    }))
}

#[derive(Deserialize)]
pub(crate) struct FlowMetricsQuery {
    start: Option<String>,
    end: Option<String>,
    model: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FlowMetricsPercentiles {
    pub(crate) p50: Option<f64>,
    pub(crate) p90: Option<f64>,
    pub(crate) p99: Option<f64>,
    pub(crate) sample_count: u64,
}

#[derive(Serialize)]
pub(crate) struct FlowMetricsModelShare {
    pub(crate) model: String,
    pub(crate) requests: u64,
    pub(crate) share: f64,
}

#[derive(Serialize)]
pub(crate) struct FlowMetricsClientIp {
    pub(crate) ip: String,
    pub(crate) requests: u64,
}

#[derive(Serialize)]
pub(crate) struct FlowMetricsTrend {
    pub(crate) bucket_unit: &'static str,
    pub(crate) buckets: Vec<String>,
    pub(crate) success_completed: Vec<u64>,
    pub(crate) failed_completed: Vec<u64>,
}

#[derive(Serialize)]
pub(crate) struct FlowMetricsHistorical {
    pub(crate) total_completed: u64,
    pub(crate) success_completed: u64,
    pub(crate) failed_completed: u64,
    pub(crate) model_share: Vec<FlowMetricsModelShare>,
    pub(crate) client_ips: Vec<FlowMetricsClientIp>,
    pub(crate) latency_ms: FlowMetricsPercentiles,
    pub(crate) ttft_ms: FlowMetricsPercentiles,
    pub(crate) trend: FlowMetricsTrend,
}

#[derive(Serialize)]
pub(crate) struct FlowMetricsRealtimeQueue {
    status: &'static str,
    count: Option<u64>,
    reason: &'static str,
}

#[derive(Serialize)]
pub(crate) struct FlowMetricsRealtime {
    as_of: String,
    in_flight: u64,
    upstream_generating: u64,
    upstream_outputting: u64,
    queue: FlowMetricsRealtimeQueue,
    consistency: &'static str,
    source: &'static str,
}

#[derive(Serialize)]
pub(crate) struct FlowMetricsRange {
    start: String,
    end: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FlowMetricsResponse {
    schema_version: u8,
    range: FlowMetricsRange,
    historical: FlowMetricsHistorical,
    realtime: FlowMetricsRealtime,
}

pub(crate) async fn flow_metrics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<FlowMetricsQuery>,
) -> Result<Json<FlowMetricsResponse>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:dashboard").await?;

    let start = q
        .start
        .unwrap_or_else(|| (chrono::Utc::now() - chrono::Duration::hours(24)).to_rfc3339());
    let end = q.end.unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let model = q.model.as_deref().filter(|m| !m.is_empty() && *m != "all");

    let start_dt = chrono::DateTime::parse_from_rfc3339(&start)
        .map_err(|_| AdminError::bad_request("Invalid start datetime"))?;
    let end_dt = chrono::DateTime::parse_from_rfc3339(&end)
        .map_err(|_| AdminError::bad_request("Invalid end datetime"))?;
    if end_dt <= start_dt {
        return Err(AdminError::bad_request("end must be after start"));
    }

    let published_model_names = state
        .db
        .list_published_models()
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|model| model.name)
        .collect::<Vec<_>>();
    let model = model.filter(|name| {
        published_model_names
            .iter()
            .any(|published| published == name)
    });
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let historical = ch
        .query_flow_metrics(&start, &end, model, &published_model_names)
        .await
        .map_err(AdminError::internal)?;

    Ok(Json(FlowMetricsResponse {
        schema_version: 1,
        range: FlowMetricsRange {
            start,
            end,
            model: model.map(str::to_string),
        },
        historical,
        realtime: {
            let snapshot = state
                .flow_tracker
                .snapshot_global()
                .await
                .map_err(AdminError::internal)?;
            FlowMetricsRealtime {
                as_of: snapshot.as_of,
                in_flight: snapshot.in_flight,
                upstream_generating: snapshot.upstream_generating,
                upstream_outputting: snapshot.upstream_outputting,
                queue: FlowMetricsRealtimeQueue {
                    status: "unavailable",
                    count: None,
                    reason: "admission_not_enabled",
                },
                consistency: "eventually_consistent",
                source: "redis_flow_tracker",
            }
        },
    }))
}
