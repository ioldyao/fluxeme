use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Serialize;

use crate::server::AppState;

use super::*;

// ── Effective Scheduler Topology ───────────────────────────────────
//
// The scheduler is endpoint-centric and single-level: the model's bound
// channels are expanded into one endpoint pool. This endpoint groups that
// pool by channel for display, aggregates configured/eligible weight totals,
// and reports breaker + observed traffic. Scheduling weights live only on
// endpoints; a channel's weight is the sum of its endpoints.

#[derive(Debug, Serialize)]
pub(crate) struct TopologyResponse {
    pub model: String,
    /// Model-wide weight totals (denominators for share math).
    pub configured_total_weight: u64,
    pub eligible_total_weight: u64,
    pub bindings: Vec<BindingTopology>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BindingTopology {
    pub channel_id: String,
    pub channel_name: String,
    pub provider: String,
    pub upstream_model: Option<String>,
    pub channel_enabled: bool,

    // Channel = aggregate of its endpoints.
    pub endpoint_count: usize,
    pub configured_total_weight: u64,
    pub eligible_total_weight: u64,
    pub configured_share: Option<f64>,
    pub eligible_share: Option<f64>,

    pub routing_state: String,
    pub routing_reason: String,

    pub request_count_24h: u64,
    pub observed_model_share_24h: Option<f64>,

    pub endpoints: Vec<EndpointTopology>,
}

#[derive(Debug, Serialize)]
pub(crate) struct EndpointTopology {
    pub endpoint_id: i64,
    pub url: String,

    // Scheduler policy (single source of truth).
    pub weight: u32,
    pub timeout_secs: Option<u64>,
    pub max_tokens: Option<u32>,

    pub routing_available: bool,
    pub routing_state: String,
    pub routing_reason: String,
    pub circuit_state: String,

