use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::balancer::{BreakerSnapshot, LoadBalancer};
use crate::config::types::EndpointConfig;
use crate::domain::model::{EndpointWeightOverride, Model, ModelChannel};

/// Stable identity for a model-to-channel binding.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct BindingKey {
    pub model_id: String,
    pub channel_id: String,
}

impl BindingKey {
    pub fn new(model_id: impl Into<String>, channel_id: impl Into<String>) -> Self {
        Self {
            model_id: model_id.into(),
            channel_id: channel_id.into(),
        }
    }
}

/// Runtime endpoint state scoped to one model binding.
///
/// The pool deliberately contains no credentials and does not persist data.
/// It is reconciled from the routing snapshot whenever configuration changes.
#[derive(Default)]
pub struct BindingStatePool {
    bindings: RwLock<HashMap<BindingKey, Arc<LoadBalancer>>>,
}

/// Build the effective endpoint list for one binding: the channel's endpoint
/// defaults with the binding's per-endpoint weight overrides applied. Only
/// endpoints listed in `overrides` change; everything else inherits.
fn build_binding_endpoints(
    channel_endpoints: &[EndpointConfig],
    overrides: &[EndpointWeightOverride],
) -> Vec<EndpointConfig> {
    if overrides.is_empty() {
        return channel_endpoints.to_vec();
    }
    let map: HashMap<i64, u32> = overrides
        .iter()
        .map(|o| (o.endpoint_id, o.weight))
        .collect();
    channel_endpoints
        .iter()
        .map(|ep| {
            let mut ep = ep.clone();
            if let Some(w) = ep.id.and_then(|id| map.get(&id)) {
                ep.weight = *w;
            }
            ep
        })
        .collect()
}

