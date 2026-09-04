use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Serialize;

use crate::domain::model::{Model, ModelChannel};
use crate::server::AppState;

use super::*;

// ── Effective Scheduler Topology ───────────────────────────────────
//
// The scheduler is the final interpreter of effective routing state. This
// endpoint returns what the scheduler actually compiled (config → effective
// endpoint weights → breaker state → observed shares), so the frontend never
// has to derive routing state by stitching together several APIs.

#[derive(Debug, Serialize)]
pub(crate) struct TopologyResponse {
    pub model: String,
    pub bindings: Vec<BindingTopology>,
}

#[derive(Debug, Serialize)]
pub(crate) struct BindingTopology {
    pub channel_id: String,
    pub channel_name: String,
    pub provider: String,
    pub priority: i32,
    pub upstream_model: Option<String>,
    pub max_tokens: Option<u32>,

    pub routing_state: String,
    pub routing_reason: String,

    // Observed traffic over 24h (None when no requests).
    pub request_count_24h: u64,
    pub observed_model_share_24h: Option<f64>,
    pub observed_endpoint_total_24h: u64,

    pub endpoints: Vec<EndpointTopology>,
}

#[derive(Debug, Serialize)]
pub(crate) struct EndpointTopology {
    pub endpoint_id: i64,
    pub url: String,

    // Configuration layer.
    pub default_weight: u32,
    pub override_weight: Option<u32>,
    pub effective_weight: u32,
    pub weight_source: String,

    // Current scheduling eligibility.
    pub routing_available: bool,
    pub routing_state: String,
    pub routing_reason: String,

    // Circuit breaker (strictly model × channel × endpoint).
    pub circuit_state: String,

    // Distribution layer.
    pub configured_share: Option<f64>,
    pub eligible_share: Option<f64>,
    pub observed_endpoint_share_24h: Option<f64>,
}

struct BindingInput<'a> {
    binding: &'a ModelChannel,
    channel: Option<crate::domain::channel::Channel>,
    // endpoint_id -> default weight (from channel).
    defaults: Vec<(i64, u32, String)>,
    // effective weights after override merge.
    effective: HashMap<i64, u32>,
    // breaker state per endpoint index (from binding balancer).
    breakers: Vec<(i64, bool, bool)>, // (endpoint_id, enabled, healthy)
}

