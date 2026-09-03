use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::balancer::LoadBalancer;
use crate::domain::model::Model;

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

impl BindingStatePool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep existing balancers for unchanged model bindings and rebuild only
    /// the routing index. Endpoint breaker state is reconciled by stable endpoint
    /// identity inside `LoadBalancer::rebuild_preserving_state`.
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
                let endpoints = channel_balancer.as_health_aware().endpoints().to_vec();
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
                    channel_id: "channel-a".to_string(),
                    priority: 1,
                    provider: String::new(),
                    upstream_model: None,
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
                    channel_id: "channel-a".to_string(),
                    priority: 1,
                    provider: String::new(),
                    upstream_model: None,
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
