use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

use crate::balancer::{BreakerSnapshot, CircuitBreaker, LoadBalancer};
use crate::config::types::EndpointConfig;
use crate::domain::model::Model;
use crate::domain::scheduler::SchedulerEndpointPolicy;

/// Runtime state for one model-visible upstream endpoint.
///
/// The endpoint's channel is metadata used by dispatch, observability, and
/// recovery scope. It is not a scheduling level: all states in one model
/// runtime are one candidate set.
#[derive(Clone)]
pub struct EndpointRuntimeState {
    pub endpoint_id: i64,
    pub channel_id: String,
    pub channel_enabled: bool,
    pub provider: String,
    pub upstream_model: Option<String>,
    pub endpoint: EndpointConfig,
    pub breaker: Arc<CircuitBreaker>,
}

/// Compiled endpoint runtime for one logical model. The selector counter is
/// kept here so weighted selection remains stateful across requests.
pub struct ModelEndpointRuntime {
    pub endpoints: Vec<EndpointRuntimeState>,
    counter: AtomicUsize,
}

impl ModelEndpointRuntime {
    fn new(endpoints: Vec<EndpointRuntimeState>) -> Self {
        Self {
            endpoints,
            counter: AtomicUsize::new(0),
        }
    }

    pub fn endpoint_count(&self) -> usize {
        self.endpoints.len()
    }

    pub fn has_healthy_endpoint(&self, channel_scope: Option<&str>) -> bool {
        self.endpoints.iter().any(|e| {
            e.channel_enabled
                && channel_scope.is_none_or(|scope| e.channel_id == scope)
                && e.breaker.is_healthy()
        })
    }

    pub fn select_healthy_excluding(
        &self,
        channel_scope: Option<&str>,
        upstream_model: Option<&str>,
        excluded_endpoint_ids: &std::collections::HashSet<i64>,
        excluded_indexes: &std::collections::HashSet<usize>,
    ) -> Option<usize> {
        let candidates: Vec<usize> = self
            .endpoints
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                (e.channel_enabled
                    && channel_scope.is_none_or(|scope| e.channel_id == scope)
                    && upstream_model
                        .is_none_or(|upstream| e.upstream_model.as_deref() == Some(upstream))
                    && e.breaker.is_healthy()
                    && !excluded_indexes.contains(&i)
                    && !excluded_endpoint_ids.contains(&e.endpoint_id))
                .then_some(i)
            })
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let total: u32 = candidates
            .iter()
            .map(|&i| self.endpoints[i].endpoint.weight)
            .sum();
        if total == 0 {
            return Some(
                candidates[self.counter.fetch_add(1, Ordering::Relaxed) % candidates.len()],
            );
        }
        let pos = self.counter.fetch_add(1, Ordering::Relaxed) % total as usize;
        let mut cumulative = 0u32;
        for i in candidates {
            cumulative = cumulative.saturating_add(self.endpoints[i].endpoint.weight);
            if pos < cumulative as usize {
                return Some(i);
            }
        }
        None
    }

    pub fn set_endpoint_enabled(&self, endpoint_id: i64, enabled: bool) {
        for state in &self.endpoints {
            if state.endpoint_id == endpoint_id {
                state.breaker.set_enabled(enabled);
            }
        }
    }

    pub fn record_endpoint_health(&self, endpoint_id: i64, success: bool) {
        for state in &self.endpoints {
            if state.endpoint_id == endpoint_id {
                if success {
                    state.breaker.record_success();
                } else {
                    state.breaker.record_failure();
                }
            }
        }
    }

    pub fn all_snapshots(&self) -> Vec<(i64, BreakerSnapshot)> {
        self.endpoints
            .iter()
            .map(|e| (e.endpoint_id, e.breaker.snapshot()))
            .collect()
    }
}

/// Compiled model → endpoint runtime pool. Channels are expanded during
/// reconcile; request handling never rebuilds ModelChannel configuration.
#[derive(Default)]
pub struct ModelEndpointPool {
    models: RwLock<HashMap<String, Arc<ModelEndpointRuntime>>>,
}

fn build_endpoint(
    channel_id: &str,
    channel_enabled: bool,
    provider: &str,
    upstream_model: Option<String>,
    endpoint: &EndpointConfig,
    policy: Option<&SchedulerEndpointPolicy>,
) -> EndpointRuntimeState {
    let mut compiled = endpoint.clone();
    compiled.weight = policy.map_or(1, |p| p.weight);
    compiled.timeout_secs = policy.and_then(|p| p.timeout_secs);
    compiled.max_tokens = policy.and_then(|p| p.max_tokens);
    EndpointRuntimeState {
        endpoint_id: endpoint.id.unwrap_or_default(),
        channel_id: channel_id.to_string(),
        channel_enabled,
        provider: provider.to_string(),
        upstream_model,
        endpoint: compiled.clone(),
        breaker: Arc::new(CircuitBreaker::new(compiled.enabled)),
    }
}