impl BindingStatePool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep existing balancers for unchanged model bindings and rebuild only
    /// the routing index. Endpoint breaker state is reconciled by stable endpoint
    /// identity inside `LoadBalancer::rebuild_preserving_state`.
    ///
    /// Each binding's balancer is built from the channel's endpoint defaults
    /// merged with the binding's per-endpoint weight overrides, so a model can
    /// steer traffic within a shared channel without affecting other models.
    pub fn reconcile(
        &self,
        models: &[Model],
        routes: &HashMap<String, (String, Arc<LoadBalancer>)>,
    ) {
        let previous = self.bindings.read().unwrap_or_else(|e| e.into_inner());
        let mut next = HashMap::new();

        for model in models {
            for binding in &model.channels {
                let Some((_, channel_balancer)) = routes.get(&binding.channel_id) else {
                    continue;
                };
                let channel_endpoints = channel_balancer.as_health_aware().endpoints().to_vec();
                let endpoints =
                    build_binding_endpoints(&channel_endpoints, &binding.endpoint_weight_overrides);
                let key = BindingKey::new(&model.id, &binding.channel_id);
                let balancer = previous
                    .get(&key)
                    .map(|old| LoadBalancer::rebuild_preserving_state(old, &endpoints))
                    .unwrap_or_else(|| LoadBalancer::new(&endpoints));
                next.insert(key, Arc::new(balancer));
            }
        }

        drop(previous);
        *self.bindings.write().unwrap_or_else(|e| e.into_inner()) = next;
    }

    pub fn get(&self, model_id: &str, channel_id: &str) -> Option<Arc<LoadBalancer>> {
        self.bindings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&BindingKey::new(model_id, channel_id))
            .cloned()
    }

    /// Snapshot of all (key, balancer) pairs. Used by aggregate health queries.
    pub fn iter(&self) -> Vec<(BindingKey, Arc<LoadBalancer>)> {
        self.bindings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn has_healthy(&self, model_id: &str, channel_id: &str) -> bool {
        self.get(model_id, channel_id)
            .is_some_and(|balancer| balancer.as_health_aware().has_healthy_endpoint())
    }

    pub fn set_endpoint_enabled(&self, endpoint_id: i64, enabled: bool) {
        let bindings = self.bindings.read().unwrap_or_else(|e| e.into_inner());
        for balancer in bindings.values() {
            for (index, endpoint) in balancer.as_health_aware().endpoints().iter().enumerate() {
                if endpoint.id == Some(endpoint_id) {
                    balancer.as_health_aware().breakers()[index].set_enabled(enabled);
                }
            }
        }
    }

    /// Record a probe outcome on every binding balancer containing the endpoint
    /// (matched by DB id). Success is a physical fact about the endpoint, so it
    /// may reset every model sharing it; used after a successful manual/auto
    /// probe so the binding pool reflects the real endpoint status.
    pub fn record_endpoint_health(&self, endpoint_id: i64, success: bool) {
        let bindings = self.bindings.read().unwrap_or_else(|e| e.into_inner());
        for balancer in bindings.values() {
            for (index, endpoint) in balancer.as_health_aware().endpoints().iter().enumerate() {
                if endpoint.id == Some(endpoint_id) {
                    if success {
                        balancer.as_health_aware().record_success(index);
                    } else {
                        balancer.as_health_aware().record_failure(index);
                    }
                }
            }
        }
    }

    /// Record a probe outcome on only the given model binding's balancer.
    ///
    /// A manual probe tests one specific model; its failure may be model-scoped
    /// (wrong upstream model name, auth contract, etc.) and must not open the
    /// breaker for other models sharing the same endpoint.
    pub fn record_endpoint_health_for_model(
        &self,
        model_id: &str,
        channel_id: &str,
        endpoint_id: i64,
        success: bool,
    ) {
        let Some(balancer) = self.get(model_id, channel_id) else {
            return;
        };
        let health = balancer.as_health_aware();
        for (index, endpoint) in health.endpoints().iter().enumerate() {
            if endpoint.id == Some(endpoint_id) {
                if success {
                    health.record_success(index);
                } else {
                    health.record_failure(index);
                }
                break;
            }
        }
    }

    /// One persisted snapshot per breaker, keyed by (model, channel, endpoint id).
    pub fn all_snapshots(&self) -> Vec<(String, String, Vec<(i64, BreakerSnapshot)>)> {
        let bindings = self.bindings.read().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::with_capacity(bindings.len());
        for (key, balancer) in bindings.iter() {
            let health = balancer.as_health_aware();
            let snapshots = health
                .endpoints()
                .iter()
                .enumerate()
                .filter_map(|(i, ep)| ep.id.map(|id| (id, health.breakers()[i].snapshot())))
                .collect::<Vec<_>>();
            if !snapshots.is_empty() {
                out.push((key.model_id.clone(), key.channel_id.clone(), snapshots));
            }
        }
        out
    }

    /// Restore persisted breaker snapshots by (model, channel, endpoint id).
    pub fn restore_snapshots(&self, snapshots: &[(String, String, Vec<(i64, BreakerSnapshot)>)]) {
        let bindings = self.bindings.read().unwrap_or_else(|e| e.into_inner());
        for (model_id, channel_id, rows) in snapshots {
            let Some(balancer) = bindings.get(&BindingKey::new(model_id, channel_id)) else {
                continue;
            };
            let health = balancer.as_health_aware();
            for (endpoint_id, snap) in rows {
                for (i, ep) in health.endpoints().iter().enumerate() {
                    if ep.id == Some(*endpoint_id) {
                        health.breakers()[i].restore(snap);
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::EndpointConfig;

    fn endpoint(id: i64) -> EndpointConfig {
        EndpointConfig {
            id: Some(id),
            url: format!("https://example-{id}.test/v1"),
            api_key: String::new(),
            weight: 1,
            timeout_secs: None,
            enabled: true,
            full_url: false,
        }
    }

    #[test]
    fn partial_overrides_preserve_unlisted_channel_defaults() {
        let endpoints = vec![endpoint(1), endpoint(2)];
        let effective = build_binding_endpoints(
            &endpoints,
            &[EndpointWeightOverride {
                endpoint_id: 1,
                weight: 8,
            }],
        );
        assert_eq!(effective[0].weight, 8);
        assert_eq!(effective[1].weight, 1);
    }

    #[test]
    fn empty_overrides_inherit_all_channel_defaults() {
        let endpoints = vec![endpoint(1), endpoint(2)];
        let effective = build_binding_endpoints(&endpoints, &[]);
        assert_eq!(
            effective.iter().map(|e| e.weight).collect::<Vec<_>>(),
            vec![1, 1]
        );
    }

    #[test]
    fn model_binding_overrides_are_isolated() {
        let pool = BindingStatePool::new();
        let channel_balancer = Arc::new(LoadBalancer::new(&vec![endpoint(1), endpoint(2)]));
        let mut routes = HashMap::new();
        routes.insert(
            "channel-a".to_string(),
            ("openai".to_string(), channel_balancer),
        );
        let models = vec![
            Model {
                id: "model-a".to_string(),
                name: "a".to_string(),
                model_pattern: "a".to_string(),
                pricing: Default::default(),
                channels: vec![ModelChannel {
                    model_id: "model-a".to_string(),
                    binding_id: None,
                    channel_id: "channel-a".to_string(),
                    priority: 1,
                    provider: String::new(),
                    upstream_model: None,
                    max_tokens: None,
                    endpoint_weight_overrides: vec![EndpointWeightOverride {
                        endpoint_id: 1,
                        weight: 9,
                    }],
                }],
                published: true,
                context_length: None,
                category: String::new(),
            },
            Model {
                id: "model-b".to_string(),
                name: "b".to_string(),
                model_pattern: "b".to_string(),
                pricing: Default::default(),
                channels: vec![ModelChannel {
                    model_id: "model-b".to_string(),
                    binding_id: None,
                    channel_id: "channel-a".to_string(),
                    priority: 1,
                    provider: String::new(),
                    upstream_model: None,
                    max_tokens: None,
                    endpoint_weight_overrides: Vec::new(),
                }],
                published: true,
                context_length: None,
                category: String::new(),
            },
        ];
        pool.reconcile(&models, &routes);
        assert_eq!(
            pool.get("model-a", "channel-a")
                .unwrap()
                .as_health_aware()
                .endpoint(0)
                .unwrap()
                .weight,
            9
        );
        assert_eq!(
            pool.get("model-b", "channel-a")
                .unwrap()
                .as_health_aware()
                .endpoint(0)
                .unwrap()
                .weight,
            1
        );
    }
    #[test]
    fn binding_keys_are_model_scoped() {
        let pool = BindingStatePool::new();
        let first = Arc::new(LoadBalancer::new(&vec![endpoint(1)]));
        let mut routes = HashMap::new();
        routes.insert(
            "channel-a".to_string(),
            ("openai".to_string(), first.clone()),
        );

        let models = vec![
            Model {
                id: "model-a".to_string(),
                name: "display-a".to_string(),
                model_pattern: "display-a".to_string(),
                pricing: Default::default(),
                channels: vec![crate::domain::model::ModelChannel {
                    model_id: "model-a".to_string(),
                    binding_id: None,
                    channel_id: "channel-a".to_string(),
                    priority: 1,
                    provider: String::new(),
                    upstream_model: None,
                    max_tokens: None,
                    endpoint_weight_overrides: Vec::new(),
                }],
                published: true,
                context_length: None,
                category: String::new(),
            },
            Model {
                id: "model-b".to_string(),
                name: "display-b".to_string(),
                model_pattern: "display-b".to_string(),
                pricing: Default::default(),
                channels: vec![crate::domain::model::ModelChannel {
                    model_id: "model-b".to_string(),
                    binding_id: None,
                    channel_id: "channel-a".to_string(),
                    priority: 1,
                    provider: String::new(),
                    upstream_model: None,
                    max_tokens: None,
                    endpoint_weight_overrides: Vec::new(),
                }],
                published: true,
                context_length: None,
                category: String::new(),
            },
        ];
        pool.reconcile(&models, &routes);
        let model_a = pool.get("model-a", "channel-a").unwrap();
        let model_b = pool.get("model-b", "channel-a").unwrap();
        model_a.as_health_aware().record_failure(0);
        model_a.as_health_aware().record_failure(0);
        model_a.as_health_aware().record_failure(0);

        assert!(!model_a.as_health_aware().has_available_endpoint());
        assert!(model_b.as_health_aware().has_available_endpoint());
    }
}
