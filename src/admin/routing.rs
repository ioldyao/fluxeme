use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::Json;
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

    let models = state.db.list_models().await.map_err(db_err)?;
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let usage = ch
        .query_channel_usage_24h()
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

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let paths: Vec<serde_json::Value> = ch
        .query_recent_request_paths(15)
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

#[derive(Serialize)]
pub(crate) struct RoutingHistoryResponse {
    buckets: Vec<String>,
    series: std::collections::HashMap<String, ChannelSeries>,
    summary: Vec<ChannelSummary>,
}

#[derive(Serialize)]
pub(crate) struct ChannelSeries {
    channel_name: String,
    volume: Vec<u64>,
    success_rate: Vec<f64>,
}

#[derive(Serialize)]
pub(crate) struct ChannelSummary {
    channel_id: String,
    requests: u64,
    success_rate: f64,
    avg_latency: f64,
    p95_latency: f64,
    endpoints: Vec<EndptDetail>,
}

#[derive(Serialize)]
pub(crate) struct EndptDetail {
    endpoint_id: Option<i64>,
    url: String,
    requests: u64,
    success_rate: f64,
    avg_latency: f64,
    p95_latency: f64,
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
    ch.query_routing_flow_snapshot(24)
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

    let model_filter: Option<&str> = q.model.as_deref().filter(|m| !m.is_empty() && *m != "all");

    tracing::info!(start = %q.start, end = %q.end, model = ?model_filter, "routing_history query");

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let buckets = ch
        .query_routing_history_buckets_filtered(&q.start, &q.end, model_filter)
        .await
        .map_err(|e| {
            tracing::error!(error = e, "routing_history_buckets (CH) failed");
            AdminError::internal(e)
        })?;

    let stats = ch
        .query_routing_history_stats_filtered(&q.start, &q.end, model_filter)
        .await
        .map_err(|e| {
            tracing::error!(error = e, "routing_history_stats (CH) failed");
            AdminError::internal(e)
        })?;

    let details = ch
        .query_routing_history_endpoint_details(&q.start, &q.end, model_filter)
        .await
        .map_err(|e| {
            tracing::error!(error = e, "routing_history_endpoint_details (CH) failed");
            AdminError::internal(e)
        })?;
    let mut ep_by_channel: std::collections::HashMap<String, Vec<EndptDetail>> =
        std::collections::HashMap::new();
    for (ch, eid, url, reqs, succs, avg, p95) in &details {
        let rate = if *reqs > 0 {
            (*succs as f64 / *reqs as f64) * 100.0
        } else {
            0.0
        };
        ep_by_channel
            .entry(ch.clone())
            .or_default()
            .push(EndptDetail {
                endpoint_id: *eid,
                url: url.clone().unwrap_or_default(),
                requests: *reqs,
                success_rate: (rate * 10.0).round() / 10.0,
                avg_latency: (avg * 10.0).round() / 10.0,
                p95_latency: (p95 * 10.0).round() / 10.0,
            });
    }

    // Build time-series: one series per channel
    let mut channel_map: std::collections::HashMap<String, Vec<(String, u64, u64)>> =
        std::collections::HashMap::new();
    let mut all_buckets: Vec<String> = Vec::new();
    for b in &buckets {
        if all_buckets.last() != Some(&b.bucket) {
            all_buckets.push(b.bucket.clone());
        }
        channel_map.entry(b.channel_id.clone()).or_default().push((
            b.bucket.clone(),
            b.requests,
            b.successes,
        ));
    }

    let mut series = std::collections::HashMap::new();
    for (ch_id, points) in &channel_map {
        let ch_name = state
            .routing
            .get_channel(ch_id)
            .map(|c| c.name)
            .unwrap_or_else(|| ch_id.clone());
        let volume: Vec<u64> = all_buckets
            .iter()
            .map(|bk| {
                points
                    .iter()
                    .find(|(b, _, _)| b == bk)
                    .map(|(_, v, _)| *v)
                    .unwrap_or(0)
            })
            .collect();
        let success_rate: Vec<f64> = all_buckets
            .iter()
            .map(|bk| {
                points
                    .iter()
                    .find(|(b, _, _)| b == bk)
                    .map(|(_, v, s)| {
                        if *v > 0 {
                            (*s as f64 / *v as f64) * 100.0
                        } else {
                            0.0
                        }
                    })
                    .unwrap_or(0.0)
            })
            .collect();
        series.insert(
            ch_id.clone(),
            ChannelSeries {
                channel_name: ch_name,
                volume,
                success_rate,
            },
        );
    }

    let summary: Vec<ChannelSummary> = stats
        .iter()
        .map(|s| {
            let rate = if s.requests > 0 {
                (s.successes as f64 / s.requests as f64) * 100.0
            } else {
                0.0
            };
            ChannelSummary {
                channel_id: s.channel_id.clone(),
                requests: s.requests,
                success_rate: (rate * 10.0).round() / 10.0,
                avg_latency: (s.avg_latency * 10.0).round() / 10.0,
                p95_latency: (s.p95_latency * 10.0).round() / 10.0,
                endpoints: ep_by_channel.remove(&s.channel_id).unwrap_or_default(),
            }
        })
        .collect();

    Ok(Json(RoutingHistoryResponse {
        buckets: all_buckets,
        series,
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

    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let historical = ch
        .query_flow_metrics(&start, &end, model)
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
            let snapshot = state.flow_tracker.snapshot();
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
                consistency: "approximate",
                source: "in_memory_flow_tracker",
            }
        },
    }))
}
