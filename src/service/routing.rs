use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use crate::balancer::LoadBalancer;
use crate::config::types::EndpointConfig;

type RouteCacheEntry = (String, Arc<LoadBalancer>, Vec<EndpointConfig>);
type RouteCache = RwLock<HashMap<String, RouteCacheEntry>>;
use crate::domain::channel::Channel;
use crate::domain::model::Model;
use crate::domain::routing::RoutingRule;
use crate::db::Database;

/// In-memory route cache, rebuilt from DB on startup and after admin changes.
pub struct RoutingService {
    db: Arc<Database>,
    channels: RwLock<HashMap<String, Arc<Channel>>>,
    models: RwLock<Vec<Model>>,
    rules: RwLock<Vec<RoutingRule>>,
    cache: RouteCache,
}

impl RoutingService {
    pub fn new(db: Arc<Database>) -> Self {
        let svc = Self {
            db,
            channels: RwLock::new(HashMap::new()),
            models: RwLock::new(Vec::new()),
            rules: RwLock::new(Vec::new()),
            cache: RwLock::new(HashMap::new()),
        };
        svc.reload();
        svc
    }

    pub fn reload(&self) {
        match self.db.list_channels() {
            Ok(chs) => {
                let map: HashMap<_, _> = chs.into_iter().map(|c| (c.id.clone(), Arc::new(c))).collect();
                *self.channels.write().unwrap_or_else(|e| e.into_inner()) = map;
            }
            Err(e) => tracing::error!("Failed to load channels: {}", e),
        }
        {
            let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
            let mut cache_map = HashMap::new();
            for (id, ch) in chs.iter() {
                let endpoints: Vec<EndpointConfig> = ch.endpoints.iter()
                    .map(|ep| EndpointConfig {
                        id: ep.id,
                        url: ep.url.clone(),
                        api_key: ep.api_key.clone(),
                        weight: ep.weight,
                        timeout_secs: ep.timeout_secs,
                        enabled: ep.enabled,
                    })
                    .collect();
                cache_map.insert(id.clone(), (ch.provider.clone(), Arc::new(LoadBalancer::new(&endpoints)), endpoints));
            }
            *self.cache.write().unwrap_or_else(|e| e.into_inner()) = cache_map;
        }
        match self.db.list_models() {
            Ok(ms) => *self.models.write().unwrap_or_else(|e| e.into_inner()) = ms,
            Err(e) => tracing::error!("Failed to load models: {}", e),
        }
        match self.db.list_rules() {
            Ok(rs) => *self.rules.write().unwrap_or_else(|e| e.into_inner()) = rs,
            Err(e) => tracing::error!("Failed to load routing rules: {}", e),
        }
    }

    pub fn get_channel(&self, id: &str) -> Option<Channel> {
        self.channels.read().unwrap_or_else(|e| e.into_inner()).get(id).map(|c| c.as_ref().clone())
    }

    #[allow(dead_code)]
    pub fn get_enabled_channel(&self, id: &str) -> Option<Channel> {
        self.channels.read().unwrap_or_else(|e| e.into_inner()).get(id)
            .filter(|c| c.enabled)
            .map(|c| c.as_ref().clone())
    }

    /// Resolve a channel_id to its provider adapter name and endpoint configs.
    pub fn resolve_channel(&self, channel_id: &str) -> Option<(String, Vec<EndpointConfig>)> {
        let ch = self.channels.read().unwrap_or_else(|e| e.into_inner()).get(channel_id)?.clone(); // Arc clone, cheap
        if !ch.enabled {
            return None;
        }
        let endpoints: Vec<EndpointConfig> = ch
            .endpoints
            .iter()
            .map(|ep| EndpointConfig {
                id: ep.id,
                url: ep.url.clone(),
                api_key: ep.api_key.clone(),
                weight: ep.weight,
                timeout_secs: ep.timeout_secs,
                enabled: ep.enabled,
            })
            .collect();
        Some((ch.provider.clone(), endpoints))
    }

    pub fn get_route(&self, channel_id: &str) -> Option<RouteCacheEntry> {
        self.cache.read().ok()?.get(channel_id).cloned()
    }

