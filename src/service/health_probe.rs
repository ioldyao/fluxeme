use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::config::types::EndpointConfig;
use crate::db::{Database, ProbeResultRow};
use crate::provider::{
    is_retryable_error, ErrorKind, ProviderAdapter, ProviderError, ProviderRegistry,
};
use crate::service::endpoint_pool::ModelEndpointRuntime;
use crate::service::routing::RoutingService;

const MAX_CONCURRENT_ENDPOINT_PROBES: usize = 8;
const PROBE_LEASE_TTL_SECS: u64 = 120;

struct ProbeJob {
    binding_order: usize,
    endpoint_order: usize,
    channel_id: String,
    model_id: String,
    provider_name: String,
    upstream_name: String,
    adapter: Arc<dyn ProviderAdapter>,
    /// The model's compiled endpoint runtime; `endpoint_idx` indexes it.
    runtime: Arc<ModelEndpointRuntime>,
    endpoint_idx: usize,
    /// The routing service, so a successful probe can sync its breaker state
    /// back to both the model endpoint pool and the channel-level cache balancer.
    /// Only set for manual probes (`automatic=false`); automatic probes are
    /// serialized by proxy lease and the caller does not wait for results, so
    /// there is no caller-side sync step.
    routing: Option<Arc<RoutingService>>,
    /// The upstream model name this probe should test with. Written to CH
    /// so the frontend can match results to the specific binding.
    upstream_model: String,
    endpoint: EndpointConfig,
    stream: bool,
    /// Whether this is an automatic recovery probe.
    automatic: bool,
    cache: Arc<crate::cache::RedisCache>,
    instance_id: String,
}

struct OrderedProbeRow {
    binding_order: usize,
    endpoint_order: usize,
    row: ProbeResultRow,
}

/// Unified health probe service.
///
/// Sends real chat completion requests to every selected channel endpoint of a
/// model and records success/latency in the database for persistence across
/// restarts.
pub struct HealthProbeService {
    db: Arc<Database>,
    providers: Arc<ProviderRegistry>,
    routing: Arc<RoutingService>,
    /// ClickHouse backend — probe results are observability data and live
    /// in CH. Falls back to PostgreSQL when CH is not configured.
    ch: Option<std::sync::Arc<crate::ch_backend::ClickHouseBackend>>,
    /// Redis coordinates automatic probes across gateway instances.
    cache: Arc<crate::cache::RedisCache>,
    instance_id: String,
}

impl HealthProbeService {
    pub fn new(
        db: Arc<Database>,
        providers: Arc<ProviderRegistry>,
        routing: Arc<RoutingService>,
        ch: Option<std::sync::Arc<crate::ch_backend::ClickHouseBackend>>,
        cache: Arc<crate::cache::RedisCache>,
        instance_id: impl Into<String>,
    ) -> Self {
        Self {
            db,
            providers,
            routing,
            ch,
            cache,
            instance_id: instance_id.into(),
        }
    }

