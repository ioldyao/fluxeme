use std::pin::Pin;

use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::Value;

use super::{ProviderAdapter, ProviderError, StreamResult};
use crate::config::types::EndpointConfig;
use crate::provider::shared_client;

fn build_anthropic_headers(endpoint: &EndpointConfig) -> Result<HeaderMap, ProviderError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-api-key",
        HeaderValue::from_str(&endpoint.api_key)
            .map_err(|e| ProviderError(format!("Invalid API key: {}", e)))?,
    );
    headers.insert(
        "anthropic-version",
        HeaderValue::from_static("2023-06-01"),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    Ok(headers)
}

pub struct AnthropicAdapter;

#[async_trait::async_trait]
impl ProviderAdapter for AnthropicAdapter {
    async fn chat_complete(
        &self,
        _endpoint: &EndpointConfig,
        _body: Value,
    ) -> Result<Value, ProviderError> {
        Err(ProviderError(
            "Anthropic provider only supports /v1/messages, not /v1/chat/completions".into(),
        ))
    }

    async fn chat_complete_stream(
        &self,
        _endpoint: &EndpointConfig,
        _body: Value,
    ) -> Result<StreamResult, ProviderError> {
        Err(ProviderError(
            "Anthropic provider only supports /v1/messages, not /v1/chat/completions".into(),
        ))
    }

    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        super::validate_endpoint_url(&endpoint.url)?;
        let client = shared_client();

        let url = format!("{}/v1/messages", endpoint.url.trim_end_matches('/'));
        let headers = build_anthropic_headers(endpoint)?;

        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        tracing::info!(endpoint = %endpoint.url, body_size = %body_size, "Sending request to upstream (anthropic)");

        let req = client.post(&url).headers(headers).json(&body);
        let resp = req.send().await.map_err(|e| {
                tracing::error!(endpoint = %endpoint.url, error = %e, "Upstream HTTP request failed");
                ProviderError(format!("Request failed: {}", e))
            })?;

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

    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        super::validate_endpoint_url(&endpoint.url)?;
        let client = shared_client();

        let url = format!("{}/v1/messages", endpoint.url.trim_end_matches('/'));
        let headers = build_anthropic_headers(endpoint)?;

        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        tracing::info!(endpoint = %endpoint.url, body_size = %body_size, "Sending stream request to upstream (anthropic)");

        let req = client.post(&url).headers(headers).json(&body);
        let response = req.send().await
            .map_err(|e| ProviderError(format!("Stream request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            tracing::error!(%status, body = %text, "anthropic upstream stream request failed");
            return Err(ProviderError(format!(
                "Upstream request failed with status {}",
                status.as_u16()
            )));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(1024);
        let mut byte_stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut buffer = String::new();

            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));

                        while let Some(pos) = buffer.find("\n\n") {
                            let event = buffer[..pos + 2].to_string();
                            buffer = buffer[pos + 2..].to_string();
                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        let _ =
                            tx.send(format!("data: {{\"error\":\"{}\"}}\n\n", e))
                                .await;
                        break;
                    }
                }
            }

            if !buffer.is_empty() {
                let _ = tx.send(buffer).await;
            }
            let _ = tx.send("data: [DONE]\n\n".to_string()).await;
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Pin::from(Box::new(stream)))
    }

    async fn relay(
        &self,
        endpoint: &EndpointConfig,
        path: &str,
        body: Value,
    ) -> Result<Value, ProviderError> {
        super::validate_endpoint_url(&endpoint.url)?;
        let client = shared_client();

        let url = format!(
            "{}/{}",
            endpoint.url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let headers = build_anthropic_headers(endpoint)?;

        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        tracing::info!(endpoint = %endpoint.url, body_size = %body_size, "Sending relay request to upstream (anthropic)");

        let req = client.post(&url).headers(headers).json(&body);
        let resp = req.send().await
            .map_err(|e| ProviderError(format!("Request failed: {}", e)))?;

        let status = resp.status();
        let resp_body: Value = resp
            .json()
            .await
            .map_err(|e| ProviderError(format!("Failed to parse response: {}", e)))?;

        if !status.is_success() {
            tracing::error!(%status, body = %resp_body, "anthropic relay request failed");
            return Err(ProviderError(format!(
                "Upstream request failed with status {}",
                status.as_u16()
            )));
        }

        Ok(resp_body)
    }
}
