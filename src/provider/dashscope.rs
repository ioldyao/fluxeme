/// DashScope (阿里云百炼 Bailian) provider adapter.
///
/// Supports two compatible modes auto-detected from endpoint URL:
/// - **OpenAI Compatible**: URL contains `/compatible-mode` → `base/compatible-mode/v1/chat/completions` with `Authorization: Bearer`
/// - **Anthropic Compatible**: URL contains `/apps/anthropic` → `base/apps/anthropic/v1/messages` with `x-api-key` or `Authorization: Bearer`
///
/// Routing (no format conversion):
/// - Client `/v1/chat/completions` → DashScope OpenAI Compatible
/// - Client `/v1/messages` → DashScope Anthropic Compatible
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;

use super::{
    classify_reqwest_error, classify_status, default_config, request_timeout, ErrorKind,
    ProviderAdapter, ProviderError, RequestKind, StreamResult,
};
use crate::config::types::EndpointConfig;
use crate::provider::shared_client;

pub struct DashScopeAdapter;

#[derive(Debug)]
enum DashScopeMode {
    OpenAI,
    Anthropic,
}

impl DashScopeAdapter {
    pub fn new() -> Self {
        Self
    }

    fn detect_mode(endpoint: &EndpointConfig) -> Result<DashScopeMode, ProviderError> {
        let url = &endpoint.url;
        if url.contains("/apps/anthropic") {
            Ok(DashScopeMode::Anthropic)
        } else if url.contains("/compatible-mode") {
            Ok(DashScopeMode::OpenAI)
        } else {
            Err(ProviderError::new(
                format!(
                    "Unsupported DashScope URL: {}. Expected URL to contain '/apps/anthropic' \
                     (Anthropic compatible) or '/compatible-mode' (OpenAI compatible). \
                     Examples: https://{{WorkspaceId}}.cn-beijing.maas.aliyuncs.com/apps/anthropic \
                     or https://{{WorkspaceId}}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
                    url
                ),
                ErrorKind::Other,
            ))
        }
    }

