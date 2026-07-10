pub mod openai;
pub mod anthropic;
pub mod vllm;

use std::net::{IpAddr, ToSocketAddrs};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::OnceLock;

use futures::stream::Stream;
use serde_json::Value;
use url::Url;

use crate::config::types::EndpointConfig;

pub type StreamResult = Pin<Box<dyn Stream<Item = String> + Send>>;

#[derive(Debug)]
pub struct ProviderError(pub String);

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ProviderError {}

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
        Err(ProviderError(format!(
            "Relay not supported for path: {}",
            path
        )))
    }
}

/// Validate that an endpoint URL doesn't resolve to a private/reserved IP (SSRF protection).
pub fn validate_endpoint_url(url_str: &str) -> Result<(), ProviderError> {
    let parsed = Url::parse(url_str).map_err(|_| {
        ProviderError("Invalid endpoint URL format".into())
    })?;
    let host = parsed.host_str().ok_or_else(|| {
        ProviderError("Endpoint URL has no host".into())
    })?;

    // Check if host is an IP literal
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(ProviderError(
                "SSRF blocked: endpoint resolves to a private or reserved IP address".into(),
            ));
        }
        return Ok(());
    }

    // Resolve hostname to IP addresses
    let addr_iter = format!("{}:0", host).to_socket_addrs().map_err(|_| {
        ProviderError(format!("Failed to resolve endpoint host: {}", host))
    })?;

    for addr in addr_iter {
        if is_private_ip(&addr.ip()) {
            return Err(ProviderError(
                "SSRF blocked: endpoint resolves to a private or reserved IP address".into(),
            ));
        }
    }

    Ok(())
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.to_ipv4_mapped().is_some_and(|v4| {
                    v4.is_private() || v4.is_loopback()
                })
        }
    }
}

fn shared_client() -> Arc<reqwest::Client> {
    static CLIENT: OnceLock<Arc<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Arc::new(
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(60))
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
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            openai: Arc::new(openai::OpenAIAdapter),
            anthropic: Arc::new(anthropic::AnthropicAdapter),
            vllm: Arc::new(vllm::VllmAdapter),
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn ProviderAdapter>> {
        match name {
            "openai" | "azure" | "ollama" => Some(self.openai.clone()),
            "anthropic" => Some(self.anthropic.clone()),
            "vllm" => Some(self.vllm.clone()),
            _ => None,
        }
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
