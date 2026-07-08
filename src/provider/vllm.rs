use std::pin::Pin;

use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, AUTHORIZATION};
use serde_json::Value;

use super::{ProviderAdapter, ProviderError, StreamResult};
use crate::config::types::EndpointConfig;

pub struct VllmAdapter;

impl VllmAdapter {
    async fn send_request(
        &self,
        endpoint: &EndpointConfig,
        path: &str,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(endpoint.timeout_secs.unwrap_or(300)))
            .build()
            .map_err(|e| ProviderError(format!("Failed to create client: {}", e)))?;

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

        let mut headers = HeaderMap::new();
        if !endpoint.api_key.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key))
                    .map_err(|e| ProviderError(format!("Invalid API key: {}", e)))?,
            );
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let resp = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError(format!("Request failed: {}", e)))?;

        let status = resp.status();
        let resp_body: Value = resp
            .json()
            .await
            .map_err(|e| ProviderError(format!("Failed to parse response: {}", e)))?;

        if !status.is_success() {
            return Err(ProviderError(format!(
                "Upstream returned {}: {}",
                status, resp_body
            )));
        }

        Ok(resp_body)
    }
}

#[async_trait::async_trait]
impl ProviderAdapter for VllmAdapter {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        self.send_request(endpoint, "/v1/chat/completions", body).await
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(endpoint.timeout_secs.unwrap_or(600)))
            .build()
            .map_err(|e| ProviderError(format!("Failed to create client: {}", e)))?;

        let base = endpoint.url.trim_end_matches('/').trim_end_matches("/v1");
        let url = format!("{}/v1/chat/completions", base);

        let mut headers = HeaderMap::new();
        if !endpoint.api_key.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key))
                    .map_err(|e| ProviderError(format!("Invalid API key: {}", e)))?,
            );
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let response = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError(format!("Stream request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_default();
            return Err(ProviderError(format!(
                "Upstream returned {}: {}",
                status, body
            )));
        }

        let byte_stream = response.bytes_stream();
        let mapped = byte_stream.map(|chunk| match chunk {
            Ok(bytes) => String::from_utf8_lossy(&bytes).to_string(),
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
