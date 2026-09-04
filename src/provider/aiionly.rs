//! AiOnly adapter for the provider's native OpenAI and Anthropic endpoints.
//!
//! AiOnly exposes both protocols from the same root URL, but uses different
//! authentication headers and has a narrower Anthropic request surface than
//! the current Claude Code client. Compatibility filtering is intentionally
//! scoped to this adapter so other Anthropic providers retain full semantics.

use std::pin::Pin;
use std::time::Instant;

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};

use super::{
    classify_reqwest_error, classify_status, default_config, request_timeout, ErrorKind,
    ProviderAdapter, ProviderError, RequestKind, StreamResult,
};
use crate::config::types::EndpointConfig;
use crate::provider::shared_client;

const DEFAULT_BASE: &str = "https://llm.aiionly.com";
const CHAT_PATH: &str = "/v1/chat/completions";
const MESSAGES_PATH: &str = "/v1/messages";

pub struct AiOnlyAdapter;

impl AiOnlyAdapter {
    pub fn new() -> Self {
        Self
    }

    async fn url(endpoint: &EndpointConfig, path: &str) -> Result<String, ProviderError> {
        if endpoint.full_url {
            return Ok(endpoint.url.clone());
        }
        super::validate_endpoint_url(&endpoint.url).await?;
        let base = endpoint.url.trim_end_matches('/');
        let base = if base.is_empty() { DEFAULT_BASE } else { base };
        Ok(format!("{base}{path}"))
    }

    fn chat_headers(endpoint: &EndpointConfig) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key)).map_err(|e| {
                ProviderError::new(format!("Invalid API key: {e}"), ErrorKind::Other)
            })?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    fn messages_headers(endpoint: &EndpointConfig) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&endpoint.api_key).map_err(|e| {
                ProviderError::new(format!("Invalid API key: {e}"), ErrorKind::Other)
            })?,
        );
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(headers)
    }

    async fn send(
        endpoint: &EndpointConfig,
        path: &str,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<Value, ProviderError> {
        let url = Self::url(endpoint, path).await?;
        let body_size = body.to_string().len();
        let timeout = request_timeout(
            &RequestKind::Unary { body_size },
            endpoint,
            &default_config(),
        );
        let started = Instant::now();
        let response = shared_client()
            .post(&url)
            .headers(headers)
            .json(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                let kind = classify_reqwest_error(&e);
                tracing::error!(endpoint = %url, error = %e, error_kind = ?kind, "AiOnly upstream request failed");
                ProviderError::new(format!("Request failed: {e}"), kind)
            })?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|e| {
            ProviderError::new(
                format!("Failed to read response body: {e}"),
                ErrorKind::Parse,
            )
        })?;
        tracing::info!(endpoint = %url, status = status.as_u16(), elapsed_ms = started.elapsed().as_millis(), "AiOnly response received");
        if !status.is_success() {
            let text = String::from_utf8_lossy(&bytes);
            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
                .unwrap_or_else(|| text.trim().to_owned());
            return Err(ProviderError::new(
                format!(
                    "Upstream request failed with status {}: {message}",
                    status.as_u16()
                ),
                classify_status(status.as_u16()),
            ));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            ProviderError::new(format!("Failed to parse response: {e}"), ErrorKind::Parse)
        })
    }

    async fn send_stream(
        endpoint: &EndpointConfig,
        path: &str,
        headers: HeaderMap,
        body: &Value,
    ) -> Result<StreamResult, ProviderError> {
        let url = Self::url(endpoint, path).await?;
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        let response = shared_client()
            .post(&url)
            .headers(headers)
            .json(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                let kind = classify_reqwest_error(&e);
                ProviderError::new(format!("Stream request failed: {e}"), kind)
            })?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError::new(
                format!(
                    "Stream request failed with status {}: {}",
                    status.as_u16(),
                    text.trim()
                ),
                classify_status(status.as_u16()),
            ));
        }
        let stream = response.bytes_stream().map(|chunk| match chunk {
            Ok(bytes) => String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
            Err(e) => format!("data: {{\"error\":\"{e}\"}}\n\n"),
        });
        Ok(Pin::from(Box::new(stream)))
    }
}

