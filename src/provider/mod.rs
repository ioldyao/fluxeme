pub mod openai;
pub mod anthropic;
pub mod vllm;

use std::pin::Pin;
use std::sync::Arc;

use futures::stream::Stream;
use serde_json::Value;

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

    /// Relay a request to a provider-specific endpoint (e.g. /v1/completions, /tokenize).
    /// Each provider implements only the paths it supports.
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