    pub observed_endpoint_share_24h: Option<f64>,
}

fn circuit_state_label(enabled: bool, healthy: bool) -> String {
    if !enabled {
        "disabled".to_string()
    } else if healthy {
        "closed".to_string()
    } else {
        "open".to_string()
    }
}

pub(crate) async fn model_scheduler_topology(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;

    let model = state
        .db
        .get_model(&model_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Model not found"))?;
    let Some(runtime) = state.routing.get_model_endpoint_runtime(&model_id) else {
        return Err(AdminError::not_found("Model has no endpoint runtime"));
    };

    // ── Observed traffic (ClickHouse, observability data only) ──
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let model_channel_usage: HashMap<String, u64> = ch
        .query_model_channel_usage_24h(&model.name)
        .await
        .map_err(AdminError::internal)?
        .into_iter()
        .collect();
    let model_total_24h: u64 = model_channel_usage.values().sum();

    // Per-channel endpoint request totals.
    let mut endpoint_usage: HashMap<String, HashMap<i64, u64>> = HashMap::new();
    for (channel_id, _) in model_channel_usage.iter() {
        let rows = ch
            .query_endpoint_usage_24h(&model.name, channel_id)
            .await
            .map_err(AdminError::internal)?;
        endpoint_usage.insert(channel_id.clone(), rows.into_iter().collect());
    }

    let configured_total_weight: u64 = runtime
        .endpoints
        .iter()
        .map(|e| u64::from(e.endpoint.weight))
        .sum();
    let eligible_total_weight: u64 = runtime
        .endpoints
        .iter()
        .filter(|e| e.channel_enabled && e.breaker.is_healthy())
        .map(|e| u64::from(e.endpoint.weight))
        .sum();

    // Group the flattened pool by channel.
    let mut by_channel: Vec<(
        String,
        Vec<&crate::service::endpoint_pool::EndpointRuntimeState>,
    )> = Vec::new();
    for state in &runtime.endpoints {
        match by_channel
            .iter_mut()
            .find(|(id, _)| *id == state.channel_id)
        {
            Some((_, group)) => group.push(state),
            None => by_channel.push((state.channel_id.clone(), vec![state])),
        }
    }

    let mut bindings = Vec::new();
    for (channel_id, states) in by_channel {
        let channel = state.routing.get_channel(&channel_id);
        let channel_enabled = channel.as_ref().is_some_and(|c| c.enabled);
        let channel_name = channel
            .as_ref()
            .map(|c| {
                if c.name.is_empty() {
                    c.id.clone()
                } else {
                    c.name.clone()
                }
            })
            .unwrap_or_else(|| channel_id.clone());
        let provider = states
            .first()
            .map(|s| s.provider.clone())
            .unwrap_or_default();
        let upstream_model = states.first().and_then(|s| s.upstream_model.clone());
        let endpoint_count = states.len();

        let channel_configured: u64 = states.iter().map(|s| u64::from(s.endpoint.weight)).sum();
        let channel_eligible: u64 = states
            .iter()
            .filter(|s| s.channel_enabled && s.breaker.is_healthy())
            .map(|s| u64::from(s.endpoint.weight))
            .sum();
        let has_eligible = channel_eligible > 0;
        let routing_state = if has_eligible {
            "available"
        } else {
            "unavailable"
        }
        .to_string();
        let routing_reason = if !channel_enabled {
            "Channel disabled".to_string()
        } else if has_eligible {
            "Has eligible endpoints".to_string()
        } else {
            "No healthy endpoint (circuit open / disabled)".to_string()
        };

        let configured_share = if configured_total_weight > 0 {
            Some(channel_configured as f64 / configured_total_weight as f64)
        } else {
            None
        };
        let eligible_share = if eligible_total_weight > 0 {
            Some(channel_eligible as f64 / eligible_total_weight as f64)
        } else {
            None
        };

        let channel_req = model_channel_usage.get(&channel_id).copied().unwrap_or(0);
        let observed_model_share = if model_total_24h > 0 {
            Some(channel_req as f64 / model_total_24h as f64)
        } else {
            None
        };
        let endpoint_reqs = endpoint_usage.get(&channel_id).cloned().unwrap_or_default();
        let endpoint_total: u64 = endpoint_reqs.values().sum();

        let mut endpoints = Vec::new();
        for state in &states {
            let routing_available = state.channel_enabled && state.breaker.is_healthy();
            let circuit =
                circuit_state_label(state.breaker.is_enabled(), state.breaker.is_healthy());
            let routing_state = if routing_available {
                "eligible"
            } else {
                "excluded"
            }
            .to_string();
            let routing_reason = if !state.channel_enabled {
                "Channel disabled".to_string()
            } else if !state.breaker.is_enabled() {
                "Endpoint disabled".to_string()
            } else if state.breaker.is_healthy() {
                "Eligible".to_string()
            } else {
                "Circuit breaker open".to_string()
            };
            let observed_endpoint_share_24h = if endpoint_total > 0 {
                endpoint_reqs
                    .get(&state.endpoint_id)
                    .map(|c| *c as f64 / endpoint_total as f64)
            } else {
                None
            };
            endpoints.push(EndpointTopology {
                endpoint_id: state.endpoint_id,
                url: state.endpoint.url.clone(),
                weight: state.endpoint.weight,
                timeout_secs: state.endpoint.timeout_secs,
                max_tokens: state.endpoint.max_tokens,
                routing_available,
                routing_state,
                routing_reason,
                circuit_state: circuit,
                observed_endpoint_share_24h,
            });
        }

        bindings.push(BindingTopology {
            channel_id,
            channel_name,
            provider,
            upstream_model,
            channel_enabled,
            endpoint_count,
            configured_total_weight: channel_configured,
            eligible_total_weight: channel_eligible,
            configured_share,
            eligible_share,
            routing_state,
            routing_reason,
            request_count_24h: channel_req,
            observed_model_share_24h: observed_model_share,
            endpoints,
        });
    }

    // Stable order: channel name.
    bindings.sort_by(|a, b| a.channel_name.cmp(&b.channel_name));

    Ok(Json(
        serde_json::to_value(TopologyResponse {
            model: model.name.clone(),
            configured_total_weight,
            eligible_total_weight,
            bindings,
        })
        .map_err(|e| AdminError::internal(e.to_string()))?,
    ))
}
