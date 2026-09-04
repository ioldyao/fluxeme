use std::collections::HashSet;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::domain::scheduler::SchedulerEndpointPolicy;
use crate::server::AppState;

use super::*;

// Scheduler Control is endpoint-centric. A channel is only an endpoint group;
// every scheduling field below belongs to one model × endpoint.

#[derive(Debug, Serialize)]
pub(crate) struct SchedulerModelSummary {
    pub id: String,
    pub name: String,
    pub published: bool,
    pub binding_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SchedulerModelPolicy {
    pub model_id: String,
    pub bindings: Vec<BindingPolicyDoc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BindingPolicyDoc {
    pub channel_id: String,
    pub endpoints: Vec<EndpointPolicyDoc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EndpointPolicyDoc {
    pub endpoint_id: i64,
    pub weight: u32,
    pub timeout_secs: Option<u64>,
    pub max_tokens: Option<u32>,
}

pub(crate) async fn list_scheduler_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<SchedulerModelSummary>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;
    // Only published models are schedulable — an unpublished model has no
    // public routing surface, so there is nothing to schedule.
    let models = state.db.list_published_models().await.map_err(db_err)?;
    Ok(Json(
        models
            .into_iter()
            .map(|m| SchedulerModelSummary {
                binding_count: m.channels.len(),
                id: m.id,
                name: m.name,
                published: m.published,
            })
            .collect(),
    ))
}

pub(crate) async fn get_scheduler_model_policy(
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
    let channels = state.db.list_channels().await.map_err(db_err)?;
    let channel_by_id: std::collections::HashMap<String, _> =
        channels.into_iter().map(|c| (c.id.clone(), c)).collect();
    let endpoint_policies = state.routing.endpoint_policies_snapshot();
    let mut bindings = Vec::new();
    for binding in &model.channels {
        let pol_map: std::collections::HashMap<i64, SchedulerEndpointPolicy> = endpoint_policies
            .get(&(model_id.clone(), binding.channel_id.clone()))
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|p| (p.endpoint_id, p))
            .collect();
        let endpoints = channel_by_id
            .get(&binding.channel_id)
            .map(|channel| {
                channel
                    .endpoints
                    .iter()
                    .filter_map(|ep| ep.id)
                    .map(|id| {
                        let p = pol_map
                            .get(&id)
                            .cloned()
                            .unwrap_or(SchedulerEndpointPolicy {
                                model_id: model_id.clone(),
                                channel_id: binding.channel_id.clone(),
                                endpoint_id: id,
                                weight: 1,
                                timeout_secs: None,
                                max_tokens: None,
                            });
                        EndpointPolicyDoc {
                            endpoint_id: id,
                            weight: p.weight,
                            timeout_secs: p.timeout_secs,
                            max_tokens: p.max_tokens,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        bindings.push(BindingPolicyDoc {
            channel_id: binding.channel_id.clone(),
            endpoints,
        });
    }
    Ok(Json(
        serde_json::json!({ "model_id": model.id, "model_name": model.name, "bindings": bindings }),
    ))
}

pub(crate) async fn put_scheduler_model_policy(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
    Json(policy): Json<SchedulerModelPolicy>,
) -> Result<Json<serde_json::Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;
    if policy.model_id != model_id {
        return Err(AdminError::bad_request("Model ID mismatch"));
    }
    let model = state
        .db
        .get_model(&model_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("Model not found"))?;
    let channels = state.db.list_channels().await.map_err(db_err)?;
    let channel_by_id: std::collections::HashMap<String, _> =
        channels.into_iter().map(|c| (c.id.clone(), c)).collect();
    let valid_bindings: HashSet<&str> = model
        .channels
        .iter()
        .map(|b| b.channel_id.as_str())
        .collect();
    let mut seen: HashSet<i64> = HashSet::new();
    let mut rows = Vec::new();
    for binding in &policy.bindings {
        if !valid_bindings.contains(binding.channel_id.as_str()) {
            return Err(AdminError::bad_request(format!(
                "Channel '{}' is not a binding of this model",
                binding.channel_id
            )));
        }
        let channel = channel_by_id
            .get(&binding.channel_id)
            .ok_or_else(|| AdminError::bad_request("Channel does not exist"))?;
        for ep in &binding.endpoints {
            if !seen.insert(ep.endpoint_id) {
                return Err(AdminError::bad_request(format!(
                    "Duplicate endpoint {}",
                    ep.endpoint_id
                )));
            }
            if ep.weight == 0 {
                return Err(AdminError::bad_request(
                    "Endpoint weight must be greater than zero",
                ));
            }
            if ep.timeout_secs == Some(0) {
                return Err(AdminError::bad_request(
                    "Endpoint timeout must be greater than zero",
                ));
            }
            if ep.max_tokens == Some(0) {
                return Err(AdminError::bad_request(
                    "Endpoint max_tokens must be greater than zero",
                ));
            }
            if !channel
                .endpoints
                .iter()
                .any(|candidate| candidate.id == Some(ep.endpoint_id))
            {
                return Err(AdminError::bad_request(format!(
                    "Endpoint {} does not belong to channel '{}'",
                    ep.endpoint_id, binding.channel_id
                )));
            }
            rows.push(SchedulerEndpointPolicy {
                model_id: model_id.clone(),
                channel_id: binding.channel_id.clone(),
                endpoint_id: ep.endpoint_id,
                weight: ep.weight,
                timeout_secs: ep.timeout_secs,
                max_tokens: ep.max_tokens,
            });
        }
    }
    state
        .db
        .replace_endpoint_policies(&model_id, &rows)
        .await
        .map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;
    tracing::info!(admin = %session.user_id, action = "put_scheduler_policy", model = %model_id, endpoints = rows.len());
    Ok(Json(serde_json::json!({ "ok": true })))
}