    fn build_headers(
        endpoint: &EndpointConfig,
        mode: &DashScopeMode,
    ) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        match mode {
            DashScopeMode::OpenAI => {
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key)).map_err(
                        |e| ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other),
                    )?,
                );
            }
            DashScopeMode::Anthropic => {
                headers.insert(
                    "x-api-key",
                    HeaderValue::from_str(&endpoint.api_key).map_err(|e| {
                        ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other)
                    })?,
                );
                headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
            }
        }

        Ok(headers)
    }

    async fn build_chat_completions_url(
        endpoint: &EndpointConfig,
    ) -> Result<String, ProviderError> {
        super::validate_endpoint_url(&endpoint.url).await?;
        let base = endpoint.url.trim_end_matches('/').trim_end_matches("/v1");
        Ok(format!("{}/v1/chat/completions", base))
    }

    async fn build_messages_url(endpoint: &EndpointConfig) -> Result<String, ProviderError> {
        super::validate_endpoint_url(&endpoint.url).await?;
        let base = endpoint.url.trim_end_matches('/');
        if base.ends_with("/apps/anthropic") {
            Ok(format!("{}/v1/messages", base))
        } else if base.ends_with("/compatible-mode") {
            // If user configured OpenAI URL but sends /v1/messages, that's a mismatch
            // We still route to OpenAI chat completions as fallback
            Ok(format!("{}/v1/chat/completions", base))
        } else {
            // Fallback: assume Anthropic compatible at base URL
            Ok(format!("{}/apps/anthropic/v1/messages", base))
        }
    }

    async fn build_count_tokens_url(endpoint: &EndpointConfig) -> Result<String, ProviderError> {
        let messages_url = Self::build_messages_url(endpoint).await?;
        Ok(format!("{}count_tokens", messages_url))
    }

    async fn do_send(
        client: Arc<reqwest::Client>,
        url: &str,
        headers: HeaderMap,
        body: &Value,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        let _body_size = serde_json::to_string(body).map(|s| s.len()).unwrap_or(0);
        let resp_start = Instant::now();

        let resp = client
            .post(url)
            .headers(headers)
            .json(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                let kind = classify_reqwest_error(&e);
                tracing::error!(
                    url = %url,
                    error = %e,
                    error_kind = ?kind,
                    elapsed_ms = resp_start.elapsed().as_millis(),
                    "Upstream HTTP request failed"
                );
                ProviderError::new(format!("Request failed: {}", e), kind)
            })?;

        let status = resp.status();
        let body_resp = resp.bytes().await.map_err(|e| {
            ProviderError::new(
                format!("Failed to read response body: {}", e),
                ErrorKind::Parse,
            )
        })?;

        if !status.is_success() {
            let resp_text = String::from_utf8_lossy(&body_resp);
            let upstream_msg = serde_json::from_str::<Value>(&resp_text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from))
                .unwrap_or(resp_text.trim().to_string());
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %resp_text, "Upstream request failed");
            return Err(ProviderError::new(
                format!(
                    "Upstream request failed with status {}: {}",
                    status.as_u16(),
                    upstream_msg
                ),
                kind,
            ));
        }

        let resp_body: Value = serde_json::from_slice(&body_resp).map_err(|e| {
            ProviderError::new(format!("Failed to parse response: {}", e), ErrorKind::Parse)
        })?;
        Ok(resp_body)
    }

    async fn do_send_stream(
        client: Arc<reqwest::Client>,
        url: &str,
        headers: HeaderMap,
        body: &Value,
        timeout: Duration,
    ) -> Result<StreamResult, ProviderError> {
        let resp = client
            .post(url)
            .headers(headers)
            .json(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                let kind = classify_reqwest_error(&e);
                tracing::error!(
                    url = %url,
                    error = %e,
                    error_kind = ?kind,
                    "Upstream stream request failed"
                );
                ProviderError::new(format!("Stream request failed: {}", e), kind)
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %body_text, "Upstream stream request failed");
            return Err(ProviderError::new(
                format!(
                    "Stream request failed with status {}: {}",
                    status.as_u16(),
                    body_text.trim()
                ),
                kind,
            ));
        }

        let byte_stream = resp.bytes_stream();
        let mapped = byte_stream.map(|chunk| match chunk {
            Ok(bytes) => String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).to_string()),
            Err(e) => format!("data: {{\"error\":\"{}\"}}\n\n", e),
        });

        Ok(Pin::from(Box::new(mapped)))
    }
}

#[async_trait::async_trait]
impl ProviderAdapter for DashScopeAdapter {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let mode = Self::detect_mode(endpoint)?;
        let url = Self::build_chat_completions_url(endpoint).await?;
        let headers = Self::build_headers(endpoint, &mode)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, mode = ?mode, "DashScope chat_completions → {}", url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let mode = Self::detect_mode(endpoint)?;
        let url = Self::build_chat_completions_url(endpoint).await?;
        let headers = Self::build_headers(endpoint, &mode)?;
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(endpoint = %endpoint.url, mode = ?mode, "DashScope chat_completions_stream → {}", url);
        Self::do_send_stream(shared_client(), &url, headers, &body, timeout).await
    }

    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let mode = Self::detect_mode(endpoint)?;
        let url = Self::build_messages_url(endpoint).await?;
        let headers = Self::build_headers(endpoint, &mode)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, mode = ?mode, "DashScope messages → {}", url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let mode = Self::detect_mode(endpoint)?;
        let url = Self::build_messages_url(endpoint).await?;
        let headers = Self::build_headers(endpoint, &mode)?;
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(endpoint = %endpoint.url, mode = ?mode, "DashScope messages_stream → {}", url);
        Self::do_send_stream(shared_client(), &url, headers, &body, timeout).await
    }

    async fn count_tokens(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let mode = Self::detect_mode(endpoint)?;
        let url = Self::build_count_tokens_url(endpoint).await?;
        let headers = Self::build_headers(endpoint, &mode)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, mode = ?mode, "DashScope count_tokens → {}", url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }
}