    /// Probe every endpoint under the selected channel bindings of a model and
    /// return per-endpoint probe results.
    pub async fn probe_model(
        &self,
        model_id: &str,
        channel_ids: &[String],
        stream: bool,
    ) -> Result<Vec<ProbeResultRow>, String> {
        let model = self
            .db
            .get_model(model_id)
            .await
            .map_err(|e| e.0)?
            .ok_or_else(|| format!("Model '{}' not found", model_id))?;

        let mut bindings = model.channels.clone();
        if !channel_ids.is_empty() {
            bindings.retain(|binding| channel_ids.contains(&binding.channel_id));
        }
        if bindings.is_empty() {
            return Err("No channel bindings selected".to_string());
        }
        // Probe bindings in model declaration order; endpoint selection no longer
        // has a channel-priority ordering.
        let mut ordered_results = Vec::new();
        let mut jobs = Vec::new();

        let Some(runtime) = self.routing.get_model_endpoint_runtime(model_id) else {
            return Err(format!("Model '{}' has no endpoint runtime", model_id));
        };

        for (binding_order, binding) in bindings.iter().enumerate() {
            let upstream_name = binding
                .upstream_model
                .clone()
                .unwrap_or_else(|| model.name.clone());
            let Some(route) = self.routing.get_route(&binding.channel_id) else {
                ordered_results.push(OrderedProbeRow {
                    binding_order,
                    endpoint_order: 0,
                    row: Self::make_row(
                        &binding.channel_id,
                        model_id,
                        false,
                        0,
                        Some("Route not available"),
                        None,
                        None,
                        None,
                    ),
                });
                continue;
            };
            let provider_name = route.0.clone();
            let Some(adapter) = self.providers.get(&provider_name) else {
                ordered_results.push(OrderedProbeRow {
                    binding_order,
                    endpoint_order: 0,
                    row: Self::make_row(
                        &binding.channel_id,
                        model_id,
                        false,
                        0,
                        Some("Provider adapter not found"),
                        None,
                        None,
                        None,
                    ),
                });
                continue;
            };
            let endpoint_jobs: Vec<_> = runtime
                .endpoints
                .iter()
                .enumerate()
                .filter(|(_, state)| {
                    state.channel_id == binding.channel_id && state.endpoint.enabled
                })
                .map(|(idx, state)| (idx, state.endpoint.clone()))
                .collect();
            if endpoint_jobs.is_empty() {
                ordered_results.push(OrderedProbeRow {
                    binding_order,
                    endpoint_order: 0,
                    row: Self::make_row(
                        &binding.channel_id,
                        model_id,
                        false,
                        0,
                        Some("No enabled endpoints"),
                        None,
                        None,
                        None,
                    ),
                });
                continue;
            }
            for (endpoint_order, (endpoint_idx, endpoint)) in endpoint_jobs.into_iter().enumerate()
            {
                jobs.push(ProbeJob {
                    binding_order,
                    endpoint_order,
                    channel_id: binding.channel_id.clone(),
                    model_id: model_id.to_string(),
                    provider_name: provider_name.clone(),
                    upstream_name: upstream_name.clone(),
                    upstream_model: upstream_name.clone(),
                    adapter: adapter.clone(),
                    runtime: runtime.clone(),
                    endpoint_idx,
                    routing: Some(self.routing.clone()),
                    endpoint,
                    stream,
                    automatic: false,
                    cache: self.cache.clone(),
                    instance_id: self.instance_id.clone(),
                });
            }
        }

        let mut job_results = stream::iter(jobs)
            .map(|job| async move { Self::run_probe_job(job).await })
            .buffer_unordered(MAX_CONCURRENT_ENDPOINT_PROBES)
            .collect::<Vec<_>>()
            .await;
        ordered_results.append(&mut job_results);

        ordered_results.sort_by(|left, right| {
            left.binding_order
                .cmp(&right.binding_order)
                .then(left.endpoint_order.cmp(&right.endpoint_order))
                .then_with(|| left.row.endpoint_url.cmp(&right.row.endpoint_url))
        });

        let rows: Vec<ProbeResultRow> = ordered_results.into_iter().map(|o| o.row).collect();

        // Probe results are observability data → ClickHouse only. No PG
        // fallback: observability and business storage are fully decoupled.
        let ch = self
            .ch
            .as_ref()
            .ok_or_else(|| "ClickHouse not configured — probe results require CH".to_string())?;
        ch.insert_probe_results(&rows)
            .await
            .map_err(|e| format!("CH probe write failed: {e}"))?;

        Ok(rows)
    }

    /// Probe binding endpoints through the same model-aware provider operation
    /// used by business traffic. A probe claim is separate from business
    /// traffic, so a recovering endpoint cannot re-enter routing until success.
    pub async fn probe_open_bindings(&self) -> Result<Vec<ProbeResultRow>, String> {
        self.probe_bindings(false).await
    }

    /// Fast recovery cycle — probe every enabled endpoint that is currently
    /// Open (after cooldown) so recovery doesn't wait for the next slow cycle.
    pub async fn probe_recovering_bindings(&self) -> Result<Vec<ProbeResultRow>, String> {
        let rows = self.probe_bindings(true).await?;
        Ok(rows)
    }

