use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use crate::balancer::{BreakerSnapshot, LoadBalancer};
use crate::config::types::EndpointConfig;

type RouteCacheEntry = (String, Arc<LoadBalancer>);
type RouteCache = RwLock<HashMap<String, RouteCacheEntry>>;
use crate::db::Database;
use crate::domain::channel::Channel;
use crate::domain::model::Model;
use crate::domain::routing::RoutingRule;
use crate::service::endpoint_pool::BindingStatePool;

/// In-memory route cache, rebuilt from DB on startup and after admin changes.
#[derive(Clone)]
pub struct RoutePlan {
    pub channel_id: String,
    pub provider_name: String,
    pub endpoint_idx: usize,
    pub endpoint: EndpointConfig,
    pub balancer: Arc<LoadBalancer>,
}

pub struct RoutingService {
    db: Arc<Database>,
    channels: RwLock<HashMap<String, Arc<Channel>>>,
    models: RwLock<Vec<Model>>,
    rules: RwLock<Vec<RoutingRule>>,
    cache: RouteCache,
    /// Model-binding endpoint state, kept across configuration reloads.
    binding_pool: Arc<BindingStatePool>,
    /// Independent persistent encryption key used for endpoint credentials.
    enc_key: String,
    /// Atomic counter for round-robin channel selection across same-named models.
    zone_counter: AtomicU64,
    /// Serializes snapshot swaps with route reads and endpoint toggles.
    snapshot_lock: RwLock<()>,
    /// Public routing is disabled before the first complete snapshot is loaded.
    public_snapshot_valid: AtomicBool,
}

#[derive(Clone, Copy)]
enum RouteVisibility {
    Public,
    Internal,
}

fn is_published_model(models: &[Model], model_name: &str) -> bool {
    models
        .iter()
        .any(|configured| configured.name == model_name && configured.published)
}

fn channel_is_routable(
    channels: &HashMap<String, Arc<Channel>>,
    cache: &HashMap<String, RouteCacheEntry>,
    channel_id: &str,
) -> bool {
    channels.get(channel_id).is_some_and(|channel| {
        channel.enabled
            && cache
                .get(channel_id)
                .is_some_and(|(_, balancer)| balancer.as_health_aware().has_available_endpoint())
    })
}

fn models_for_display(models: &[Model]) -> Vec<serde_json::Value> {
    let mut seen: HashSet<String> = HashSet::new();
    models
        .iter()
        .filter(|model| model.published && seen.insert(model.name.clone()))
        .map(|model| {
            serde_json::json!({
                "id": model.name,
                "type": "model",
                "display_name": model.name,
                "created_at": "2026-01-01T00:00:00Z",
                "max_input_tokens": model.context_length.unwrap_or(0),
                "max_tokens": model.context_length.unwrap_or(0),
                "capabilities": {},
                "upstream_id": model.id,
                "category": model.category,
            })
        })
        .collect()
}

impl RoutingService {
    pub async fn new(db: Arc<Database>, enc_key: &str) -> Result<Self, String> {
        let svc = Self {
            db,
            channels: RwLock::new(HashMap::new()),
            models: RwLock::new(Vec::new()),
            rules: RwLock::new(Vec::new()),
            cache: RwLock::new(HashMap::new()),
            binding_pool: Arc::new(BindingStatePool::new()),
            enc_key: enc_key.to_string(),
            zone_counter: AtomicU64::new(0),
            snapshot_lock: RwLock::new(()),
            public_snapshot_valid: AtomicBool::new(false),
        };
        svc.reload().await?;
        Ok(svc)
    }