/// Remove only Anthropic features not listed in AiOnly's Messages API docs.
/// Standard Anthropic providers do not call this function.
fn ai_only_messages_body(body: &Value) -> Value {
    let mut body = body.clone();
    if let Some(object) = body.as_object_mut() {
        object.remove("context_management");
        object.remove("output_config");
        if let Some(Value::Array(blocks)) = object.get_mut("system") {
            let text = blocks
                .iter()
                .filter_map(|b| {
                    (b.get("type")?.as_str() == Some("text")).then(|| b.get("text")?.as_str())
                })
                .flatten()
                .collect::<Vec<_>>()
                .join("\n");
            object.insert("system".into(), Value::String(text));
        }
        if let Some(Value::Array(messages)) = object.get_mut("messages") {
            for message in messages {
                if let Some(Value::Array(blocks)) = message.get_mut("content") {
                    blocks.retain(|block| {
                        !matches!(
                            block.get("type").and_then(Value::as_str),
                            Some("thinking") | Some("redacted_thinking")
                        )
                    });
                    for block in blocks.iter_mut() {
                        if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                            if let Some(Value::Array(inner)) = block.get_mut("content") {
                                inner.retain(|part| {
                                    matches!(
                                        part.get("type").and_then(Value::as_str),
                                        Some("text") | Some("image")
                                    )
                                });
                            }
                        }
                    }
                    if blocks.is_empty() {
                        message["content"] = Value::String(String::new());
                    }
                }
            }
        }
    }
    body
}

#[async_trait]
impl ProviderAdapter for AiOnlyAdapter {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        Self::send(endpoint, CHAT_PATH, Self::chat_headers(endpoint)?, &body).await
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        Self::send_stream(endpoint, CHAT_PATH, Self::chat_headers(endpoint)?, &body).await
    }

    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let body = ai_only_messages_body(&body);
        Self::send(
            endpoint,
            MESSAGES_PATH,
            Self::messages_headers(endpoint)?,
            &body,
        )
        .await
    }

    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let body = ai_only_messages_body(&body);
        Self::send_stream(
            endpoint,
            MESSAGES_PATH,
            Self::messages_headers(endpoint)?,
            &body,
        )
        .await
    }

    async fn count_tokens(
        &self,
        _endpoint: &EndpointConfig,
        _body: Value,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError::new(
            "AiOnly does not document count_tokens",
            ErrorKind::Other,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(url: &str, full_url: bool) -> EndpointConfig {
        EndpointConfig {
            id: None,
            url: url.into(),
            api_key: "test-key".into(),
            weight: 1,
            timeout_secs: None,
            max_tokens: None,
            enabled: true,
            full_url,
        }
    }

    #[tokio::test]
    async fn builds_protocol_specific_urls() {
        let ep = endpoint(DEFAULT_BASE, false);
        assert_eq!(
            AiOnlyAdapter::url(&ep, CHAT_PATH).await.unwrap(),
            "https://llm.aiionly.com/v1/chat/completions"
        );
        assert_eq!(
            AiOnlyAdapter::url(&ep, MESSAGES_PATH).await.unwrap(),
            "https://llm.aiionly.com/v1/messages"
        );
    }

    #[tokio::test]
    async fn preserves_explicit_full_url() {
        let ep = endpoint("https://example.test/custom", true);
        assert_eq!(
            AiOnlyAdapter::url(&ep, MESSAGES_PATH).await.unwrap(),
            "https://example.test/custom"
        );
    }

    #[test]
    fn uses_protocol_specific_auth_headers() {
        let ep = endpoint(DEFAULT_BASE, false);
        assert_eq!(
            AiOnlyAdapter::chat_headers(&ep).unwrap()[AUTHORIZATION],
            "Bearer test-key"
        );
        assert_eq!(
            AiOnlyAdapter::messages_headers(&ep).unwrap()["x-api-key"],
            "test-key"
        );
        assert_eq!(
            AiOnlyAdapter::messages_headers(&ep).unwrap()["anthropic-version"],
            "2023-06-01"
        );
    }

    #[test]
    fn strips_unsupported_new_anthropic_fields_only_for_ai_only() {
        let body = json!({
            "context_management": {"edits": [{"type": "clear_thinking_20251015", "keep": "all"}]},
            "output_config": {"effort": "high"},
            "messages": [{"role": "assistant", "content": [
                {"type": "thinking", "signature": "sig", "thinking": "private"},
                {"type": "text", "text": "visible"},
                {"type": "tool_use", "id": "toolu_1", "name": "bash", "input": {"command": "ls"}}
            ]}]
        });
        let filtered = ai_only_messages_body(&body);
        assert!(filtered.get("context_management").is_none());
        assert!(filtered.get("output_config").is_none());
        let blocks = filtered["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert!(serde_json::to_string(&filtered).is_ok());
    }

    #[test]
    fn keeps_tool_result_text_and_image_blocks() {
        let body = json!({"messages": [{"role": "user", "content": [{
            "type": "tool_result", "tool_use_id": "toolu_1", "content": [
                {"type": "thinking", "thinking": "drop"},
                {"type": "text", "text": "keep"},
                {"type": "image", "source": {"type": "base64", "data": "abc"}}
            ]
        }]}]});
        let filtered = ai_only_messages_body(&body);
        let inner = filtered["messages"][0]["content"][0]["content"]
            .as_array()
            .unwrap();
        assert_eq!(inner.len(), 2);
        assert_eq!(inner[0]["type"], "text");
        assert_eq!(inner[1]["type"], "image");
    }
}
