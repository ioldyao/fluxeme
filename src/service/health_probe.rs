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
const PROBE_LEASE_TTL_SECS: u64 = 180;
const PROBE_REQUEST_TIMEOUT_MAX_SECS: u64 = PROBE_LEASE_TTL_SECS - 1;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeProtocol {
    #[default]
    Auto,
    OpenaiChat,
    AnthropicMessages,
    Responses,
}

impl ProbeProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::OpenaiChat => "openai_chat",
            Self::AnthropicMessages => "anthropic_messages",
            Self::Responses => "responses",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ProbeRequestConfig {
    pub prompt: String,
    pub max_output_tokens: u32,
    pub temperature: f64,
    pub top_p: f64,
    pub timeout_secs: u64,
    pub protocol: ProbeProtocol,
}

impl Default for ProbeRequestConfig {
    fn default() -> Self {
        Self {
            prompt: "hi".to_string(),
            max_output_tokens: 1,
            temperature: 0.01,
            top_p: 0.01,
            timeout_secs: 30,
            protocol: ProbeProtocol::Auto,
        }
    }
}

impl ProbeRequestConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.prompt.chars().count() > 4096 {
            return Err("prompt must be at most 4096 characters".to_string());
        }
        if !(1..=16).contains(&self.max_output_tokens) {
            return Err("max_output_tokens must be between 1 and 16".to_string());
        }
        if !self.temperature.is_finite() || !(0.0..=2.0).contains(&self.temperature) {
            return Err("temperature must be between 0 and 2".to_string());
        }
        if !self.top_p.is_finite() || !(0.0..=1.0).contains(&self.top_p) {
            return Err("top_p must be between 0 and 1".to_string());
        }
        if !(1..=PROBE_REQUEST_TIMEOUT_MAX_SECS).contains(&self.timeout_secs) {
            return Err(format!(
                "timeout_secs must be between 1 and {PROBE_REQUEST_TIMEOUT_MAX_SECS}"
            ));
        }
        Ok(())
    }

    /// Resolve the effective request protocol for a channel provider.
    pub fn resolved_protocol(&self, provider_name: &str) -> ProbeProtocol {
        match &self.protocol {
            ProbeProtocol::Auto => {
                if provider_name == "anthropic" {
                    ProbeProtocol::AnthropicMessages
                } else {
                    ProbeProtocol::OpenaiChat
                }
            }
            other => other.clone(),
        }
    }

    /// Build the request body for a resolved protocol.
    pub fn build_body(
        &self,
        upstream_name: &str,
        protocol: &ProbeProtocol,
        stream: bool,
    ) -> serde_json::Value {
        let role_user = serde_json::json!({"role": "user", "content": self.prompt});
        match protocol {
            ProbeProtocol::Responses => serde_json::json!({
                "model": upstream_name,
                "input": [{"role": "user", "content": [{"type": "input_text", "text": self.prompt}]}],
                "max_output_tokens": self.max_output_tokens,
                "temperature": self.temperature,
                "top_p": self.top_p,
                "stream": stream,
            }),
            _ => serde_json::json!({
                "model": upstream_name,
                "messages": [role_user],
                "max_tokens": self.max_output_tokens,
                "temperature": self.temperature,
                "top_p": self.top_p,
                "stream": stream,
            }),
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ProbeTestResult {
    pub success: bool,
    pub model: String,
    pub channel_id: String,
    pub endpoint_id: Option<i64>,
    pub endpoint_url: String,
    pub upstream_model: String,
    pub protocol: String,
    pub latency_ms: u64,
    pub ttft_ms: Option<u64>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
}

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
    /// Request template used to build the probe body and timeout.
    config: ProbeRequestConfig,
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

    /// Load and validate the persisted request template. Invalid or missing
    /// settings fail safe to the default minimal request.
    async fn request_config(&self) -> ProbeRequestConfig {
        let mut config = ProbeRequestConfig::default();
        if let Ok(Some(value)) = self.db.get_setting("probe_request_config").await {
            if let Ok(parsed) = serde_json::from_str::<ProbeRequestConfig>(&value) {
                config = parsed;
            }
        }
        if config.validate().is_err() {
            ProbeRequestConfig::default()
        } else {
            config
        }
    }

    /// Return the current request template for the admin preview.
    pub async fn get_request_config(&self) -> ProbeRequestConfig {
        self.request_config().await
    }

    pub async fn set_request_config(&self, config: ProbeRequestConfig) -> Result<(), String> {
        config.validate()?;
        let json = serde_json::to_string(&config).map_err(|e| e.to_string())?;
        self.db
            .set_setting("probe_request_config", &json)
            .await
            .map_err(|e| e.0)
    }

    /// Send a one-off probe request with the configured template. Unlike
    /// scheduled probes this never claims a probe lease and never mutates the
    /// circuit breaker — it is a pure connectivity/response test.
    pub async fn test_probe(
        &self,
        model_id: &str,
        channel_id: &str,
        endpoint_id: Option<i64>,
        protocol: Option<ProbeProtocol>,
    ) -> Result<ProbeTestResult, String> {
        let model = self
            .db
            .get_model(model_id)
            .await
            .map_err(|e| e.0)?
            .ok_or_else(|| format!("Model '{model_id}' not found"))?;
        let Some(binding) = model
            .channels
            .iter()
            .find(|binding| binding.channel_id == channel_id)
        else {
            return Err("Model is not bound to the selected channel".to_string());
        };
        let Some(route) = self.routing.get_route(channel_id) else {
            return Err("Channel route not available".to_string());
        };
        let Some(runtime) = self.routing.get_model_endpoint_runtime(model_id) else {
            return Err("Model has no endpoint runtime".to_string());
        };
        let provider_name = route.0.clone();
        let Some(adapter) = self.providers.get(&provider_name) else {
            return Err("Provider adapter not found".to_string());
        };
        let Some((_, state)) = runtime.endpoints.iter().enumerate().find(|(_, state)| {
            state.channel_id == channel_id
                && state.endpoint.enabled
                && (endpoint_id.is_none_or(|id| state.endpoint.id == Some(id)))
        }) else {
            return Err("No enabled endpoint matched the selection".to_string());
        };
        let endpoint = state.endpoint.clone();
        let upstream_name = binding
            .upstream_model
            .clone()
            .unwrap_or_else(|| model.name.clone());
        let mut config = self.request_config().await;
        if let Some(protocol) = protocol {
            config.protocol = protocol;
        }
        let effective_protocol = config.resolved_protocol(&provider_name);

        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(config.timeout_secs.min(PROBE_REQUEST_TIMEOUT_MAX_SECS)),
            Self::probe_endpoint(
                &provider_name,
                &adapter,
                &endpoint,
                &upstream_name,
                &config,
                false,
            ),
        )
        .await;
        let latency_ms = start.elapsed().as_millis() as u64;
        let (success, error_kind, error_message) = match result {
            Ok(Ok(())) => (true, None, None),
            Ok(Err(error)) => (
                false,
                Some(format!("{:?}", error.kind()).to_lowercase()),
                Some(error.0),
            ),
            Err(_) => (
                false,
                Some("timeout".to_string()),
                Some("Probe request timed out".to_string()),
            ),
        };

        Ok(ProbeTestResult {
            success,
            model: model.name,
            channel_id: channel_id.to_string(),
            endpoint_id: endpoint.id,
            endpoint_url: endpoint.url,
            upstream_model: upstream_name,
            protocol: effective_protocol.as_str().to_string(),
            latency_ms,
            ttft_ms: None,
            error_kind,
            error_message,
            prompt_tokens: None,
            completion_tokens: None,
        })
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
        let config = self.request_config().await;

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
                    config: config.clone(),
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
        let config = self.request_config().await;

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
                    if !recovering_only && !state.breaker.is_healthy() {
                        // Recovery probes exclusively own Open/HalfOpen endpoints.
                        // Keeping them out of the periodic cycle ensures the
                        // long-unavailable interval cannot be bypassed.
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
                        config: config.clone(),
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
            config,
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
                Duration::from_secs(config.timeout_secs.min(PROBE_REQUEST_TIMEOUT_MAX_SECS)),
                Self::probe_endpoint(
                    &provider_name,
                    &adapter,
                    &endpoint,
                    &upstream_name,
                    &config,
                    stream,
                ),
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
            Self::probe_endpoint(
                &provider_name,
                &adapter,
                &endpoint,
                &upstream_name,
                &config,
                stream,
            )
            .await
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
        config: &ProbeRequestConfig,
        stream: bool,
    ) -> Result<(), ProviderError> {
        let protocol = config.resolved_protocol(provider_name);
        let body = config.build_body(upstream_name, &protocol, stream);
        match protocol {
            ProbeProtocol::AnthropicMessages => {
                if stream {
                    match adapter.messages_stream(endpoint, body).await {
                        Ok(mut response) => response.next().await.map(|_| ()).ok_or_else(|| {
                            ProviderError::new(
                                "Upstream returned an empty stream",
                                ErrorKind::Other,
                            )
                        }),
                        Err(error) => Err(error),
                    }
                } else {
                    adapter.messages(endpoint, body).await.map(|_| ())
                }
            }
            ProbeProtocol::Responses => adapter
                .relay(endpoint, "/v1/responses", body)
                .await
                .map(|_| ()),
            ProbeProtocol::OpenaiChat | ProbeProtocol::Auto => {
                if stream {
                    match adapter.chat_complete_stream(endpoint, body).await {
                        Ok(mut response) => response.next().await.map(|_| ()).ok_or_else(|| {
                            ProviderError::new(
                                "Upstream returned an empty stream",
                                ErrorKind::Other,
                            )
                        }),
                        Err(error) => Err(error),
                    }
                } else {
                    adapter.chat_complete(endpoint, body).await.map(|_| ())
                }
            }
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
