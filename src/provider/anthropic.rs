use std::pin::Pin;
use std::time::Instant;

use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::Value;

use super::{
    classify_reqwest_error, classify_status, default_config, request_timeout, ErrorKind,
    ProviderAdapter, ProviderError, RequestKind, StreamResult,
};
use crate::config::types::EndpointConfig;
use crate::provider::shared_client;

fn build_anthropic_headers(endpoint: &EndpointConfig) -> Result<HeaderMap, ProviderError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-api-key",
        HeaderValue::from_str(&endpoint.api_key)
            .map_err(|e| ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other))?,
    );
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
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
        Err(ProviderError::new(
            "Anthropic provider only supports /v1/messages, not /v1/chat/completions",
            ErrorKind::Other,
        ))
    }

    async fn chat_complete_stream(
        &self,
        _endpoint: &EndpointConfig,
        _body: Value,
    ) -> Result<StreamResult, ProviderError> {
        Err(ProviderError::new(
            "Anthropic provider only supports /v1/messages, not /v1/chat/completions",
            ErrorKind::Other,
        ))
    }

    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        super::validate_endpoint_url(&endpoint.url).await?;
        let client = shared_client();

        let url = format!("{}/v1/messages", endpoint.url.trim_end_matches('/'));
        let headers = build_anthropic_headers(endpoint)?;

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
            "Sending request to upstream (anthropic)"
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
                "Upstream HTTP request failed (anthropic)"
            );
            ProviderError::new(format!("Request failed: {}", e), kind)
        })?;

        let status = resp.status();
        tracing::info!(
            endpoint = %endpoint.url,
            ttfb_ms = resp_start.elapsed().as_millis(),
            status = status.as_u16(),
            "Upstream response header received (anthropic)"
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
            "Upstream full response received (anthropic)"
        );

        if !status.is_success() {
            let resp_text = String::from_utf8_lossy(&body_resp);
            let kind = classify_status(status.as_u16());
            return Err(ProviderError::new(
                format!("Upstream returned {}: {}", status.as_u16(), resp_text),
                kind,
            ));
        }

        let resp_body: Value = serde_json::from_slice(&body_resp).map_err(|e| {
            ProviderError::new(format!("Failed to parse response: {}", e), ErrorKind::Parse)
        })?;
        Ok(resp_body)
    }

    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        super::validate_endpoint_url(&endpoint.url).await?;
        let client = shared_client();

        let url = format!("{}/v1/messages", endpoint.url.trim_end_matches('/'));
        let headers = build_anthropic_headers(endpoint)?;

        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(
            endpoint = %endpoint.url,
            body_size = %body_size,
            total_timeout_ms = timeout.as_millis(),
            "Sending stream request to upstream (anthropic)"
        );

        let req = client
            .post(&url)
            .headers(headers)
            .json(&body)
            .timeout(timeout);
        let response = req.send().await.map_err(|e| {
            let kind = classify_reqwest_error(&e);
            tracing::error!(
                endpoint = %endpoint.url,
                error = %e,
                error_kind = ?kind,
                "Anthropic upstream stream request failed"
            );
            ProviderError::new(format!("Stream request failed: {}", e), kind)
        })?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %text, "anthropic upstream stream request failed");
            return Err(ProviderError::new(
                format!("Upstream request failed with status {}", status.as_u16()),
                kind,
            ));
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
                        let _ = tx.send(format!("data: {{\"error\":\"{}\"}}\n\n", e)).await;
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
        super::validate_endpoint_url(&endpoint.url).await?;
        let client = shared_client();

        let url = format!(
            "{}/{}",
            endpoint.url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let headers = build_anthropic_headers(endpoint)?;

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
            "Sending relay request to upstream (anthropic)"
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
                "Upstream relay request failed (anthropic)"
            );
            ProviderError::new(format!("Request failed: {}", e), kind)
        })?;

        let status = resp.status();
        tracing::info!(
            endpoint = %endpoint.url,
            ttfb_ms = resp_start.elapsed().as_millis(),
            status = status.as_u16(),
            "Upstream response header received (anthropic relay)"
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
            "Upstream full response received (anthropic relay)"
        );

        if !status.is_success() {
            let resp_text = String::from_utf8_lossy(&body_resp);
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %resp_text, "anthropic relay request failed");
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
}
