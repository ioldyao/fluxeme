use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::balancer::{BreakerSnapshot, LoadBalancer};
use crate::config::types::EndpointConfig;

type RouteCacheEntry = (String, Arc<LoadBalancer>);
type RouteCache = RwLock<HashMap<String, RouteCacheEntry>>;
use crate::db::Database;
use crate::domain::channel::Channel;
use crate::domain::model::Model;
use crate::domain::routing::RoutingRule;
use crate::domain::scheduler::SchedulerEndpointPolicy;
use crate::service::endpoint_pool::ModelEndpointPool;

type EndpointPolicyMap = HashMap<(String, String), Vec<SchedulerEndpointPolicy>>;

/// A resolved endpoint selected from the model's flattened endpoint pool.
#[derive(Clone)]
pub struct RoutePlan {
    pub model_id: String,
    pub channel_id: String,
    pub provider_name: String,
    pub upstream_model: Option<String>,
    pub endpoint_idx: usize,
    pub endpoint: EndpointConfig,
    pub runtime: Arc<crate::service::endpoint_pool::ModelEndpointRuntime>,
    pub max_tokens: Option<u32>,
}

/// Route decision before endpoint selection. The scheduler performs a single
/// endpoint selection over the model's flattened pool; `channel_scope` only
/// constrains that set when a system rule pins a channel.
#[derive(Debug, Clone, Default)]
pub struct RouteContext {
    pub resolved_model: String,
    /// Upstream alias constraint from a system rule (None = unconstrained).
    pub upstream_model: Option<String>,
    /// Channel pinned by a system rule (None = all bound endpoints eligible).
    pub channel_scope: Option<String>,
}

pub struct RoutingService {
    db: Arc<Database>,
    channels: RwLock<HashMap<String, Arc<Channel>>>,
    models: RwLock<Vec<Model>>,
    rules: RwLock<Vec<RoutingRule>>,
    cache: RouteCache,
    /// Compiled model → endpoint runtime (single-level endpoint scheduling).
    model_pool: Arc<ModelEndpointPool>,
    /// Per (model_id, channel_id) endpoint scheduler policies (weight, timeout, max_tokens).
    endpoint_policies: RwLock<EndpointPolicyMap>,
    /// Independent persistent encryption key used for endpoint credentials.
    enc_key: String,
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
            model_pool: Arc::new(ModelEndpointPool::new()),
            endpoint_policies: RwLock::new(HashMap::new()),
            enc_key: enc_key.to_string(),
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
            // The channel-level cache balancer is only consulted for
            // availability/probe checks — weighted traffic selection happens in
            // the per-binding pool, where weights come from scheduler
            // endpoint policies. Use defaults here.
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
                            weight: 1,
                            timeout_secs: None,
                            max_tokens: None,
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
        let endpoint_policies = {
            let mut map: EndpointPolicyMap = HashMap::new();
            for p in self
                .db
                .list_scheduler_endpoint_policies()
                .await
                .map_err(|e| format!("Failed to load scheduler endpoint policies: {}", e))?
            {
                map.entry((p.model_id.clone(), p.channel_id.clone()))
                    .or_default()
                    .push(p);
            }
            map
        };