fn binding_is_eligible(bi: &BindingInput<'_>) -> bool {
    bi.channel.as_ref().is_some_and(|ch| ch.enabled)
        && bi
            .breakers
            .iter()
            .any(|(_, enabled, healthy)| *enabled && *healthy)
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

    // ── Observed traffic (ClickHouse, observability data only) ──
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let mut model_channel_usage: HashMap<String, u64> = ch
        .query_model_channel_usage_24h(&model.name)
        .await
        .map_err(AdminError::internal)?
        .into_iter()
        .collect();
    let model_total_24h: u64 = model_channel_usage.values().sum();

    let mut bindings: Vec<BindingInput<'_>> = Vec::new();
    for binding in &model.channels {
        let channel = state.routing.get_channel(&binding.channel_id);
        let defaults: Vec<(i64, u32, String)> = channel
            .as_ref()
            .map(|c| {
                c.endpoints
                    .iter()
                    .filter_map(|ep| ep.id.map(|id| (id, ep.weight, ep.url.clone())))
                    .collect()
            })
            .unwrap_or_default();

        // Effective weight: binding override else channel default.
        let override_map: HashMap<i64, u32> = binding
            .endpoint_weight_overrides
            .iter()
            .map(|ov| (ov.endpoint_id, ov.weight))
            .collect();
        let effective: HashMap<i64, u32> = defaults
            .iter()
            .map(|(id, def, _)| (*id, override_map.get(id).copied().unwrap_or(*def)))
            .collect();

        // Breaker state from the binding-specific balancer.
        let binding_balancer = state
            .routing
            .get_binding_route(&model.id, &binding.channel_id);
        let mut breakers: Vec<(i64, bool, bool)> = Vec::new();
        if let Some(bal) = &binding_balancer {
            let health = bal.as_health_aware();
            for (i, ep) in health.endpoints().iter().enumerate() {
                if let Some(id) = ep.id {
                    let br = &health.breakers()[i];
                    breakers.push((id, br.is_enabled(), br.is_healthy()));
                }
            }
        }

        bindings.push(BindingInput {
            binding,
            channel,
            defaults,
            effective,
            breakers,
        });
    }

    // ── Routing state: active = best-priority eligible group; standby =
    //    eligible at a worse priority; blocked = not eligible.
    let mut min_eligible_priority: Option<i32> = None;
    for bi in &bindings {
        if binding_is_eligible(bi) {
            let p = bi.binding.priority;
            min_eligible_priority = Some(min_eligible_priority.map_or(p, |m| m.min(p)));
        }
    }

    // Per-binding observed endpoint counts (denominator = this binding).
    let mut endpoint_usage: HashMap<String, HashMap<i64, u64>> = HashMap::new();
    for binding in &model.channels {
        let rows = ch
            .query_endpoint_usage_24h(&model.name, &binding.channel_id)
            .await
            .map_err(AdminError::internal)?;
        endpoint_usage.insert(binding.channel_id.clone(), rows.into_iter().collect());
    }

    let mut out_bindings = Vec::new();
    for bi in &bindings {
        let b = bi.binding;
        let eligible = binding_is_eligible(bi);
        let routing_state = if !eligible {
            "blocked".to_string()
        } else if min_eligible_priority == Some(b.priority) {
            "active".to_string()
        } else {
            "standby".to_string()
        };
        let routing_reason = match routing_state.as_str() {
            "active" => "当前最高健康优先级".to_string(),
            "standby" => format!(
                "存在可用 Priority {} binding",
                min_eligible_priority.unwrap_or(b.priority)
            ),
            _ => {
                if bi.channel.as_ref().is_some_and(|c| !c.enabled) {
                    "渠道已禁用".to_string()
                } else if !bi.breakers.iter().any(|(_, e, h)| *e && *h) {
                    "无健康端点（熔断/禁用）".to_string()
                } else {
                    "渠道未启用".to_string()
                }
            }
        };

        // Observed traffic.
        let channel_req = model_channel_usage.get(&b.channel_id).copied().unwrap_or(0);
        let observed_model_share = if model_total_24h > 0 {
            Some(channel_req as f64 / model_total_24h as f64)
        } else {
            None
        };
        let endpoint_reqs = endpoint_usage
            .get(&b.channel_id)
            .cloned()
            .unwrap_or_default();
        let endpoint_total: u64 = endpoint_reqs.values().sum();
        let observed_endpoint_total_24h = endpoint_total;

        // Endpoint topology.
        let default_total: u32 = bi.defaults.iter().map(|(_, w, _)| w).sum();
        let eligible_weights: u32 = bi
            .breakers
            .iter()
            .filter(|(_, enabled, healthy)| *enabled && *healthy)
            .filter_map(|(id, _, _)| bi.effective.get(id))
            .copied()
            .sum();
        let any_eligible = bi
            .breakers
            .iter()
            .any(|(_, enabled, healthy)| *enabled && *healthy);

        let mut endpoints = Vec::new();
        for (id, default_weight, url) in &bi.defaults {
            let override_weight = bi
                .binding
                .endpoint_weight_overrides
                .iter()
                .find(|ov| ov.endpoint_id == *id)
                .map(|ov| ov.weight);
            let effective_weight = bi.effective.get(id).copied().unwrap_or(*default_weight);
            let (breaker_enabled, breaker_healthy) = bi
                .breakers
                .iter()
                .find(|(eid, _, _)| eid == id)
                .map(|(_, e, h)| (*e, *h))
                .unwrap_or((false, false));
            let circuit_state = if !breaker_enabled {
                "disabled"
            } else if breaker_healthy {
                "closed"
            } else {
                "open"
            }
            .to_string();
            let routing_available = breaker_enabled && breaker_healthy;
            let routing_state = if routing_available {
                "eligible"
            } else {
                "excluded"
            }
            .to_string();
            let routing_reason = if !breaker_enabled {
                "Endpoint disabled".to_string()
            } else if breaker_healthy {
                "Eligible".to_string()
            } else {
                "Circuit breaker open".to_string()
            };

            let configured_share = if default_total > 0 {
                Some(*default_weight as f64 / default_total as f64)
            } else {
                None
            };
            let eligible_share = if any_eligible && eligible_weights > 0 {
                Some(if routing_available {
                    effective_weight as f64 / eligible_weights as f64
                } else {
                    0.0
                })
            } else {
                None
            };
            let observed_endpoint_share_24h = if endpoint_total > 0 {
                endpoint_reqs
                    .get(id)
                    .map(|c| *c as f64 / endpoint_total as f64)
            } else {
                None
            };

            endpoints.push(EndpointTopology {
                endpoint_id: *id,
                url: url.clone(),
                default_weight: *default_weight,
                override_weight,
                effective_weight,
                weight_source: if override_weight.is_some() {
                    "binding_override".to_string()
                } else {
                    "channel_default".to_string()
                },
                routing_available,
                routing_state,
                routing_reason,
                circuit_state,
                configured_share,
                eligible_share,
                observed_endpoint_share_24h,
            });
        }

        out_bindings.push(BindingTopology {
            channel_id: b.channel_id.clone(),
            channel_name: bi
                .channel
                .as_ref()
                .map(|c| {
                    if c.name.is_empty() {
                        c.id.clone()
                    } else {
                        c.name.clone()
                    }
                })
                .unwrap_or_else(|| b.channel_id.clone()),
            provider: b.provider.clone(),
            priority: b.priority,
            upstream_model: b.upstream_model.clone(),
            max_tokens: b.max_tokens,
            routing_state,
            routing_reason,
            request_count_24h: channel_req,
            observed_model_share_24h: observed_model_share,
            observed_endpoint_total_24h,
            endpoints,
        });
    }

    // Order by priority then channel.
    out_bindings.sort_by(|a, c| {
        a.priority
            .cmp(&c.priority)
            .then(a.channel_id.cmp(&c.channel_id))
    });

    Ok(Json(
        serde_json::to_value(TopologyResponse {
            model: model.name.clone(),
            bindings: out_bindings,
        })
        .map_err(|e| AdminError::internal(e.to_string()))?,
    ))
}
