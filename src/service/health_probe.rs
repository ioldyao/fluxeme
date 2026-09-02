use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::balancer::LoadBalancer;
use crate::config::types::EndpointConfig;
use crate::db::{Database, ProbeResultRow};
use crate::provider::{
    is_retryable_error, ErrorKind, ProviderAdapter, ProviderError, ProviderRegistry,
};
use crate::service::routing::RoutingService;

const MAX_CONCURRENT_ENDPOINT_PROBES: usize = 8;
const PROBE_LEASE_TTL_SECS: u64 = 120;

/// Breaker action to mirror onto a channel-level cache balancer.
enum BreakerAction {
    Success,
    Failure,
}

/// Mirror a probe outcome from the binding-pool balancer onto the
/// channel-level cache balancer. The two balancers are separate instances, so
/// without this the model console health dot (`channel_health()`) never sees
/// probe-driven recovery. The endpoint is located by DB id first, then URL.
fn sync_breaker(
    balancer: &crate::balancer::HealthAwareBalancer,
    endpoint: &EndpointConfig,
    action: BreakerAction,
) {
    let idx = balancer
        .endpoints()
        .iter()
        .position(|ep| {
            if let (Some(a), Some(b)) = (ep.id, endpoint.id) {
                a == b
            } else {
                ep.url == endpoint.url
            }
        });
    let Some(idx) = idx else {
        return;
    };
    match action {
        BreakerAction::Success => balancer.record_success(idx),
        BreakerAction::Failure => balancer.record_failure(idx),
    }
}

struct ProbeJob {
    binding_order: usize,
    endpoint_order: usize,
    channel_id: String,
    model_id: String,
    provider_name: String,
    upstream_name: String,
    adapter: Arc<dyn ProviderAdapter>,
    balancer: Arc<LoadBalancer>,
    /// Channel-level cache balancer (same endpoints as `balancer` but lives in
    /// the per-channel cache, not the binding pool).  When a probe succeeds or
    /// fails, we sync the circuit breaker state back to this balancer so that
    /// `channel_health()` (read by the model console dot) reflects the real
    /// endpoint status immediately.
    channel_balancer: Option<Arc<LoadBalancer>>,
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
        bindings.sort_by_key(|binding| binding.priority);

        let mut ordered_results = Vec::new();
        let mut jobs = Vec::new();

        for (binding_order, binding) in bindings.iter().enumerate() {
            let upstream_name = binding
                .upstream_model
                .clone()
                .unwrap_or_else(|| model.name.clone());

            let route = match self.routing.get_route(&binding.channel_id) {
                Some(route) => route,
                None => {
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
                        ),
                    });
                    continue;
                }
            };
            let provider_name = route.0.clone();
            let adapter = match self.providers.get(&provider_name) {
                Some(adapter) => adapter,
                None => {
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
                        ),
                    });
                    continue;
                }
            };

            let probe_balancer = self
                .routing
                .get_binding_route(model_id, &binding.channel_id)
                .unwrap_or_else(|| route.1.clone());
            let channel_balancer = route.1.clone();
            let endpoints = probe_balancer.as_health_aware().endpoints();
            // Model liveness probes exercise the configured chat operation.
            // Full URLs are valid here: provider URL resolution preserves them
            // verbatim, so they must not be mistaken for discovery endpoints.
            let endpoint_jobs: Vec<_> = endpoints
                .iter()
                .cloned()
                .enumerate()
                .filter(|(_, endpoint)| endpoint.enabled)
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
                    ),
                });
                continue;
            }

            for (endpoint_order, endpoint) in endpoint_jobs {
                jobs.push(ProbeJob {
                    binding_order,
                    endpoint_order,
                    channel_id: binding.channel_id.clone(),
                    model_id: model_id.to_string(),
                    provider_name: provider_name.clone(),
                    upstream_name: upstream_name.clone(),
                    adapter: adapter.clone(),
                    balancer: probe_balancer.clone(),
                    channel_balancer: Some(channel_balancer.clone()),
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
                let Some(balancer) = self
                    .routing
                    .get_binding_route(&model.id, &binding.channel_id)
                else {
                    continue;
                };
                let channel_balancer = self.routing.get_route(&binding.channel_id).map(|r| r.1);
                let provider_name = route.0.clone();
                let Some(adapter) = self.providers.get(&provider_name) else {
                    continue;
                };
                let upstream_name = binding
                    .upstream_model
                    .clone()
                    .unwrap_or_else(|| model.name.clone());
                for endpoint_order in 0..balancer.as_health_aware().endpoint_count() {
                    let Some(endpoint) = balancer.as_health_aware().endpoint(endpoint_order) else {
                        continue;
                    };
                    if !endpoint.enabled {
                        continue;
                    }
                    jobs.push(ProbeJob {
                        binding_order: jobs.len(),
                        endpoint_order,
                        channel_id: binding.channel_id.clone(),
                        model_id: model.id.clone(),
                        provider_name: provider_name.clone(),
                        upstream_name: upstream_name.clone(),
                        adapter: adapter.clone(),
                        balancer: balancer.clone(),
                        channel_balancer: channel_balancer.clone(),
                        endpoint: endpoint.clone(),
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
            adapter,
            balancer,
            channel_balancer,
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
                        Some(endpoint.url.clone()),
                    ),
                };
            }
            let Some((_, _, token)) = balancer
                .as_health_aware()
                .begin_probe_endpoint(endpoint_order)
            else {
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
                if let Some(token) = probe_token {
                    balancer
                        .as_health_aware()
                        .probe_success(endpoint_order, token);
                } else {
                    balancer.as_health_aware().record_success(endpoint_order);
                }
                // Sync to channel-level cache balancer so the model console
                // health dot (read via `channel_health()`) reflects success.
                if let Some(ref cb) = channel_balancer {
                    sync_breaker(cb.as_health_aware(), &endpoint, BreakerAction::Success);
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
                    Some(endpoint.url.clone()),
                )
            }
            Err(error) => {
                if matches!(error.kind(), ErrorKind::ConnectFailed | ErrorKind::Timeout)
                    || is_retryable_error(&error)
                {
                    if let Some(token) = probe_token {
                        balancer
                            .as_health_aware()
                            .probe_failure(endpoint_order, token);
                    } else {
                        balancer.as_health_aware().record_failure(endpoint_order);
                    }
                    // Sync failure to channel cache balancer.
                    if let Some(ref cb) = channel_balancer {
                        sync_breaker(cb.as_health_aware(), &endpoint, BreakerAction::Failure);
                    }
                } else if let Some(token) = probe_token {
                    // Authentication, model, and request errors describe the
                    // probe input/upstream contract, not endpoint liveness.
                    balancer
                        .as_health_aware()
                        .probe_release(endpoint_order, token);
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
            endpoint_url,
        }
    }
}
