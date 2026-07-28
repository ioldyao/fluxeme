use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use crate::balancer::LoadBalancer;
use crate::config::types::EndpointConfig;

type RouteCacheEntry = (String, Arc<LoadBalancer>);
type RouteCache = RwLock<HashMap<String, RouteCacheEntry>>;
use crate::db::Database;
use crate::domain::channel::Channel;
use crate::domain::model::Model;
use crate::domain::routing::RoutingRule;

/// In-memory route cache, rebuilt from DB on startup and after admin changes.
pub struct RoutingService {
    db: Arc<Database>,
    channels: RwLock<HashMap<String, Arc<Channel>>>,
    models: RwLock<Vec<Model>>,
    rules: RwLock<Vec<RoutingRule>>,
    cache: RouteCache,
    /// Independent persistent encryption key used for endpoint credentials.
    enc_key: String,
    /// Atomic counter for round-robin channel selection across same-named models.
    zone_counter: AtomicU64,
}

impl RoutingService {
    pub async fn new(db: Arc<Database>, enc_key: &str) -> Result<Self, String> {
        let svc = Self {
            db,
            channels: RwLock::new(HashMap::new()),
            models: RwLock::new(Vec::new()),
            rules: RwLock::new(Vec::new()),
            cache: RwLock::new(HashMap::new()),
            enc_key: enc_key.to_string(),
            zone_counter: AtomicU64::new(0),
        };
        svc.reload().await?;
        Ok(svc)
    }