impl ModelEndpointPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reconcile(
        &self,
        models: &[Model],
        routes: &HashMap<String, (String, Arc<LoadBalancer>)>,
        channels: &std::collections::HashMap<String, Arc<crate::domain::channel::Channel>>,
        endpoint_policies: &HashMap<(String, String), Vec<SchedulerEndpointPolicy>>,
    ) {
        let previous = self.models.read().unwrap_or_else(|e| e.into_inner());
        let mut next = HashMap::new();
        for model in models {
            let mut endpoints = Vec::new();
            for binding in &model.channels {
                let Some((provider, channel_balancer)) = routes.get(&binding.channel_id) else {
                    continue;
                };
                let channel_enabled = channels
                    .get(&binding.channel_id)
                    .is_some_and(|channel| channel.enabled);
                let policy_map: HashMap<i64, &SchedulerEndpointPolicy> = endpoint_policies
                    .get(&(model.id.clone(), binding.channel_id.clone()))
                    .map(|rows| rows.iter().map(|p| (p.endpoint_id, p)).collect())
                    .unwrap_or_default();
                for endpoint in channel_balancer.as_health_aware().endpoints() {
                    let Some(endpoint_id) = endpoint.id else {
                        continue;
                    };
                    let mut state = build_endpoint(
                        &binding.channel_id,
                        channel_enabled,
                        provider,
                        Some(
                            binding
                                .upstream_model
                                .clone()
                                .unwrap_or_else(|| model.name.clone()),
                        ),
                        endpoint,
                        policy_map.get(&endpoint_id).copied(),
                    );
                    if let Some(old) = previous.get(&model.id) {
                        if let Some(old_state) = old.endpoints.iter().find(|e| {
                            e.endpoint_id == endpoint_id && e.channel_id == state.channel_id
                        }) {
                            state.breaker = old_state.breaker.clone();
                        }
                    }
                    endpoints.push(state);
                }
            }
            next.insert(
                model.id.clone(),
                Arc::new(ModelEndpointRuntime::new(endpoints)),
            );
        }
        drop(previous);
        *self.models.write().unwrap_or_else(|e| e.into_inner()) = next;
    }

    pub fn get(&self, model_id: &str) -> Option<Arc<ModelEndpointRuntime>> {
        self.models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(model_id)
            .cloned()
    }

    pub fn iter(&self) -> Vec<(String, Arc<ModelEndpointRuntime>)> {
        self.models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn set_endpoint_enabled(&self, endpoint_id: i64, enabled: bool) {
        for runtime in self
            .models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            runtime.set_endpoint_enabled(endpoint_id, enabled);
        }
    }

    pub fn record_endpoint_health(&self, endpoint_id: i64, success: bool) {
        for runtime in self
            .models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            runtime.record_endpoint_health(endpoint_id, success);
        }
    }

    pub fn record_endpoint_health_for_model(
        &self,
        model_id: &str,
        endpoint_id: i64,
        success: bool,
    ) {
        if let Some(runtime) = self.get(model_id) {
            runtime.record_endpoint_health(endpoint_id, success);
        }
    }

    pub fn all_snapshots(&self) -> Vec<(String, Vec<(i64, BreakerSnapshot)>)> {
        self.iter()
            .into_iter()
            .map(|(model, runtime)| (model, runtime.all_snapshots()))
            .collect()
    }

    pub fn restore_snapshots(&self, snapshots: &[(String, Vec<(i64, BreakerSnapshot)>)]) {
        for (model_id, rows) in snapshots {
            let Some(runtime) = self.get(model_id) else {
                continue;
            };
            for (id, snapshot) in rows {
                if let Some(state) = runtime.endpoints.iter().find(|e| e.endpoint_id == *id) {
                    state.breaker.restore(snapshot);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::EndpointConfig;
    use crate::domain::model::ModelChannel;

    fn endpoint(id: i64) -> EndpointConfig {
        EndpointConfig {
            id: Some(id),
            url: format!("https://example-{id}.test/v1"),
            api_key: String::new(),
            weight: 1,
            timeout_secs: None,
            max_tokens: None,
            enabled: true,
            full_url: false,
        }
    }

    #[test]
    fn partial_policies_apply_endpoint_settings() {
        let runtime = ModelEndpointRuntime::new(vec![build_endpoint(
            "c",
            true,
            "openai",
            None,
            &endpoint(1),
            Some(&SchedulerEndpointPolicy {
                model_id: "m".into(),
                channel_id: "c".into(),
                endpoint_id: 1,
                weight: 8,
                timeout_secs: Some(600),
                max_tokens: Some(65536),
            }),
        )]);
        assert_eq!(runtime.endpoints[0].endpoint.weight, 8);
        assert_eq!(runtime.endpoints[0].endpoint.timeout_secs, Some(600));
        assert_eq!(runtime.endpoints[0].endpoint.max_tokens, Some(65536));
    }

    #[test]
    fn model_endpoint_runtime_is_flattened() {
        let channel = Arc::new(LoadBalancer::new(&vec![endpoint(1), endpoint(2)]));
        let mut routes = HashMap::new();
        routes.insert("a".into(), ("openai".into(), channel.clone()));
        routes.insert("b".into(), ("openai".into(), channel));
        let model = Model {
            id: "m".into(),
            name: "m".into(),
            model_pattern: "m".into(),
            pricing: Default::default(),
            channels: vec![
                ModelChannel {
                    model_id: "m".into(),
                    channel_id: "a".into(),
                    provider: "openai".into(),
                    upstream_model: None,
                },
                ModelChannel {
                    model_id: "m".into(),
                    channel_id: "b".into(),
                    provider: "openai".into(),
                    upstream_model: None,
                },
            ],
            published: true,
            context_length: None,
            category: String::new(),
        };
        let pool = ModelEndpointPool::new();
        pool.reconcile(&[model], &routes, &HashMap::new(), &HashMap::new());
        assert_eq!(pool.get("m").unwrap().endpoints.len(), 4);
    }
}