    /// Find an endpoint by DB id and update its enabled state in the circuit breaker.
    pub fn set_endpoint_enabled(&self, endpoint_id: i64, enabled: bool) {
        let cache = self.cache.read().unwrap_or_else(|e| e.into_inner());
        for (_, (_, balancer, endpoints)) in cache.iter() {
            for (i, ep) in endpoints.iter().enumerate() {
                if ep.id == Some(endpoint_id) {
                    balancer.as_health_aware().breakers()[i].set_enabled(enabled);
                    return;
                }
            }
        }
    }

    /// Collect health status for all endpoints in a channel.
    pub fn channel_health(&self, channel_id: &str) -> Vec<(i64, bool, bool)> {
        let cache = self.cache.read().unwrap_or_else(|e| e.into_inner());
        if let Some((_, balancer, endpoints)) = cache.get(channel_id) {
            let balancer = balancer.as_health_aware();
            endpoints
                .iter()
                .enumerate()
                .filter_map(|(i, ep)| {
                    ep.id.map(|id| (id, balancer.breakers()[i].is_enabled(), balancer.breakers()[i].is_available()))
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Route a model to a channel ID for the given user.
    /// Return models in a format suitable for the /v1/models endpoint.
    pub fn list_display_models(&self) -> Vec<serde_json::Value> {
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        models
            .iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.name,
                    "type": "model",
                    "display_name": m.name,
                    "created_at": "2026-01-01T00:00:00Z",
                    "max_input_tokens": m.context_length.unwrap_or(0),
                    "max_tokens": m.context_length.unwrap_or(0),
                    "capabilities": {},
                    "upstream_id": m.id,
                    "model_pattern": m.model_pattern,
                    "category": m.category,
                })
            })
            .collect()
    }

    pub fn route(&self, user_id: &str, model: &str) -> Result<(String, Option<String>), RouteError> {
        // Load subscribed model IDs for this user
        let subscribed: HashSet<String> = self.db.list_subscribed_model_ids(user_id)
            .unwrap_or_default()
            .into_iter()
            .collect();

        // 1. Try model-based routing
        {
            let models = self.models.read().unwrap_or_else(|e| e.into_inner());
            for model_cfg in models.iter() {
                if !subscribed.contains(&model_cfg.id) {
                    continue; // skip models the user isn't subscribed to
                }
                if match_pattern(model, &model_cfg.model_pattern) || (!model_cfg.name.is_empty() && model == model_cfg.name) {
                    let mut bindings: Vec<&crate::domain::model::ModelChannel> =
                        model_cfg.channels.iter().collect();
                    bindings.sort_by_key(|b| b.priority);

                    for binding in &bindings {
                        if let Some(ch) = self.channels.read().unwrap_or_else(|e| e.into_inner()).get(&binding.channel_id) {
                            if ch.enabled {
                                return Ok((ch.id.clone(), Some(model_cfg.id.clone())));
                            }
                        }
                    }
                }
            }
        }

        // 2. Fall back to routing_rules
        {
            let rules = self.rules.read().unwrap_or_else(|e| e.into_inner());
            let mut matched: Vec<(i32, String)> = Vec::new();

            for rule in rules.iter() {
                let user_match = rule.user_id == "*" || rule.user_id == user_id;
                let model_match = match_pattern(model, &rule.model_pattern);

                if user_match && model_match {
                    if let Some(ch) = self.channels.read().unwrap_or_else(|e| e.into_inner()).get(&rule.channel_id) {
                        if ch.enabled {
                            matched.push((ch.priority, ch.id.clone()));
                        }
                    }
                }
            }

            matched.sort_by_key(|(p, _)| *p);

            if let Some((_, id)) = matched.first() {
                return Ok((id.clone(), None));
            }
        }

        Err(RouteError(format!(
            "No route found for user '{}' model '{}'",
            user_id, model
        )))
    }
}

pub fn match_pattern(text: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return text == pattern;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    match parts.len() {
        2 => {
            let prefix = parts[0];
            let suffix = parts[1];
            (prefix.is_empty() || text.starts_with(prefix))
                && (suffix.is_empty() || text.ends_with(suffix))
        }
        3 => {
            let prefix = parts[0];
            let middle = parts[1];
            let suffix = parts[2];
            text.starts_with(prefix) && text.contains(middle) && text.ends_with(suffix)
        }
        _ => pattern == text,
    }
}

#[derive(Debug)]
pub struct RouteError(pub String);

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Route error: {}", self.0)
    }
}