    pub async fn reload(&self) -> Result<(), String> {
        match self.db.list_channels().await {
            Ok(chs) => {
                let map: HashMap<_, _> = chs
                    .into_iter()
                    .map(|c| (c.id.clone(), Arc::new(c)))
                    .collect();
                *self.channels.write().unwrap_or_else(|e| e.into_inner()) = map;
            }
            Err(e) => tracing::error!("Failed to load channels: {}", e),
        }
        {
            let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
            let mut cache_map = HashMap::new();
            for (id, ch) in chs.iter() {
                let endpoints: Vec<EndpointConfig> = ch
                    .endpoints
                    .iter()
                    .map(|ep| {
                        Ok(EndpointConfig {
                            id: ep.id,
                            url: ep.url.clone(),
                            api_key: crate::crypto::decrypt_load(&ep.api_key, &self.enc_key)
                                .map_err(|e| {
                                    format!(
                                    "failed to decrypt API key for channel '{}' endpoint {:?}: {}",
                                    id, ep.id, e
                                )
                                })?,
                            weight: ep.weight,
                            timeout_secs: ep.timeout_secs,
                            enabled: ep.enabled,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                cache_map.insert(
                    id.clone(),
                    (ch.provider.clone(), Arc::new(LoadBalancer::new(&endpoints))),
                );
            }
            *self.cache.write().unwrap_or_else(|e| e.into_inner()) = cache_map;
        }
        match self.db.list_models().await {
            Ok(ms) => *self.models.write().unwrap_or_else(|e| e.into_inner()) = ms,
            Err(e) => tracing::error!("Failed to load models: {}", e),
        }
        match self.db.list_rules().await {
            Ok(rs) => *self.rules.write().unwrap_or_else(|e| e.into_inner()) = rs,
            Err(e) => tracing::error!("Failed to load routing rules: {}", e),
        }
        Ok(())
    }

    pub fn get_channel(&self, id: &str) -> Option<Channel> {
        self.channels
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .map(|c| c.as_ref().clone())
    }

    #[allow(dead_code)]
    pub fn get_enabled_channel(&self, id: &str) -> Option<Channel> {
        self.channels
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .filter(|c| c.enabled)
            .map(|c| c.as_ref().clone())
    }

    /// Resolve a channel_id to its provider adapter name and endpoint configs.
    #[allow(dead_code)]
    pub fn resolve_channel(
        &self,
        channel_id: &str,
    ) -> Result<Option<(String, Vec<EndpointConfig>)>, String> {
        let Some(ch) = self
            .channels
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(channel_id)
            .cloned()
        else {
            return Ok(None);
        };
        if !ch.enabled {
            return Ok(None);
        }
        let endpoints: Vec<EndpointConfig> = ch
            .endpoints
            .iter()
            .map(|ep| {
                Ok(EndpointConfig {
                    id: ep.id,
                    url: ep.url.clone(),
                    api_key: crate::crypto::decrypt_load(&ep.api_key, &self.enc_key).map_err(
                        |e| {
                            format!(
                                "failed to decrypt API key for channel '{}' endpoint {:?}: {}",
                                channel_id, ep.id, e
                            )
                        },
                    )?,
                    weight: ep.weight,
                    timeout_secs: ep.timeout_secs,
                    enabled: ep.enabled,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some((ch.provider.clone(), endpoints)))
    }

    pub fn get_route(&self, channel_id: &str) -> Option<RouteCacheEntry> {
        self.cache.read().ok()?.get(channel_id).cloned()
    }

    /// Find an endpoint by DB id and update its enabled state in the circuit breaker.
    pub fn set_endpoint_enabled(&self, endpoint_id: i64, enabled: bool) {
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let cache = self.cache.read().unwrap_or_else(|e| e.into_inner());
        for (_, ch) in chs.iter() {
            for (i, ep) in ch.endpoints.iter().enumerate() {
                if ep.id == Some(endpoint_id) {
                    if let Some((_, balancer)) = cache.get(&ch.id) {
                        balancer.as_health_aware().breakers()[i].set_enabled(enabled);
                    }
                    return;
                }
            }
        }
    }

    /// Collect health status for all endpoints in a channel.
    pub fn channel_health(&self, channel_id: &str) -> Vec<(i64, bool, bool)> {
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let cache = self.cache.read().unwrap_or_else(|e| e.into_inner());
        if let Some(ch) = chs.get(channel_id) {
            if let Some((_, balancer)) = cache.get(channel_id) {
                let balancer = balancer.as_health_aware();
                return ch
                    .endpoints
                    .iter()
                    .enumerate()
                    .filter_map(|(i, ep)| {
                        ep.id.map(|id| {
                            (
                                id,
                                balancer.breakers()[i].is_enabled(),
                                balancer.breakers()[i].is_available(),
                            )
                        })
                    })
                    .collect();
            }
        }
        Vec::new()
    }

    /// Route a model to a channel ID for the given user.
    /// Return models in a format suitable for the /v1/models endpoint.
    /// Same-named models are merged into one entry (they share the "id" field).
    pub fn list_display_models(&self) -> Vec<serde_json::Value> {
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        let mut seen: HashSet<String> = HashSet::new();
        models
            .iter()
            .filter(|m| seen.insert(m.name.clone()))
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
                    "category": m.category,
                })
            })
            .collect()
    }

    /// Returns (channel_id, resolved_model_name, upstream_model_override).
    pub async fn route(
        &self,
        user_id: &str,
        model: &str,
    ) -> Result<(String, String, Option<String>), RouteError> {
        let mut model_name = model.to_string();
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let rules = self.rules.read().unwrap_or_else(|e| e.into_inner());

        // Step 1: User-level rules — model name rewrite (self-service)
        // Exact match on source_model, rewrite to target_model if set.
        for rule in rules.iter() {
            if rule.scope != "user" || rule.user_id != user_id || !rule.enabled {
                continue;
            }
            if !rule.target_model.is_empty() && match_pattern(&model_name, &rule.source_model) {
                model_name = rule.target_model.clone();
                tracing::info!(
                    user_id,
                    original = model,
                    rewritten = &model_name,
                    rule = rule.name,
                    "User routing rule applied"
                );
                break; // first matching user rule wins
            }
        }

        // Step 2: System-level rules — admin-configured routing overrides
        // Match by user_id + source_model (glob), can set channel + upstream.
        {
            let mut matched: Vec<(i32, &RoutingRule)> = Vec::new();

            for rule in rules.iter() {
                if rule.scope != "system" || !rule.enabled {
                    continue;
                }
                let user_match = rule.user_id == "*" || rule.user_id == user_id;
                if !user_match {
                    continue;
                }
                let model_match = if rule.source_model.is_empty() || rule.source_model == "*" {
                    true
                } else {
                    match_pattern(&model_name, &rule.source_model)
                };
                if !model_match {
                    continue;
                }
                matched.push((rule.priority, rule));
            }

            // Sort by priority (lower = higher priority)
            matched.sort_by_key(|(p, _)| *p);

            for (_priority, rule) in &matched {
                if !rule.channel_id.is_empty() {
                    if let Some(ch) = chs.get(&rule.channel_id) {
                        if ch.enabled {
                            let upstream = if !rule.upstream_model.is_empty() {
                                Some(rule.upstream_model.clone())
                            } else {
                                None
                            };
                            tracing::info!(
                                user_id,
                                model = &model_name,
                                rule = rule.name,
                                channel = &rule.channel_id,
                                "System routing rule matched"
                            );
                            return Ok((rule.channel_id.clone(), model_name.clone(), upstream));
                        }
                    }
                }
            }

            // System rules with no channel_id: apply target_model rewrite only
            for (_priority, rule) in &matched {
                if rule.channel_id.is_empty() && !rule.target_model.is_empty() {
                    model_name = rule.target_model.clone();
                    tracing::info!(
                        user_id,
                        original = &model_name,
                        rule = rule.name,
                        "System rule rewrote model name"
                    );
                    break;
                }
            }
        }

        // Step 3: Model console — exact name match only (no glob matching)
        {
            let models = self.models.read().unwrap_or_else(|e| e.into_inner());
            let mut candidates: Vec<(i32, String, String)> = Vec::new();

            for model_cfg in models.iter() {
                if model_cfg.name != model_name {
                    continue;
                }
                for binding in &model_cfg.channels {
                    if let Some(ch) = chs.get(&binding.channel_id) {
                        if ch.enabled {
                            let upstream = binding
                                .upstream_model
                                .clone()
                                .unwrap_or(model_cfg.name.clone());
                            candidates.push((
                                binding.priority,
                                binding.channel_id.clone(),
                                upstream,
                            ));
                        }
                    }
                }
            }

            if !candidates.is_empty() {
                candidates.sort_by_key(|(p, _, _)| *p);
                let best_priority = candidates[0].0;
                let same: Vec<&(i32, String, String)> = candidates
                    .iter()
                    .filter(|(p, _, _)| *p == best_priority)
                    .collect();
                let idx = (self.zone_counter.fetch_add(1, Ordering::Relaxed) as usize) % same.len();
                let (_, ch_id, m_id) = &same[idx];
                return Ok((ch_id.clone(), model_name.clone(), Some(m_id.clone())));
            }
        }

        Err(RouteError(format!(
            "No route found for model '{}'",
            model_name
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
