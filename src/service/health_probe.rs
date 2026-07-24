use std::sync::Arc;
use std::time::Instant;

use futures::stream::{self, StreamExt};
use uuid::Uuid;

use crate::balancer::LoadBalancer;
use crate::config::types::EndpointConfig;
use crate::db::{Database, ProbeResultRow};
use crate::provider::{ProviderAdapter, ProviderError, ProviderRegistry};
use crate::service::routing::RoutingService;

const MAX_CONCURRENT_ENDPOINT_PROBES: usize = 8;

struct ProbeJob {
    binding_order: usize,
    endpoint_order: usize,
    channel_id: String,
    model_id: String,
    provider_name: String,
    upstream_name: String,
    adapter: Arc<dyn ProviderAdapter>,
    balancer: Arc<LoadBalancer>,
    endpoint: EndpointConfig,
    stream: bool,
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
}

impl HealthProbeService {
    pub fn new(
        db: Arc<Database>,
        providers: Arc<ProviderRegistry>,
        routing: Arc<RoutingService>,
    ) -> Self {
        Self {
            db,
            providers,
            routing,
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

            let endpoint_jobs: Vec<_> = route
                .1
                .as_health_aware()
                .endpoints()
                .iter()
                .cloned()
                .enumerate()
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
                    balancer: route.1.clone(),
                    endpoint,
                    stream,
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

        let mut rows = Vec::with_capacity(ordered_results.len());
        for ordered in ordered_results {
            self.db
                .insert_probe_result(&ordered.row)
                .await
                .map_err(|e| e.0)?;
            rows.push(ordered.row);
        }

        Ok(rows)
    }

    /// Get the most recent probe result for each channel endpoint.
    pub async fn all_latest_probes(&self) -> Result<Vec<ProbeResultRow>, String> {
        self.db.all_latest_probe_results().await.map_err(|e| e.0)
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
            endpoint,
            stream,
        } = job;

        let start = Instant::now();
        let result =
            Self::probe_endpoint(&provider_name, &adapter, &endpoint, &upstream_name, stream).await;
        let latency_ms = start.elapsed().as_millis() as u64;

        let row = match result {
            Ok(()) => {
                balancer.as_health_aware().record_success(endpoint_order);
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
                balancer.as_health_aware().record_failure(endpoint_order);
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
