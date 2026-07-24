pub mod anthropic;
pub mod anthropic_compat;
pub mod generic;
pub mod openai;
pub mod vllm;

use std::net::IpAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use futures::stream::Stream;
use serde_json::Value;
use url::Url;

use crate::config::types::{EndpointConfig, GatewayRuntimeConfig};

pub type StreamResult = Pin<Box<dyn Stream<Item = String> + Send>>;

// ── Error types ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorKind {
    Timeout,
    ConnectFailed,
    RateLimited,
    Upstream5xx,
    Upstream4xx,
    Parse,
    Other,
}

#[derive(Debug)]
pub struct ProviderError(pub String, pub ErrorKind);

impl ProviderError {
    pub fn kind(&self) -> ErrorKind {
        self.1
    }

    pub fn new(msg: impl Into<String>, kind: ErrorKind) -> Self {
        Self(msg.into(), kind)
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ProviderError {}

/// Classify a reqwest transport error (from `.send().await`).
pub fn classify_reqwest_error(e: &reqwest::Error) -> ErrorKind {
    if e.is_timeout() {
        ErrorKind::Timeout
    } else if e.is_connect() {
        ErrorKind::ConnectFailed
    } else {
        ErrorKind::Other
    }
}

/// Classify an HTTP response status code.
pub fn classify_status(code: u16) -> ErrorKind {
    match code {
        429 => ErrorKind::RateLimited,
        500..=599 => ErrorKind::Upstream5xx,
        400..=499 => ErrorKind::Upstream4xx,
        _ => ErrorKind::Other,
    }
}

/// Determine whether a ProviderError should be retried.
pub fn is_retryable_error(e: &ProviderError) -> bool {
    matches!(
        e.kind(),
        ErrorKind::Timeout | ErrorKind::Upstream5xx | ErrorKind::RateLimited
    )
}

// ── Request kind & timeout calculation ──────────────────────────────

pub enum RequestKind {
    /// Non-streaming: total timeout = base + body_size extra
    Unary { body_size: usize },
    /// Streaming: loose 600s fallback; primary control is idle timeout.
    Streaming,
}

/// Compute per-request timeout.
///
/// Unary: base = endpoint.timeout_secs ?? config.unary_base_timeout_secs,
///        plus body_size_extra_secs_per_100kb * (body_size / 100_000).
///
/// Streaming: returns stream_total_timeout_secs as a loose safety net.
///            The primary disconnect mechanism is the idle timeout in
///            tokio::select!, not this total timeout.
pub fn request_timeout(
    kind: &RequestKind,
    endpoint: &EndpointConfig,
    config: &GatewayRuntimeConfig,
) -> Duration {
    match kind {
        RequestKind::Unary { body_size } => {
            let base = endpoint
                .timeout_secs
                .unwrap_or(config.unary_base_timeout_secs);
            let extra = (body_size / 100_000) as u64 * config.body_size_extra_secs_per_100kb;
            Duration::from_secs(base + extra)
        }
        RequestKind::Streaming => Duration::from_secs(config.stream_total_timeout_secs),
    }
}

/// Default config (fallback when AppState not available).
pub fn default_config() -> GatewayRuntimeConfig {
    GatewayRuntimeConfig::default()
}

#[async_trait::async_trait]
pub trait ProviderAdapter: Send + Sync {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError>;

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError>;

    /// Handle native-format /v1/messages request (e.g. Anthropic format).
    /// Default delegates to chat_complete (which applies format conversion).
    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        self.chat_complete(endpoint, body).await
    }

    /// Handle streaming native-format /v1/messages request.
    /// Default delegates to chat_complete_stream.
    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        self.chat_complete_stream(endpoint, body).await
    }

    async fn relay(
        &self,
        _endpoint: &EndpointConfig,
        path: &str,
        _body: Value,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::new(
            format!("Relay not supported for path: {}", path),
            ErrorKind::Other,
        ))
    }
}

/// Global flag to allow private IP addresses (disables SSRF protection).
static ALLOW_PRIVATE_IPS: AtomicBool = AtomicBool::new(true);

/// Set whether private IPs are allowed.
pub fn set_allow_private_ips(allow: bool) {
    ALLOW_PRIVATE_IPS.store(allow, Ordering::Relaxed);
    tracing::info!(
        "Private IP access: {}",
        if allow { "ALLOWED" } else { "BLOCKED" }
    );
}

/// Validate that an endpoint URL doesn't resolve to a private/reserved IP (SSRF protection).
pub async fn validate_endpoint_url(url_str: &str) -> Result<(), ProviderError> {
    if ALLOW_PRIVATE_IPS.load(Ordering::Relaxed) {
        return Ok(());
    }

    let parsed = Url::parse(url_str)
        .map_err(|_| ProviderError::new("Invalid endpoint URL format", ErrorKind::Other))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| ProviderError::new("Endpoint URL has no host", ErrorKind::Other))?;

    // Check if host is an IP literal
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(ProviderError::new(
                "SSRF blocked: endpoint resolves to a private or reserved IP address",
                ErrorKind::Other,
            ));
        }
        return Ok(());
    }

    // Resolve hostname to IP addresses (async to avoid blocking the runtime)
    let addr_str = format!("{}:0", host);
    let result = tokio::net::lookup_host(&addr_str).await;
    match result {
        Ok(addrs) => {
            for addr in addrs {
                if is_private_ip(&addr.ip()) {
                    return Err(ProviderError::new(
                        "SSRF blocked: endpoint resolves to a private or reserved IP address",
                        ErrorKind::Other,
                    ));
                }
            }
            Ok(())
        }
        Err(_) => Err(ProviderError::new(
            format!("Failed to resolve endpoint host: {}", host),
            ErrorKind::Other,
        )),
    }
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.is_private() || v4.is_loopback())
        }
    }
}