    async fn probe_bindings(&self, recovering_only: bool) -> Result<Vec<ProbeResultRow>, String> {
        let models = self.db.list_models().await.map_err(|e| e.0)?;
        let mut jobs = Vec::new();

        for model in &models {
            for binding in &model.channels {
                let Some(channel) = self.routing.get_channel(&binding.channel_id) else {
                    continue;
                };
                if !channel.enabled || !model.published {
                    continue;
                }
                let Some(route) = self.routing.get_route(&binding.channel_id) else {
                    continue;
                };
                let Some(runtime) = self.routing.get_model_endpoint_runtime(&model.id) else {
                    continue;
                };
                let provider_name = route.0.clone();
                let Some(adapter) = self.providers.get(&provider_name) else {
                    continue;
                };
                let upstream_name = binding
                    .upstream_model
                    .clone()
                    .unwrap_or_else(|| model.name.clone());
                for (endpoint_idx, state) in runtime.endpoints.iter().enumerate() {
                    if state.channel_id != binding.channel_id {
                        continue;
                    }
                    if !state.endpoint.enabled {
                        continue;
                    }
                    if recovering_only && state.breaker.is_healthy() {
                        continue;
                    }
                    jobs.push(ProbeJob {
                        binding_order: jobs.len(),
                        endpoint_order: endpoint_idx,
                        channel_id: binding.channel_id.clone(),
                        model_id: model.id.clone(),
                        provider_name: provider_name.clone(),
                        upstream_name: upstream_name.clone(),
                        upstream_model: upstream_name.clone(),
                        adapter: adapter.clone(),
                        runtime: runtime.clone(),
                        endpoint_idx,
                        routing: None,
                        endpoint: state.endpoint.clone(),
                        stream: false,
                        automatic: true,
                        cache: self.cache.clone(),
                        instance_id: self.instance_id.clone(),
                    });
                }
            }
        }

        if jobs.is_empty() {
            return Ok(Vec::new());
        }
        let rows = stream::iter(jobs)
            .map(|job| async move { Self::run_probe_job(job).await })
            .buffer_unordered(MAX_CONCURRENT_ENDPOINT_PROBES)
            .collect::<Vec<_>>()
            .await;
        let rows: Vec<ProbeResultRow> = rows.into_iter().map(|result| result.row).collect();
        let ch = self
            .ch
            .as_ref()
            .ok_or_else(|| "ClickHouse not configured — probe results require CH".to_string())?;
        ch.insert_probe_results(&rows)
            .await
            .map_err(|e| format!("CH probe write failed: {e}"))?;
        Ok(rows)
    }

    /// Get the most recent probe result for each channel endpoint.
    pub async fn all_latest_probes(&self) -> Result<Vec<ProbeResultRow>, String> {
        let ch = self
            .ch
            .as_ref()
            .ok_or_else(|| "ClickHouse not configured — probe results require CH".to_string())?;
        ch.all_latest_probe_results().await
    }

    async fn run_probe_job(job: ProbeJob) -> OrderedProbeRow {
        let ProbeJob {
            binding_order,
            endpoint_order,
            channel_id,
            model_id,
            provider_name,
            upstream_name,
            upstream_model,
            adapter,
            runtime,
            endpoint_idx,
            routing,
            endpoint,
            stream,
            automatic,
            cache,
            instance_id,
        } = job;

        let start = Instant::now();
        let (lease_key, lease_owner, probe_token) = if automatic {
            let lease_key = format!(
                "routing:probe-lease:{}:{}:{}",
                model_id,
                channel_id,
                endpoint.id.map_or_else(
                    || {
                        let digest = Sha256::digest(endpoint.url.as_bytes());
                        hex::encode(digest)
                    },
                    |id| id.to_string(),
                )
            );
            let lease_owner = format!("{}:{}", instance_id, Uuid::new_v4());
            let acquired = cache
                .probe_try_acquire(&lease_key, &lease_owner, PROBE_LEASE_TTL_SECS)
                .await
                .unwrap_or(false);
            if !acquired {
                return OrderedProbeRow {
                    binding_order,
                    endpoint_order,
                    row: Self::make_row(
                        &channel_id,
                        &model_id,
                        false,
                        0,
                        Some("Probe lease unavailable"),
                        endpoint.id,
                        None,
                        Some(endpoint.url.clone()),
                    ),
                };
            }
            let Some((_, _, token)) = runtime.endpoints.get(endpoint_idx).and_then(|state| {
                state
                    .breaker
                    .begin_probe()
                    .map(|token| (endpoint_idx, state, token))
            }) else {
                let _ = cache.probe_release(&lease_key, &lease_owner).await;
                return OrderedProbeRow {
                    binding_order,
                    endpoint_order,
                    row: Self::make_row(
                        &channel_id,
                        &model_id,
                        false,
                        0,
                        Some("Probe already claimed"),
                        endpoint.id,
                        None,
                        Some(endpoint.url.clone()),
                    ),
                };
            };
            (Some(lease_key), Some(lease_owner), token)
        } else {
            (None, None, None)
        };
        let result = if automatic {
            match tokio::time::timeout(
                Duration::from_secs(PROBE_LEASE_TTL_SECS.saturating_sub(1)),
                Self::probe_endpoint(&provider_name, &adapter, &endpoint, &upstream_name, stream),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(ProviderError::new(
                    "Probe lease expired",
                    ErrorKind::Timeout,
                )),
            }
        } else {
            Self::probe_endpoint(&provider_name, &adapter, &endpoint, &upstream_name, stream).await
        };
        let latency_ms = start.elapsed().as_millis() as u64;

        let row = match result {
            Ok(()) => {
                if let Some(state) = runtime.endpoints.get(endpoint_idx) {
                    if let Some(token) = probe_token {
                        state.breaker.probe_success(token);
                    } else {
                        state.breaker.record_success();
                    }
                }
                // Sync to both the binding pool and the channel-level cache
                // balancer so the model console health dot reflects success.
                if let Some(ref routing) = routing {
                    routing.record_endpoint_health(
                        &model_id,
                        &channel_id,
                        endpoint.id,
                        &endpoint.url,
                        true,
                    );
                }
                if let (Some(key), Some(owner)) = (lease_key.as_ref(), lease_owner.as_ref()) {
                    let _ = cache.probe_release(key, owner).await;
                }
                Self::make_row(
                    &channel_id,
                    &model_id,
                    true,
                    latency_ms,
                    None,
                    endpoint.id,
                    Some(upstream_model.clone()),
                    Some(endpoint.url.clone()),
                )
            }
            Err(error) => {
                if matches!(error.kind(), ErrorKind::ConnectFailed | ErrorKind::Timeout)
                    || is_retryable_error(&error)
                {
                    if let Some(state) = runtime.endpoints.get(endpoint_idx) {
                        if let Some(token) = probe_token {
                            state.breaker.probe_failure(token);
                        } else {
                            state.breaker.record_failure();
                        }
                    }
                    // Sync failure to the model-scoped binding only; never
                    // broadcast to other models sharing this endpoint.
                    if let Some(ref routing) = routing {
                        routing.record_endpoint_health(
                            &model_id,
                            &channel_id,
                            endpoint.id,
                            &endpoint.url,
                            false,
                        );
                    }
                } else if let Some(token) = probe_token {
                    // Contract/input errors do not describe endpoint liveness.
                    if let Some(state) = runtime.endpoints.get(endpoint_idx) {
                        state.breaker.probe_release(token);
                    }
                }
                if let (Some(key), Some(owner)) = (lease_key.as_ref(), lease_owner.as_ref()) {
                    let _ = cache.probe_release(key, owner).await;
                }
                Self::make_row(
                    &channel_id,
                    &model_id,
                    false,
                    latency_ms,
                    Some(&error.0),
                    endpoint.id,
                    Some(upstream_model.clone()),
                    Some(endpoint.url.clone()),
                )
            }
        };

        OrderedProbeRow {
            binding_order,
            endpoint_order,
            row,
        }
    }

