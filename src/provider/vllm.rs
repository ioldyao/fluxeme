use std::pin::Pin;
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

pub struct VllmAdapter;

impl VllmAdapter {
    async fn send_request(
        &self,
        endpoint: &EndpointConfig,
        path: &str,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let mut headers = HeaderMap::new();
        if !endpoint.api_key.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key)).map_err(|e| {
                    ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other)
                })?,
            );
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        self.send_json(endpoint, path, body, headers).await
    }

    /// Send a JSON request with the given headers to `endpoint.url + path`.
    /// `path` starts with `/v1` and `endpoint.url` may or may not end in
    /// `/v1` — the duplicate is stripped.
    async fn send_json(
        &self,
        endpoint: &EndpointConfig,
        path: &str,
        body: Value,
        headers: HeaderMap,
    ) -> Result<Value, ProviderError> {
        let client = shared_client();
        let url = super::resolve_endpoint_url(endpoint, path).await?;

        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        let timeout = request_timeout(
            &RequestKind::Unary { body_size },
            endpoint,
            &default_config(),
        );
        tracing::info!(
            endpoint = %endpoint.url,
            body_size = %body_size,
            timeout_ms = timeout.as_millis(),
            path = %path,
            "Sending request to upstream (vllm)"
        );

        let resp_start = Instant::now();
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
                "VLLM upstream request failed"
            );
            ProviderError::new(format!("Request failed: {}", e), kind)
        })?;

        let status = resp.status();
        tracing::info!(
            endpoint = %endpoint.url,
            ttfb_ms = resp_start.elapsed().as_millis(),
            status = status.as_u16(),
            "Upstream response header received (vllm)"
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
            "Upstream full response received (vllm)"
        );

        if !status.is_success() {
            let resp_text = String::from_utf8_lossy(&body_resp);
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %resp_text, "vllm upstream request failed");
            let upstream_msg = serde_json::from_str::<serde_json::Value>(&resp_text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from))
                .unwrap_or(resp_text.trim().to_string());
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

    /// Native Anthropic-format request (vLLM serves /v1/messages directly).
    async fn send_anthropic_request(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let mut headers = HeaderMap::new();
        if !endpoint.api_key.is_empty() {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(&endpoint.api_key).map_err(|e| {
                    ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other)
                })?,
            );
        }
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        self.send_json(endpoint, "/v1/messages", body, headers)
            .await
    }
}

#[async_trait::async_trait]
impl ProviderAdapter for VllmAdapter {
    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        self.send_anthropic_request(endpoint, body).await
    }

    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        super::validate_endpoint_url(&endpoint.url).await?;
        let client = shared_client();

        let url = super::resolve_endpoint_url(endpoint, "/v1/messages").await?;

        let mut headers = HeaderMap::new();
        if !endpoint.api_key.is_empty() {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(&endpoint.api_key).map_err(|e| {
                    ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other)
                })?,
            );
        }
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(
            endpoint = %endpoint.url,
            body_size = %body_size,
            total_timeout_ms = timeout.as_millis(),
            "Sending stream request to upstream (vllm, anthropic format)"
        );

        let req = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .timeout(timeout);
        let response = req.send().await.map_err(|e| {
            let kind = classify_reqwest_error(&e);
            ProviderError::new(format!("Stream request failed: {}", e), kind)
        })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %text, "vllm anthropic-format stream request failed");
            let upstream_msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from))
                .unwrap_or(text.trim().to_string());
            return Err(ProviderError::new(
                format!(
                    "Upstream request failed with status {}: {}",
                    status.as_u16(),
                    upstream_msg
                ),
                kind,
            ));
        }

        let byte_stream = response.bytes_stream();
        let mapped = byte_stream.map(|chunk| match chunk {
            Ok(bytes) => String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).to_string()),
            Err(e) => format!("data: {{\"error\":\"{}\"}}\n\n", e),
        });

        Ok(Pin::from(Box::new(mapped)))
    }

    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        self.send_request(endpoint, "/v1/chat/completions", body)
            .await
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        super::validate_endpoint_url(&endpoint.url).await?;
        let client = shared_client();

        let url = super::resolve_endpoint_url(endpoint, "/v1/chat/completions").await?;

        let mut headers = HeaderMap::new();
        if !endpoint.api_key.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key)).map_err(|e| {
                    ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other)
                })?,
            );
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(
            endpoint = %endpoint.url,
            body_size = %body_size,
            total_timeout_ms = timeout.as_millis(),
            "Sending stream request to upstream (vllm)"
        );

        let req = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .timeout(timeout);
        let response = req.send().await.map_err(|e| {
            let kind = classify_reqwest_error(&e);
            ProviderError::new(format!("Stream request failed: {}", e), kind)
        })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %body, "vllm upstream stream request failed");
            let upstream_msg = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from))
                .unwrap_or(body.trim().to_string());
            return Err(ProviderError::new(
                format!(
                    "Upstream request failed with status {}: {}",
                    status.as_u16(),
                    upstream_msg
                ),
                kind,
            ));
        }

        let byte_stream = response.bytes_stream();
        let mapped = byte_stream.map(|chunk| match chunk {
            Ok(bytes) => String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).to_string()),
            Err(e) => format!("data: {{\"error\":\"{}\"}}\n\n", e),
        });

        Ok(Pin::from(Box::new(mapped)))
    }

    async fn relay(
        &self,
        endpoint: &EndpointConfig,
        path: &str,
        body: Value,
    ) -> Result<Value, ProviderError> {
        self.send_request(endpoint, path, body).await
    }
}