    pub async fn reload(&self) -> Result<(), String> {
        // Load circuit breaker params from settings before rebuilding the
        // channel balancers, so a settings change takes effect on reload.
        {
            let threshold = self
                .db
                .get_setting("breaker_threshold")
                .await
                .ok()
                .flatten()
                .and_then(|v| v.parse::<u32>().ok());
            let cooldown = self
                .db
                .get_setting("breaker_cooldown_secs")
                .await
                .ok()
                .flatten()
                .and_then(|v| v.parse::<u64>().ok());
            let long_fail = self
                .db
                .get_setting("breaker_long_fail_threshold")
                .await
                .ok()
                .flatten()
                .and_then(|v| v.parse::<u32>().ok());
            let long_interval = self
                .db
                .get_setting("breaker_long_probe_interval_secs")
                .await
                .ok()
                .flatten()
                .and_then(|v| v.parse::<u64>().ok());
            crate::balancer::set_breaker_params(threshold, cooldown, long_fail, long_interval);
        }

        // Build the complete replacement before changing the live routing data.
        let chs = self
            .db
            .list_channels()
            .await
            .map_err(|e| format!("Failed to load channels: {}", e))?;
        let channel_map: HashMap<_, _> = chs
            .into_iter()
            .map(|c| (c.id.clone(), Arc::new(c)))
            .collect();
        let previous_cache = self.cache.read().unwrap_or_else(|e| e.into_inner()).clone();

        let mut cache_map = HashMap::new();
        for (id, ch) in channel_map.iter() {
            let endpoints: Vec<EndpointConfig> =
                ch.endpoints
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
                            full_url: ep.full_url,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
            let balancer = previous_cache
                .get(id)
                .map(|(_, previous)| previous.rebuild_preserving_state(&endpoints))
                .unwrap_or_else(|| LoadBalancer::new(&endpoints));
            cache_map.insert(id.clone(), (ch.provider.clone(), Arc::new(balancer)));
        }

        let model_list = self
            .db
            .list_models()
            .await
            .map_err(|e| format!("Failed to load models: {}", e))?;
        let rule_list = self
            .db
            .list_rules()
            .await
            .map_err(|e| format!("Failed to load routing rules: {}", e))?;

        let _snapshot_guard = self
            .snapshot_lock
            .write()
            .unwrap_or_else(|e| e.into_inner());
        self.binding_pool.reconcile(&model_list, &cache_map);
        *self.channels.write().unwrap_or_else(|e| e.into_inner()) = channel_map;
        *self.cache.write().unwrap_or_else(|e| e.into_inner()) = cache_map;
        *self.models.write().unwrap_or_else(|e| e.into_inner()) = model_list;
        *self.rules.write().unwrap_or_else(|e| e.into_inner()) = rule_list;
        self.public_snapshot_valid.store(true, Ordering::Release);
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
                    full_url: ep.full_url,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some((ch.provider.clone(), endpoints)))
    }

    pub fn get_route(&self, channel_id: &str) -> Option<RouteCacheEntry> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        self.cache.read().ok()?.get(channel_id).cloned()
    }

    pub fn get_binding_route(&self, model_id: &str, channel_id: &str) -> Option<Arc<LoadBalancer>> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        self.binding_pool.get(model_id, channel_id)
    }

    pub fn models_snapshot(&self) -> Vec<Model> {
        self.models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn has_model_binding_for_upstream(
        &self,
        model: &str,
        channel_id: &str,
        upstream_model: Option<&str>,
    ) -> bool {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        self.models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|configured| {
                configured.name == model
                    && configured.channels.iter().any(|binding| {
                        binding.channel_id == channel_id
                            && upstream_model.is_none_or(|expected| {
                                binding
                                    .upstream_model
                                    .as_deref()
                                    .unwrap_or(&configured.name)
                                    == expected
                            })
                    })
            })
    }

    pub fn has_model_binding(&self, model: &str, channel_id: &str) -> bool {
        self.models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|configured| {
                configured.name == model
                    && configured
                        .channels
                        .iter()
                        .any(|binding| binding.channel_id == channel_id)
            })
    }

    /// Select a healthy endpoint for the channel already chosen by the model
    /// routing rules. Open endpoints are not promoted by business traffic.
    pub fn route_model_binding_for_channel(
        &self,
        model: &str,
        channel_id: &str,
        attempted: &[(String, i64)],
    ) -> Result<RoutePlan, RouteError> {
        self.route_model_binding_for_channel_and_upstream(model, channel_id, None, attempted)
    }

    /// Select a healthy model-binding endpoint across all eligible channels.
    /// This is used when a channel selected by a routing rule becomes unhealthy
    /// between channel selection and endpoint resolution.
    pub fn route_model_binding_for_model(
        &self,
        model: &str,
        upstream_model: Option<&str>,
        attempted: &[(String, i64)],
    ) -> Result<RoutePlan, RouteError> {
        self.route_model_binding_for_model_excluding_channels(
            model,
            upstream_model,
            attempted,
            &HashSet::new(),
        )
    }

    pub fn route_model_binding_for_model_excluding_channels(
        &self,
        model: &str,
        upstream_model: Option<&str>,
        attempted: &[(String, i64)],
        excluded_channels: &HashSet<String>,
    ) -> Result<RoutePlan, RouteError> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        let channels = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let mut candidates: Vec<(i32, String, String, Arc<LoadBalancer>)> = models
            .iter()
            .filter(|configured| configured.name == model && configured.published)
            .flat_map(|configured| {
                configured.channels.iter().filter_map(|binding| {
                    let channel_enabled = !excluded_channels.contains(&binding.channel_id)
                        && channels
                            .get(&binding.channel_id)
                            .is_some_and(|channel| channel.enabled);
                    let upstream_matches = upstream_model.is_none_or(|expected| {
                        binding
                            .upstream_model
                            .as_deref()
                            .unwrap_or(&configured.name)
                            == expected
                    });
                    if !channel_enabled || !upstream_matches {
                        return None;
                    }
                    self.binding_pool
                        .get(&configured.id, &binding.channel_id)
                        .filter(|balancer| balancer.as_health_aware().has_healthy_endpoint())
                        .and_then(|balancer| {
                            Some((
                                binding.priority,
                                binding.channel_id.clone(),
                                channels.get(&binding.channel_id)?.provider.clone(),
                                balancer,
                            ))
                        })
                })
            })
            .collect();
        candidates.sort_by_key(|(priority, _, _, _)| *priority);
        let Some(best_priority) = candidates.first().map(|(priority, _, _, _)| *priority) else {
            return Err(RouteError::unavailable(format!(
                "No route found for model '{}'",
                model
            )));
        };
        let same_priority: Vec<_> = candidates
            .into_iter()
            .filter(|(priority, _, _, _)| *priority == best_priority)
            .collect();
        let start = self.zone_counter.fetch_add(1, Ordering::Relaxed) as usize;
        for offset in 0..same_priority.len() {
            let (_, channel_id, provider_name, balancer) =
                &same_priority[(start + offset) % same_priority.len()];
            let excluded: HashSet<i64> = attempted
                .iter()
                .filter(|(attempted_channel, _)| attempted_channel == channel_id)
                .map(|(_, endpoint_id)| *endpoint_id)
                .collect();
            if let Some((endpoint_idx, endpoint)) = balancer
                .as_health_aware()
                .select_healthy_excluding(&excluded)
            {
                return Ok(RoutePlan {
                    channel_id: channel_id.clone(),
                    provider_name: provider_name.clone(),
                    endpoint_idx,
                    endpoint: endpoint.clone(),
                    balancer: balancer.clone(),
                });
            }
        }
        Err(RouteError::unavailable(format!(
            "No route found for model '{}'",
            model
        )))
    }

    pub fn route_model_binding_for_channel_and_upstream(
        &self,
        model: &str,
        channel_id: &str,
        upstream_model: Option<&str>,
        attempted: &[(String, i64)],
    ) -> Result<RoutePlan, RouteError> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        let channel_enabled = self
            .channels
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(channel_id)
            .is_some_and(|channel| channel.enabled);
        let (model_cfg, balancer) = models
            .iter()
            .filter(|configured| {
                configured.name == model
                    && configured.published
                    && channel_enabled
                    && configured.channels.iter().any(|binding| {
                        binding.channel_id == channel_id
                            && upstream_model.is_none_or(|expected| {
                                binding
                                    .upstream_model
                                    .as_deref()
                                    .unwrap_or(&configured.name)
                                    == expected
                            })
                    })
            })
            .find_map(|configured| {
                self.binding_pool
                    .get(&configured.id, channel_id)
                    .filter(|balancer| balancer.as_health_aware().has_healthy_endpoint())
                    .map(|balancer| (configured, balancer))
            })
            .ok_or_else(|| {
                RouteError::not_found(format!("No route found for model '{}'", model))
            })?;
        let excluded: HashSet<i64> = attempted
            .iter()
            .filter(|(channel, _)| channel == channel_id)
            .map(|(_, endpoint)| *endpoint)
            .collect();
        let (endpoint_idx, endpoint) = balancer
            .as_health_aware()
            .select_healthy_excluding(&excluded)
            .ok_or_else(|| RouteError::unavailable("No available endpoints"))?;
        let binding = model_cfg
            .channels
            .iter()
            .find(|binding| binding.channel_id == channel_id)
            .expect("binding was checked above");
        Ok(RoutePlan {
            channel_id: channel_id.to_string(),
            provider_name: binding.provider.clone(),
            endpoint_idx,
            endpoint: endpoint.clone(),
            balancer,
        })
    }

    /// Find an endpoint by DB id and update its enabled state in the circuit breaker.
    pub fn set_endpoint_enabled(&self, endpoint_id: i64, enabled: bool) {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let cache = self.cache.read().unwrap_or_else(|e| e.into_inner());
        self.binding_pool.set_endpoint_enabled(endpoint_id, enabled);
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

    /// Mirror a probe result to the binding pool and the per-channel cache
    /// balancer for the given endpoint, identified by DB id (preferred) or URL.
    /// Used after manual/automatic probes so that `channel_health()` (read by
    /// the model console health dot) reflects the real endpoint status.
    ///
    /// Success is a physical fact about the endpoint and resets every model
    /// binding sharing it. Failure of a manual probe is model-scoped: it only
    /// opens the breaker of the model under test, never other models that bind
    /// the same endpoint, and never trips the shared channel breaker.
    pub fn record_endpoint_health(
        &self,
        model_id: &str,
        channel_id: &str,
        endpoint_id: Option<i64>,
        endpoint_url: &str,
        success: bool,
    ) {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let cache = self.cache.read().unwrap_or_else(|e| e.into_inner());

        if success {
            // Physical recovery: every model binding on this endpoint may reset.
            if let Some(eid) = endpoint_id {
                self.binding_pool.record_endpoint_health(eid, true);
            }
        } else if let Some(eid) = endpoint_id {
            // Model-scoped failure: only the model under test is affected,
            // never other models sharing the endpoint.
            self.binding_pool
                .record_endpoint_health_for_model(model_id, channel_id, eid, false);
        }

        // Channel cache — try endpoint_id first, then URL. Only success is
        // mirrored: a model-scoped probe failure must not trip the shared
        // channel breaker for every model routed through it.
        if let Some(ch) = chs.get(channel_id) {
            if let Some((_, balancer)) = cache.get(channel_id) {
                if let Some(idx) = ch.endpoints.iter().position(|ep| {
                    endpoint_id.is_some_and(|eid| ep.id == Some(eid))
                        || (endpoint_id.is_none() && ep.url == endpoint_url)
                }) {
                    if success {
                        balancer.as_health_aware().record_success(idx);
                    }
                }
            }
        }
    }

    /// Collect all binding-level breaker snapshots for persistence.
    /// Returns Vec<(model_id, channel_id, [(endpoint_id, snapshot)])>.
    pub fn all_breaker_snapshots(&self) -> Vec<(String, String, Vec<(i64, BreakerSnapshot)>)> {
        self.binding_pool.all_snapshots()
    }

    /// Restore binding-level breaker snapshots (called on startup after reload).
    pub fn restore_breaker_snapshots(
        &self,
        snapshots: &[(String, String, Vec<(i64, BreakerSnapshot)>)],
    ) {
        self.binding_pool.restore_snapshots(snapshots);
    }

    /// Aggregated endpoint health for a channel, across all **published**
    /// model bindings that use it. Unlike the channel-level balancer (which
    /// business traffic doesn't update), this reflects the real binding-level
    /// circuit breakers — the ones routing actually consults.
    ///
    /// Returns `(endpoint_id, enabled, healthy_bindings, total_bindings,
    /// long_unavailable)` per endpoint.
    pub fn channel_health_aggregated(&self, channel_id: &str) -> Vec<(i64, bool, u32, u32, bool)> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        let published_ids: HashSet<&str> = models
            .iter()
            .filter(|m| m.published)
            .map(|m| m.id.as_str())
            .collect();

        // endpoint_id -> (healthy_bindings, total_bindings, any_long_unavailable)
        let mut agg: HashMap<i64, (u32, u32, bool)> = HashMap::new();
        for (key, balancer) in self.binding_pool.iter() {
            if key.channel_id != channel_id || !published_ids.contains(key.model_id.as_str()) {
                continue;
            }
            let health = balancer.as_health_aware();
            for (i, ep) in health.endpoints().iter().enumerate() {
                if let Some(id) = ep.id {
                    let entry = agg.entry(id).or_insert((0, 0, false));
                    entry.1 += 1;
                    if health.breakers()[i].is_healthy() {
                        entry.0 += 1;
                    }
                    if health.breakers()[i].long_unavailable() {
                        entry.2 = true;
                    }
                }
            }
        }

        let Some(ch) = chs.get(channel_id) else {
            return Vec::new();
        };
        ch.endpoints
            .iter()
            .filter_map(|ep| {
                ep.id.map(|id| {
                    let (healthy, total, long) = agg.get(&id).copied().unwrap_or((0, 0, false));
                    (id, ep.enabled, healthy, total, long)
                })
            })
            .collect()
    }

    /// Collect health status for all endpoints in a channel (channel-level
    /// balancer only; kept for the flow-control console).
    pub fn channel_health(&self, channel_id: &str) -> Vec<(i64, bool, bool)> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
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
                                balancer.breakers()[i].is_healthy(),
                            )
                        })
                    })
                    .collect();
            }
        }
        Vec::new()
    }

    /// Global live endpoint health across all channels, aggregated over all
    /// **published** model bindings. This is the real state business routing
    /// consults (binding_pool), not the channel-level balancer.
    ///
    /// Returns per-endpoint: (endpoint_id, enabled, healthy_bindings,
    /// total_bindings, long_unavailable).
    pub fn all_endpoints_live_health(
        &self,
    ) -> Vec<(i64, bool, u32, u32, bool)> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        let published_ids: HashSet<&str> = models
            .iter()
            .filter(|m| m.published)
            .map(|m| m.id.as_str())
            .collect();

        // endpoint_id -> (healthy_bindings, total_bindings, any_long_unavailable)
        let mut agg: HashMap<i64, (u32, u32, bool)> = HashMap::new();
        for (key, balancer) in self.binding_pool.iter() {
            if !published_ids.contains(key.model_id.as_str()) {
                continue;
            }
            let health = balancer.as_health_aware();
            for (i, ep) in health.endpoints().iter().enumerate() {
                if let Some(id) = ep.id {
                    let entry = agg.entry(id).or_insert((0, 0, false));
                    entry.1 += 1;
                    if health.breakers()[i].is_healthy() {
                        entry.0 += 1;
                    }
                    if health.breakers()[i].long_unavailable() {
                        entry.2 = true;
                    }
                }
            }
        }

        let mut out = Vec::new();
        for ch in chs.values() {
            for ep in &ch.endpoints {
                if let Some(id) = ep.id {
                    let (healthy, total, long) = agg.get(&id).copied().unwrap_or((0, 0, false));
                    out.push((id, ep.enabled, healthy, total, long));
                }
            }
        }
        out
    }
    /// Same-named models are merged into one entry (they share the "id" field).
    pub fn list_display_models(&self) -> Vec<serde_json::Value> {
        self.list_display_models_for(None)
    }

    pub fn list_display_models_for(
        &self,
        allowed_models: Option<&[String]>,
    ) -> Vec<serde_json::Value> {
        if !self.public_snapshot_valid.load(Ordering::Acquire) {
            return Vec::new();
        }
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        if !self.public_snapshot_valid.load(Ordering::Acquire) {
            return Vec::new();
        }
        let filtered: Vec<Model> = models
            .iter()
            .filter(|model| {
                allowed_models.is_none_or(|allowed| allowed.iter().any(|name| name == &model.name))
            })
            .cloned()
            .collect();
        models_for_display(&filtered)
    }

    /// Route a public data-plane request. Only published logical models may be used.
    pub async fn route_public(
        &self,
        user_id: &str,
        model: &str,
        team_id: Option<&str>,
    ) -> Result<(String, String, Option<String>), RouteError> {
        if !self.public_snapshot_valid.load(Ordering::Acquire) {
            return Err(RouteError::unavailable(
                "No route found for requested model",
            ));
        }
        self.route_with_visibility(user_id, model, team_id, RouteVisibility::Public)
            .await
    }

    /// Route a trusted internal request, retaining access to all configured models.
    pub async fn route_internal(
        &self,
        user_id: &str,
        model: &str,
        team_id: Option<&str>,
    ) -> Result<(String, String, Option<String>), RouteError> {
        self.route_with_visibility(user_id, model, team_id, RouteVisibility::Internal)
            .await
    }

    /// Backward-compatible public routing entry point.
    /// New internal callers should use `route_internal` explicitly.
    pub async fn route(
        &self,
        user_id: &str,
        model: &str,
        team_id: Option<&str>,
    ) -> Result<(String, String, Option<String>), RouteError> {
        self.route_public(user_id, model, team_id).await
    }

    /// Returns (channel_id, resolved_model_name, upstream_model_override).
    /// `team_id` is the active team of the request (None for personal accounts);
    /// user rules scoped to that team apply.
    async fn route_with_visibility(
        &self,
        user_id: &str,
        model: &str,
        team_id: Option<&str>,
        visibility: RouteVisibility,
    ) -> Result<(String, String, Option<String>), RouteError> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let mut model_name = model.to_string();
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let cache = self.cache.read().unwrap_or_else(|e| e.into_inner());
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        let rules = self.rules.read().unwrap_or_else(|e| e.into_inner());

        // Step 1: User/team-level rules — model name rewrite (self-service)
        // Exact match on source_model, rewrite to target_model if set.
        // A rule is either personal (team_id None, matched by user_id) or
        // team-scoped (team_id Some, matched by the request's active team).
        for rule in rules.iter() {
            if rule.scope != "user" || !rule.enabled {
                continue;
            }
            match &rule.team_id {
                Some(rid) => {
                    // Team-scoped rule: apply only when the request carries
                    // the same active team.
                    if team_id != Some(rid.as_str()) {
                        continue;
                    }
                }
                None => {
                    // Personal rule: apply only to the owning user.
                    if rule.user_id != user_id {
                        continue;
                    }
                }
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
                if rule.channel_id.is_empty() {
                    continue;
                }
                if matches!(visibility, RouteVisibility::Public)
                    && !is_published_model(&models, &model_name)
                {
                    return Err(RouteError::not_found(format!(
                        "No route found for model '{}'",
                        model
                    )));
                }
                let rule_routable = if matches!(visibility, RouteVisibility::Public) {
                    models.iter().any(|model_cfg| {
                        model_cfg.name == model_name
                            && model_cfg.published
                            && model_cfg.channels.iter().any(|binding| {
                                binding.channel_id == rule.channel_id
                                    && chs
                                        .get(&rule.channel_id)
                                        .is_some_and(|channel| channel.enabled)
                                    && self
                                        .binding_pool
                                        .has_healthy(&model_cfg.id, &rule.channel_id)
                                    && (rule.upstream_model.is_empty()
                                        || binding
                                            .upstream_model
                                            .as_deref()
                                            .unwrap_or(&model_cfg.name)
                                            == rule.upstream_model)
                            })
                    })
                } else {
                    channel_is_routable(&chs, &cache, &rule.channel_id)
                };
                if !rule_routable {
                    continue;
                }
                // A system rule may pin only the channel. In that case keep
                // the binding's upstream model instead of silently sending the
                // logical display name to an upstream that expects an alias.
                let upstream = if !rule.upstream_model.is_empty() {
                    Some(rule.upstream_model.clone())
                } else {
                    models.iter().find_map(|model_cfg| {
                        if model_cfg.name != model_name || !model_cfg.published {
                            return None;
                        }
                        model_cfg.channels.iter().find_map(|binding| {
                            if binding.channel_id == rule.channel_id {
                                binding.upstream_model.clone()
                            } else {
                                None
                            }
                        })
                    })
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

            // System rules with no channel_id: apply target_model rewrite only
            for (_priority, rule) in &matched {
                if rule.channel_id.is_empty() && !rule.target_model.is_empty() {
                    model_name = rule.target_model.clone();
                    if matches!(visibility, RouteVisibility::Public)
                        && !is_published_model(&models, &model_name)
                    {
                        return Err(RouteError::not_found(format!(
                            "No route found for model '{}'",
                            model
                        )));
                    }
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

        if matches!(visibility, RouteVisibility::Public)
            && !is_published_model(&models, &model_name)
        {
            return Err(RouteError::not_found(format!(
                "No route found for model '{}'",
                model
            )));
        }

        // Step 3: Model console — exact name match only (no glob matching)
        {
            let mut candidates: Vec<(i32, String, String)> = Vec::new();

            for model_cfg in models.iter() {
                if model_cfg.name != model_name
                    || (matches!(visibility, RouteVisibility::Public) && !model_cfg.published)
                {
                    continue;
                }
                for binding in &model_cfg.channels {
                    let routable = if matches!(visibility, RouteVisibility::Public) {
                        chs.get(&binding.channel_id)
                            .is_some_and(|channel| channel.enabled)
                            && self
                                .binding_pool
                                .has_healthy(&model_cfg.id, &binding.channel_id)
                    } else {
                        channel_is_routable(&chs, &cache, &binding.channel_id)
                    };
                    if !routable {
                        continue;
                    }
                    let upstream = binding
                        .upstream_model
                        .clone()
                        .unwrap_or(model_cfg.name.clone());
                    candidates.push((binding.priority, binding.channel_id.clone(), upstream));
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

        let binding_count = models
            .iter()
            .filter(|configured| configured.name == model_name)
            .map(|configured| configured.channels.len())
            .sum::<usize>();
        let enabled_binding_count = models
            .iter()
            .filter(|configured| configured.name == model_name)
            .flat_map(|configured| configured.channels.iter())
            .filter(|binding| {
                chs.get(&binding.channel_id)
                    .is_some_and(|channel| channel.enabled)
            })
            .count();
        let healthy_binding_count = models
            .iter()
            .filter(|configured| configured.name == model_name)
            .flat_map(|configured| {
                configured
                    .channels
                    .iter()
                    .map(move |binding| (configured.id.as_str(), binding))
            })
            .filter(|(model_id, binding)| {
                self.binding_pool.has_healthy(model_id, &binding.channel_id)
            })
            .count();
        tracing::warn!(
            user_id,
            requested_model = model,
            resolved_model = %model_name,
            binding_count,
            enabled_binding_count,
            healthy_binding_count,
            "No routable model binding"
        );
        Err(RouteError::unavailable(format!(
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

/// Whether a route failure should be reported as 503 (temporarily
/// unavailable) or 404 (no such model/route).
///
/// A model that exists but has no currently-healthy binding (circuit breaker
/// open, all endpoints failing, snapshot not yet loaded) is *unavailable*, not
/// *missing*. Returning 404 makes clients like Claude Code conclude the model
/// doesn't exist. 503 keeps the model identity valid and lets the client retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteErrorKind {
    /// Model/route genuinely not found — 404.
    NotFound,
    /// Model exists but nothing healthy right now — 503.
    Unavailable,
}

#[derive(Debug)]
pub struct RouteError {
    pub kind: RouteErrorKind,
    pub message: String,
}

impl RouteError {
    pub fn new(message: impl Into<String>, kind: RouteErrorKind) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(message, RouteErrorKind::NotFound)
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(message, RouteErrorKind::Unavailable)
    }
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Route error: {}", self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, name: &str, published: bool) -> Model {
        Model {
            id: id.to_string(),
            name: name.to_string(),
            model_pattern: name.to_string(),
            pricing: Default::default(),
            channels: Vec::new(),
            published,
            context_length: None,
            category: String::new(),
        }
    }

    #[test]
    fn display_models_exclude_unpublished_models() {
        let models = vec![
            model("hidden", "hidden-model", false),
            model("visible", "visible-model", true),
        ];

        let displayed = models_for_display(&models);
        let ids: Vec<&str> = displayed
            .iter()
            .filter_map(|entry| entry.get("id").and_then(|id| id.as_str()))
            .collect();

        assert_eq!(ids, vec!["visible-model"]);
    }

    #[test]
    fn published_duplicate_is_not_hidden_by_unpublished_model() {
        let models = vec![
            model("hidden", "shared-model", false),
            model("visible", "shared-model", true),
        ];

        let displayed = models_for_display(&models);

        assert_eq!(displayed.len(), 1);
        assert_eq!(displayed[0]["id"], "shared-model");
        assert_eq!(displayed[0]["upstream_id"], "visible");
    }

    #[test]
    fn publication_check_uses_logical_model_name() {
        let models = vec![model("model-id", "published-model", true)];

        assert!(is_published_model(&models, "published-model"));
        assert!(!is_published_model(&models, "model-id"));
        assert!(!is_published_model(&models, "unpublished-model"));
    }
}