    async fn probe_endpoint(
        provider_name: &str,
        adapter: &Arc<dyn ProviderAdapter>,
        endpoint: &EndpointConfig,
        upstream_name: &str,
        stream: bool,
    ) -> Result<(), ProviderError> {
        let test_body = serde_json::json!({
            "model": upstream_name,
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": 0.01,
            "max_tokens": 1,
            "top_p": 0.01,
            "stream": stream,
        });

        if provider_name == "anthropic" {
            let body = serde_json::json!({
                "model": upstream_name,
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1,
                "stream": stream,
            });
            if stream {
                match adapter.messages_stream(endpoint, body).await {
                    Ok(mut response) => response.next().await.map(|_| ()).ok_or_else(|| {
                        ProviderError::new(
                            "Upstream returned an empty stream",
                            crate::provider::ErrorKind::Other,
                        )
                    }),
                    Err(error) => Err(error),
                }
            } else {
                adapter.messages(endpoint, body).await.map(|_| ())
            }
        } else if stream {
            match adapter.chat_complete_stream(endpoint, test_body).await {
                Ok(mut response) => response.next().await.map(|_| ()).ok_or_else(|| {
                    ProviderError::new(
                        "Upstream returned an empty stream",
                        crate::provider::ErrorKind::Other,
                    )
                }),
                Err(error) => Err(error),
            }
        } else {
            adapter.chat_complete(endpoint, test_body).await.map(|_| ())
        }
    }

    fn make_row(
        channel_id: &str,
        model_id: &str,
        success: bool,
        latency_ms: u64,
        error: Option<&str>,
        endpoint_id: Option<i64>,
        upstream_model: Option<String>,
        endpoint_url: Option<String>,
    ) -> ProbeResultRow {
        ProbeResultRow {
            id: Uuid::new_v4().to_string(),
            channel_id: channel_id.to_string(),
            model_id: model_id.to_string(),
            success,
            latency_ms,
            error: error.map(|text| text.to_string()),
            probed_at: chrono::Utc::now().to_rfc3339(),
            endpoint_id,
            upstream_model,
            endpoint_url,
        }
    }
}
