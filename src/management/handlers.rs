use std::sync::Arc;

use axum::extract::{Extension, State};
use axum::Json;
use serde::Serialize;

use crate::domain::channel::Channel;
use crate::server::AppState;

use super::auth::{ManagementError, ManagementPrincipal};

#[derive(Debug, Serialize)]
pub(crate) struct ManagementStatus {
    status: &'static str,
    instance_id: String,
    version: &'static str,
    checks: ManagementChecks,
}

#[derive(Debug, Serialize)]
pub(crate) struct ManagementChecks {
    postgres: bool,
    redis: bool,
    clickhouse: bool,
}

pub(crate) async fn status(
    State(state): State<Arc<AppState>>,
    Extension(_principal): Extension<ManagementPrincipal>,
) -> Result<Json<ManagementStatus>, ManagementError> {
    let postgres = tokio::time::timeout(std::time::Duration::from_secs(2), state.db.ping())
        .await
        .is_ok_and(|result| result.is_ok());
    let redis = tokio::time::timeout(std::time::Duration::from_secs(2), state.cache.ping())
        .await
        .is_ok_and(|result| result.is_ok());
    let clickhouse = match state.ch.as_ref() {
        Some(ch) => tokio::time::timeout(std::time::Duration::from_secs(2), ch.ping())
            .await
            .unwrap_or(false),
        None => false,
    };
    let healthy = postgres && redis && clickhouse;
    Ok(Json(ManagementStatus {
        status: if healthy { "ok" } else { "degraded" },
        instance_id: state.instance_id.clone(),
        version: env!("CARGO_PKG_VERSION"),
        checks: ManagementChecks {
            postgres,
            redis,
            clickhouse,
        },
    }))
}

#[derive(Debug, Serialize)]
pub(crate) struct ManagementModel {
    id: String,
    name: String,
    category: String,
    context_length: Option<i64>,
}

pub(crate) async fn models(
    State(state): State<Arc<AppState>>,
    Extension(_principal): Extension<ManagementPrincipal>,
) -> Result<Json<Vec<ManagementModel>>, ManagementError> {
    let models = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        state.db.list_published_models(),
    )
    .await
    .map_err(|_| ManagementError::unavailable())?
    .map_err(|_| ManagementError::internal())?;
    Ok(Json(
        models
            .into_iter()
            .map(|model| ManagementModel {
                id: model.id,
                name: model.name,
                category: model.category,
                context_length: model.context_length,
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize)]
pub(crate) struct ManagementChannel {
    id: String,
    name: String,
    provider: String,
    enabled: bool,
    endpoint_count: usize,
    enabled_endpoint_count: usize,
}

pub(crate) async fn channels(
    State(state): State<Arc<AppState>>,
    Extension(_principal): Extension<ManagementPrincipal>,
) -> Result<Json<Vec<ManagementChannel>>, ManagementError> {
    let channels =
        tokio::time::timeout(std::time::Duration::from_secs(5), state.db.list_channels())
            .await
            .map_err(|_| ManagementError::unavailable())?
            .map_err(|_| ManagementError::internal())?;
    Ok(Json(channels.into_iter().map(channel_dto).collect()))
}

fn channel_dto(channel: Channel) -> ManagementChannel {
    ManagementChannel {
        id: channel.id,
        name: channel.name,
        provider: channel.provider,
        enabled: channel.enabled,
        endpoint_count: channel.endpoints.len(),
        enabled_endpoint_count: channel
            .endpoints
            .iter()
            .filter(|endpoint| endpoint.enabled)
            .count(),
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct RoutingHealth {
    channels: Vec<ChannelHealth>,
    total_requests_24h: u64,
    total_successes_24h: u64,
    success_rate_24h: f64,
}

#[derive(Debug, Serialize)]
pub(crate) struct ChannelHealth {
    channel_id: String,
    model: String,
    requests_24h: u64,
    successes_24h: u64,
    success_rate_24h: f64,
    avg_latency_ms: f64,
    p95_latency_ms: f64,
}

pub(crate) async fn routing_health(
    State(state): State<Arc<AppState>>,
    Extension(_principal): Extension<ManagementPrincipal>,
) -> Result<Json<RoutingHealth>, ManagementError> {
    let ch = state.ch.as_ref().ok_or_else(ManagementError::unavailable)?;
    let usage = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        ch.query_channel_usage_24h_all(),
    )
    .await
    .map_err(|_| ManagementError::unavailable())?
    .map_err(|_| ManagementError::unavailable())?;
    let mut total_requests = 0u64;
    let mut total_successes = 0u64;
    let channels = usage
        .into_iter()
        .map(
            |(channel_id, model, requests, successes, avg_latency, p95_latency)| {
                total_requests += requests;
                total_successes += successes;
                ChannelHealth {
                    channel_id,
                    model,
                    requests_24h: requests,
                    successes_24h: successes,
                    success_rate_24h: rate(successes, requests),
                    avg_latency_ms: avg_latency,
                    p95_latency_ms: p95_latency,
                }
            },
        )
        .collect();

    Ok(Json(RoutingHealth {
        channels,
        total_requests_24h: total_requests,
        total_successes_24h: total_successes,
        success_rate_24h: rate(total_successes, total_requests),
    }))
}

fn rate(successes: u64, requests: u64) -> f64 {
    if requests == 0 {
        0.0
    } else {
        successes as f64 / requests as f64
    }
}
