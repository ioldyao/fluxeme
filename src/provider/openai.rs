use std::pin::Pin;

use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, AUTHORIZATION};
use serde_json::Value;

use super::{ProviderAdapter, ProviderError, StreamResult};
use crate::config::types::EndpointConfig;
use crate::provider::shared_client;

pub struct OpenAIAdapter;

#[async_trait::async_trait]
impl ProviderAdapter for OpenAIAdapter {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        super::validate_endpoint_url(&endpoint.url)?;
        let client = shared_client();
        let base = endpoint.url.trim_end_matches('/').trim_end_matches("/v1");
        let url = format!("{}/v1/chat/completions", base);

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key))
                .map_err(|e| ProviderError(format!("Invalid API key: {}", e)))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        tracing::info!(endpoint = %endpoint.url, "Sending request to upstream (openai)");

        let req = client.post(&url).headers(headers).json(&body);
        let resp = req.send().await.map_err(|e| {
                tracing::error!(endpoint = %endpoint.url, error = %e, "OpenAI upstream HTTP request failed");
                ProviderError(format!("Request failed: {}", e))
            })?;

        let status = resp.status();
        let resp_body: Value = resp
            .json()
            .await
            .map_err(|e| ProviderError(format!("Failed to parse response: {}", e)))?;

        if !status.is_success() {
            tracing::error!(%status, body = %resp_body, "openai upstream request failed");
            return Err(ProviderError(format!(
                "Upstream request failed with status {}",
                status.as_u16()
            )));
        }

        Ok(resp_body)
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        super::validate_endpoint_url(&endpoint.url)?;
        let client = shared_client();

        let base = endpoint.url.trim_end_matches('/').trim_end_matches("/v1");
        let url = format!("{}/v1/chat/completions", base);

        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key))
                .map_err(|e| ProviderError(format!("Invalid API key: {}", e)))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let req = client.post(&url).headers(headers).json(&body);
        let response = req.send().await
            .map_err(|e| ProviderError(format!("Stream request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_default();
            tracing::error!(%status, body = %body, "openai upstream stream request failed");
            return Err(ProviderError(format!(
                "Upstream request failed with status {}",
                status.as_u16()
            )));
        }

        let byte_stream = response.bytes_stream();
        let mapped = byte_stream.map(|chunk| match chunk {
            Ok(bytes) => String::from_utf8(bytes.to_vec()).unwrap_or_else(|e| {
                String::from_utf8_lossy(e.as_bytes()).to_string()
            }),
            Err(e) => format!("data: {{\"error\":\"{}\"}}\n\n", e),
        });

        Ok(Pin::from(Box::new(mapped)))
    }
}