// ── Generic relay / proxy request ─────────────────────────────────

/// Send a raw JSON body to an upstream endpoint at `endpoint.url + path`.
/// Used by adapters' `relay()` methods for endpoints that don't need
/// format conversion (e.g. `/v1/completions`, `/v1/embeddings`).
pub async fn relay_request(
    endpoint: &EndpointConfig,
    path: &str,
    body: Value,
    provider_name: &str,
) -> Result<Value, ProviderError> {
    validate_endpoint_url(&endpoint.url).await?;
    let client = shared_client();

    let base = endpoint.url.trim_end_matches('/');
    let url = if base.ends_with("/v1") && path.starts_with("/v1") {
        format!(
            "{}{}",
            base.trim_end_matches("/v1").trim_end_matches('/'),
            path
        )
    } else {
        format!("{}{}", base, path)
    };

    let mut headers = reqwest::header::HeaderMap::new();
    if !endpoint.api_key.is_empty() {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key))
                .map_err(|e| {
                    ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other)
                })?,
        );
    }
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/json"),
    );

    let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
    let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
    tracing::info!(
        endpoint = %endpoint.url,
        body_size = %body_size,
        timeout_ms = timeout.as_millis(),
        path = %path,
        provider = %provider_name,
        "Sending relay request to upstream"
    );

    let resp_start = std::time::Instant::now();
    let req = client
        .post(&url)
        .headers(headers)
        .json(&body)
        .timeout(timeout);
    let resp = req.send().await.map_err(|e| {
        let kind = classify_reqwest_error(&e);
        tracing::error!(
            endpoint = %endpoint.url,
            error = %e,
            error_kind = ?kind,
            elapsed_ms = resp_start.elapsed().as_millis(),
            provider = %provider_name,
            "Relay upstream request failed"
        );
        ProviderError::new(format!("Request failed: {}", e), kind)
    })?;

    let status = resp.status();
    tracing::info!(
        endpoint = %endpoint.url,
        ttfb_ms = resp_start.elapsed().as_millis(),
        status = status.as_u16(),
        provider = %provider_name,
        "Relay upstream response header received"
    );

    let body_resp = resp.bytes().await.map_err(|e| {
        ProviderError::new(
            format!("Failed to read response body: {}", e),
            ErrorKind::Parse,
        )
    })?;
    tracing::info!(
        endpoint = %endpoint.url,
        body_size = body_resp.len(),
        total_ms = resp_start.elapsed().as_millis(),
        provider = %provider_name,
        "Relay upstream full response received"
    );

    if !status.is_success() {
        let resp_text = String::from_utf8_lossy(&body_resp);
        let kind = classify_status(status.as_u16());
        tracing::error!(%status, body = %resp_text, provider = %provider_name, "Relay upstream request failed");
        return Err(ProviderError::new(
            format!("Upstream request failed with status {}", status.as_u16()),
            kind,
        ));
    }

    let resp_body: Value = serde_json::from_slice(&body_resp).map_err(|e| {
        ProviderError::new(format!("Failed to parse response: {}", e), ErrorKind::Parse)
    })?;
    Ok(resp_body)
}

fn shared_client() -> Arc<reqwest::Client> {
    static CLIENT: OnceLock<Arc<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Arc::new(
                reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(10))
                    .timeout(Duration::from_secs(600))
                    .tcp_keepalive(Duration::from_secs(15))
                    .pool_max_idle_per_host(100)
                    .pool_idle_timeout(Duration::from_secs(90))
                    .http2_adaptive_window(true)
                    .build()
                    .expect("Failed to build reqwest client"),
            )
        })
        .clone()
}

pub struct ProviderRegistry {
    openai: Arc<openai::OpenAIAdapter>,
    anthropic: Arc<anthropic::AnthropicAdapter>,
    vllm: Arc<vllm::VllmAdapter>,
    sglang: Arc<generic::GenericAdapter>,
    deepseek: Arc<generic::GenericAdapter>,
    dashscope: Arc<generic::GenericAdapter>,
    zhipu: Arc<generic::GenericAdapter>,
    minimax: Arc<generic::GenericAdapter>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            openai: Arc::new(openai::OpenAIAdapter),
            anthropic: Arc::new(anthropic::AnthropicAdapter),
            vllm: Arc::new(vllm::VllmAdapter),
            sglang: Arc::new(generic::GenericAdapter::sglang()),
            deepseek: Arc::new(generic::GenericAdapter::deepseek()),
            dashscope: Arc::new(generic::GenericAdapter::dashscope()),
            zhipu: Arc::new(generic::GenericAdapter::zhipu()),
            minimax: Arc::new(generic::GenericAdapter::minimax()),
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn ProviderAdapter>> {
        match name {
            "openai" => Some(self.openai.clone()),
            "anthropic" => Some(self.anthropic.clone()),
            "vllm" => Some(self.vllm.clone()),
            "sglang" => Some(self.sglang.clone()),
            "deepseek" => Some(self.deepseek.clone()),
            "dashscope" => Some(self.dashscope.clone()),
            "zhipu" => Some(self.zhipu.clone()),
            "minimax" => Some(self.minimax.clone()),
            "azure" | "ollama" => Some(self.openai.clone()),
            _ => None,
        }
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