        let _snapshot_guard = self
            .snapshot_lock
            .write()
            .unwrap_or_else(|e| e.into_inner());
        self.model_pool
            .reconcile(&model_list, &cache_map, &channel_map, &endpoint_policies);
        *self.channels.write().unwrap_or_else(|e| e.into_inner()) = channel_map;
        *self.cache.write().unwrap_or_else(|e| e.into_inner()) = cache_map;
        *self.models.write().unwrap_or_else(|e| e.into_inner()) = model_list;
        *self.rules.write().unwrap_or_else(|e| e.into_inner()) = rule_list;
        *self
            .endpoint_policies
            .write()
            .unwrap_or_else(|e| e.into_inner()) = endpoint_policies;
        self.public_snapshot_valid.store(true, Ordering::Release);
        Ok(())
    }

    /// Snapshot of endpoint scheduler policies keyed by (model, channel).
    pub fn endpoint_policies_snapshot(&self) -> EndpointPolicyMap {
        self.endpoint_policies
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Compiled endpoint runtime for a model (single-level scheduling pool).
    pub fn get_model_endpoint_runtime(
        &self,
        model_id: &str,
    ) -> Option<Arc<crate::service::endpoint_pool::ModelEndpointRuntime>> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        self.model_pool.get(model_id)
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
                    weight: 1,
                    timeout_secs: None,
                    max_tokens: None,
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

    /// Whether the model has at least one healthy endpoint (optionally within a
    /// single channel scope, used by system-rule routability checks).
    pub fn has_healthy_endpoint(&self, model: &str, channel_scope: Option<&str>) -> bool {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        self.models
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|configured| {
                configured.name == model
                    && configured.published
                    && self
                        .model_pool
                        .get(&configured.id)
                        .is_some_and(|runtime| runtime.has_healthy_endpoint(channel_scope))
            })
    }

    /// Select a healthy endpoint from the model's flattened endpoint pool.
    ///
    /// `channel_scope` constrains the candidate set to one channel (system rule
    /// pin); `None` means every bound endpoint is a candidate. `upstream_model`
    /// optionally constrains to bindings carrying that upstream alias.
    pub fn route_model_endpoint(
        &self,
        model: &str,
        upstream_model: Option<&str>,
        channel_scope: Option<&str>,
        attempted: &[(String, i64)],
    ) -> Result<RoutePlan, RouteError> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        let model_cfg = models
            .iter()
            .find(|configured| configured.name == model && configured.published)
            .ok_or_else(|| {
                RouteError::not_found(format!("No route found for model '{}'", model))
            })?;
        let runtime = self
            .model_pool
            .get(&model_cfg.id)
            .ok_or_else(|| RouteError::unavailable(model_busy_message(&model)))?;
        let excluded: HashSet<i64> = attempted
            .iter()
            .map(|(_, endpoint_id)| *endpoint_id)
            .collect();
        let endpoint_idx = runtime
            .select_healthy_excluding(channel_scope, upstream_model, &excluded, &HashSet::new())
            .ok_or_else(|| RouteError::unavailable(model_busy_message(&model)))?;
        let state = &runtime.endpoints[endpoint_idx];
        Ok(RoutePlan {
            model_id: model_cfg.id.clone(),
            channel_id: state.channel_id.clone(),
            provider_name: state.provider.clone(),
            upstream_model: state.upstream_model.clone(),
            endpoint_idx,
            endpoint: state.endpoint.clone(),
            runtime: runtime.clone(),
            max_tokens: state.endpoint.max_tokens,
        })
    }

    /// Find an endpoint by DB id and update its enabled state in the circuit breaker.
    pub fn set_endpoint_enabled(&self, endpoint_id: i64, enabled: bool) {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let cache = self.cache.read().unwrap_or_else(|e| e.into_inner());
        self.model_pool.set_endpoint_enabled(endpoint_id, enabled);
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
                self.model_pool.record_endpoint_health(eid, true);
            }
        } else if let Some(eid) = endpoint_id {
            // Model-scoped failure: only the model under test is affected,
            // never other models sharing the endpoint.
            self.model_pool
                .record_endpoint_health_for_model(model_id, eid, false);
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

    /// Collect all model-endpoint breaker snapshots for persistence.
    /// Returns Vec<(model_id, [(endpoint_id, snapshot)])>.
    pub fn all_breaker_snapshots(&self) -> Vec<(String, Vec<(i64, BreakerSnapshot)>)> {
        self.model_pool.all_snapshots()
    }

    /// Restore model-endpoint breaker snapshots (called on startup after reload).
    pub fn restore_breaker_snapshots(&self, snapshots: &[(String, Vec<(i64, BreakerSnapshot)>)]) {
        self.model_pool.restore_snapshots(snapshots);
    }

    /// Aggregated endpoint health for a channel, across all **published**
    /// model runtimes that use it. Unlike the channel-level balancer (which
    /// business traffic doesn't update), this reflects the real model-endpoint
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
        for (model_id, runtime) in self.model_pool.iter() {
            if !published_ids.contains(model_id.as_str()) {
                continue;
            }
            for state in &runtime.endpoints {
                if state.channel_id != channel_id {
                    continue;
                }
                let entry = agg.entry(state.endpoint_id).or_insert((0, 0, false));
                entry.1 += 1;
                if state.breaker.is_healthy() {
                    entry.0 += 1;
                }
                if state.breaker.long_unavailable() {
                    entry.2 = true;
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
    /// **published** model runtimes. This is the real state business routing
    /// consults (model_pool), not the channel-level balancer.
    ///
    /// Returns per-endpoint: (endpoint_id, enabled, healthy_bindings,
    /// total_bindings, long_unavailable).
    pub fn all_endpoints_live_health(&self) -> Vec<(i64, bool, u32, u32, bool)> {
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
        for (model_id, runtime) in self.model_pool.iter() {
            if !published_ids.contains(model_id.as_str()) {
                continue;
            }
            for state in &runtime.endpoints {
                let entry = agg.entry(state.endpoint_id).or_insert((0, 0, false));
                entry.1 += 1;
                if state.breaker.is_healthy() {
                    entry.0 += 1;
                }
                if state.breaker.long_unavailable() {
                    entry.2 = true;
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
    ) -> Result<RouteContext, RouteError> {
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
    ) -> Result<RouteContext, RouteError> {
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
    ) -> Result<RouteContext, RouteError> {
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
    ) -> Result<RouteContext, RouteError> {
        let _snapshot_guard = self.snapshot_lock.read().unwrap_or_else(|e| e.into_inner());
        let mut model_name = model.to_string();
        let chs = self.channels.read().unwrap_or_else(|e| e.into_inner());
        let models = self.models.read().unwrap_or_else(|e| e.into_inner());
        let rules = self.rules.read().unwrap_or_else(|e| e.into_inner());

        for rule in rules.iter() {
            if rule.scope != "user" || !rule.enabled {
                continue;
            }
            let owner_matches = match &rule.team_id {
                Some(id) => team_id == Some(id.as_str()),
                None => rule.user_id == user_id,
            };
            if owner_matches
                && !rule.target_model.is_empty()
                && match_pattern(&model_name, &rule.source_model)
            {
                model_name = rule.target_model.clone();
            }
        }

        let mut channel_scope = None;
        let mut upstream_model = None;
        let mut matched_system: Vec<&RoutingRule> = rules
            .iter()
            .filter(|rule| {
                if rule.scope != "system"
                    || !rule.enabled
                    || (rule.user_id != "*" && rule.user_id != user_id)
                {
                    return false;
                }
                rule.source_model.is_empty()
                    || rule.source_model == "*"
                    || match_pattern(&model_name, &rule.source_model)
            })
            .collect();
        matched_system.sort_by_key(|r| r.priority);
        // A system rule may pin the candidate set to one channel. Skip rules
        // whose channel currently has no healthy endpoint so a lower-priority
        // rule can still match.
        for rule in &matched_system {
            if !rule.channel_id.is_empty() {
                let routable = models.iter().any(|m| {
                    m.name == model_name
                        && (matches!(visibility, RouteVisibility::Internal) || m.published)
                        && self.model_pool.get(&m.id).is_some_and(|r| {
                            r.has_healthy_endpoint(Some(&rule.channel_id))
                                && (rule.upstream_model.is_empty()
                                    || r.endpoints.iter().any(|e| {
                                        e.channel_id == rule.channel_id
                                            && e.upstream_model.as_deref()
                                                == Some(rule.upstream_model.as_str())
                                    }))
                        })
                });
                if routable {
                    channel_scope = Some(rule.channel_id.clone());
                    if !rule.upstream_model.is_empty() {
                        upstream_model = Some(rule.upstream_model.clone());
                    }
                    break;
                }
            }
        }
        if channel_scope.is_none() {
            for rule in &matched_system {
                if !rule.target_model.is_empty() {
                    model_name = rule.target_model.clone();
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
        let model_cfg = models.iter().find(|m| {
            m.name == model_name && (matches!(visibility, RouteVisibility::Internal) || m.published)
        });
        let Some(model_cfg) = model_cfg else {
            return Err(RouteError::unavailable(model_busy_message(&model_name)));
        };
        let scope = channel_scope.as_deref();
        let has_endpoint = self
            .model_pool
            .get(&model_cfg.id)
            .is_some_and(|r| r.has_healthy_endpoint(scope));
        if !has_endpoint {
            let binding_count = model_cfg.channels.len();
            let enabled_binding_count = model_cfg
                .channels
                .iter()
                .filter(|b| chs.get(&b.channel_id).is_some_and(|c| c.enabled))
                .count();
            tracing::warn!(user_id, requested_model = model, resolved_model = %model_name, binding_count, enabled_binding_count, "No routable model endpoint");
            return Err(RouteError::unavailable(model_busy_message(&model_name)));
        }
        Ok(RouteContext {
            resolved_model: model_name,
            upstream_model,
            channel_scope,
        })
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

/// Friendly message for when a model exists but all its endpoints are
/// circuit-broken (no healthy endpoint to route to).
pub(crate) fn model_busy_message(model: &str) -> String {
    format!(
        "Model '{}' is currently at peak capacity, all channels are busy. \
         Please try again later.",
        model
    )
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
