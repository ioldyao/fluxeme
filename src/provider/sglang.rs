use std::pin::Pin;
use std::time::Instant;

use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, AUTHORIZATION};
use serde_json::Value;

use super::{
    classify_reqwest_error, classify_status, default_config, request_timeout, ErrorKind,
    ProviderAdapter, ProviderError, RequestKind, StreamResult,
};
use crate::config::types::EndpointConfig;
use crate::provider::shared_client;

pub struct SglangAdapter;

impl SglangAdapter {
    /// Convert OpenAI-format chat body to SGLang native `/generate` format.
    fn to_native(body: &Value) -> Value {
        let text = Self::messages_to_text(body.get("messages"));
        let mut sampling = serde_json::Map::new();

        if let Some(v) = body.get("max_tokens").and_then(|v| v.as_u64()) {
            sampling.insert("max_new_tokens".into(), Value::Number(v.into()));
        }
        if let Some(v) = body.get("temperature").and_then(|v| v.as_f64()) {
            sampling.insert("temperature".into(), Value::from(v));
        }
        if let Some(v) = body.get("top_p").and_then(|v| v.as_f64()) {
            sampling.insert("top_p".into(), Value::from(v));
        }
        if let Some(v) = body.get("top_k").and_then(|v| v.as_u64()) {
            sampling.insert("top_k".into(), Value::Number(v.into()));
        }
        if let Some(v) = body.get("stop") {
            sampling.insert("stop".into(), v.clone());
        }
        if let Some(v) = body.get("frequency_penalty").and_then(|v| v.as_f64()) {
            sampling.insert("frequency_penalty".into(), Value::from(v));
        }
        if let Some(v) = body.get("presence_penalty").and_then(|v| v.as_f64()) {
            sampling.insert("presence_penalty".into(), Value::from(v));
        }
        if let Some(v) = body.get("repetition_penalty").and_then(|v| v.as_f64()) {
            sampling.insert("repetition_penalty".into(), Value::from(v));
        }

        let mut req = serde_json::Map::new();
        req.insert("text".into(), Value::String(text));
        req.insert("sampling_params".into(), Value::Object(sampling));

        let is_stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
        if is_stream {
            req.insert("stream".into(), Value::Bool(true));
        }

        Value::Object(req)
    }

    /// Simple messages → plain text conversion using chatml format.
    fn messages_to_text(messages: Option<&Value>) -> String {
        let msgs = match messages.and_then(|m| m.as_array()) {
            Some(a) => a,
            None => return String::new(),
        };

        let mut parts: Vec<String> = Vec::with_capacity(msgs.len());
        for msg in msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = msg
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match role {
                "system" => parts.push(format!("<|im_start|>system\n{}\n<|im_end|>", content)),
                "user" => parts.push(format!("<|im_start|>user\n{}\n<|im_end|>", content)),
                "assistant" => parts.push(format!("<|im_start|>assistant\n{}\n<|im_end|>", content)),
                _ => parts.push(content.to_string()),
            }
        }
        parts.push("<|im_start|>assistant\n".to_string());
        parts.join("\n")
    }

    /// Convert SGLang `/generate` response to OpenAI-compatible format.
    fn to_openai(body: &Value, model: &str) -> Value {
        let text = body
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let completion_tokens = text.len() / 4;

        serde_json::json!({
            "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion",
            "model": model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": text,
                },
                "finish_reason": "stop",
            }],
            "usage": {
                "prompt_tokens": 0,
                "completion_tokens": completion_tokens.max(1) as u64,
                "total_tokens": completion_tokens.max(1) as u64,
            }
        })
    }

    async fn send_generate(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        super::validate_endpoint_url(&endpoint.url).await?;
        let client = shared_client();

        let base = endpoint.url.trim_end_matches('/');
        let url = format!("{}/generate", base);

        let mut headers = HeaderMap::new();
        if !endpoint.api_key.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key))
                    .map_err(|e| ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other))?,
            );
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

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
            "Sending request to upstream (sglang)"
        );

        let resp_start = Instant::now();
        let req = client.post(&url).headers(headers).json(&body).timeout(timeout);
        let resp = req.send().await.map_err(|e| {
            let kind = classify_reqwest_error(&e);
            tracing::error!(
                endpoint = %endpoint.url,
                error = %e,
                error_kind = ?kind,
                elapsed_ms = resp_start.elapsed().as_millis(),
                "SGLang upstream request failed"
            );
            ProviderError::new(format!("Request failed: {}", e), kind)
        })?;

        let status = resp.status();
        tracing::info!(
            endpoint = %endpoint.url,
            ttfb_ms = resp_start.elapsed().as_millis(),
            status = status.as_u16(),
            "Upstream response header received (sglang)"
        );

        let body_resp = resp.bytes().await.map_err(|e| {
            ProviderError::new(format!("Failed to read response body: {}", e), ErrorKind::Parse)
        })?;
        tracing::info!(
            endpoint = %endpoint.url,
            body_size = body_resp.len(),
            total_ms = resp_start.elapsed().as_millis(),
            "Upstream full response received (sglang)"
        );

        if !status.is_success() {
            let resp_text = String::from_utf8_lossy(&body_resp);
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %resp_text, "sglang upstream request failed");
            return Err(ProviderError::new(
                format!("Upstream request failed with status {}", status.as_u16()),
                kind,
            ));
        }

        let resp_body: Value = serde_json::from_slice(&body_resp)
            .map_err(|e| ProviderError::new(format!("Failed to parse response: {}", e), ErrorKind::Parse))?;
        Ok(resp_body)
    }
}

#[async_trait::async_trait]
impl ProviderAdapter for SglangAdapter {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();
        let native = Self::to_native(&body);
        let resp = self.send_generate(endpoint, native).await?;
        Ok(Self::to_openai(&resp, &model))
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();
        super::validate_endpoint_url(&endpoint.url).await?;
        let client = shared_client();

        let base = endpoint.url.trim_end_matches('/');
        let url = format!("{}/generate", base);

        let mut headers = HeaderMap::new();
        if !endpoint.api_key.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key))
                    .map_err(|e| ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other))?,
            );
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let native = Self::to_native(&body);
        let stream_body = {
            let mut map = if let Value::Object(m) = native { m } else { serde_json::Map::new() };
            map.insert("stream".into(), Value::Bool(true));
            Value::Object(map)
        };

        let body_size = serde_json::to_string(&stream_body).map(|s| s.len()).unwrap_or(0);
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(
            endpoint = %endpoint.url,
            body_size = %body_size,
            total_timeout_ms = timeout.as_millis(),
            "Sending stream request to upstream (sglang)"
        );

        let req = client.post(&url).headers(headers).json(&stream_body).timeout(timeout);
        let response = req.send().await
            .map_err(|e| {
                let kind = classify_reqwest_error(&e);
                ProviderError::new(format!("Stream request failed: {}", e), kind)
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_default();
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %body, "sglang upstream stream request failed");
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

                            if let Some(json_str) = event.strip_prefix("data: ") {
                                let trimmed = json_str.trim();
                                if trimmed == "[DONE]" {
                                    let _ = tx.send("data: [DONE]\n\n".to_string()).await;
                                    return;
                                }
                                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                                    let text = val
                                        .get("text")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    // Check for final chunk with meta info
                                    let finish_reason = if val.get("meta").is_some() {
                                        Some("stop")
                                    } else {
                                        None
                                    };

                                    let openai_chunk = serde_json::json!({
                                        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                                        "object": "chat.completion.chunk",
                                        "model": model,
                                        "choices": [{
                                            "index": 0,
                                            "delta": {
                                                "content": text,
                                            },
                                            "finish_reason": finish_reason,
                                        }]
                                    });
                                    let _ = tx
                                        .send(format!("data: {}\n\n", serde_json::to_string(&openai_chunk).unwrap_or_default()))
                                        .await;

                                    if finish_reason.is_some() {
                                        let _ = tx.send("data: [DONE]\n\n".to_string()).await;
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(format!("data: {{\"error\":\"{}\"}}\n\n", e)).await;
                        break;
                    }
                }
            }

            // Remaining buffer without trailing \n\n
            if !buffer.is_empty() {
                if let Some(json_str) = buffer.strip_prefix("data: ") {
                    if let Ok(val) = serde_json::from_str::<Value>(json_str.trim()) {
                        if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
                            let openai_chunk = serde_json::json!({
                                "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                                "object": "chat.completion.chunk",
                                "model": model,
                                "choices": [{
                                    "index": 0,
                                    "delta": {"content": text},
                                    "finish_reason": "stop",
                                }]
                            });
                            let _ = tx
                                .send(format!("data: {}\n\n", serde_json::to_string(&openai_chunk).unwrap_or_default()))
                                .await;
                        }
                    }
                }
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

        let mut headers = HeaderMap::new();
        if !endpoint.api_key.is_empty() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key))
                    .map_err(|e| ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other))?,
            );
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let http_method = match body {
            Value::Null => reqwest::Method::GET,
            _ => reqwest::Method::POST,
        };

        let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
        let timeout = request_timeout(
            &RequestKind::Unary { body_size },
            endpoint,
            &default_config(),
        );
        tracing::info!(
            endpoint = %endpoint.url,
            path = %path,
            body_size = %body_size,
            timeout_ms = timeout.as_millis(),
            "Sending relay request to upstream (sglang)"
        );

        let resp_start = Instant::now();
        let mut req_builder = client.request(http_method, &url).headers(headers).timeout(timeout);
        if !body.is_null() {
            req_builder = req_builder.json(&body);
        }
        let resp = req_builder.send().await.map_err(|e| {
            let kind = classify_reqwest_error(&e);
            tracing::error!(
                endpoint = %endpoint.url,
                error = %e,
                error_kind = ?kind,
                elapsed_ms = resp_start.elapsed().as_millis(),
                "SGLang relay request failed"
            );
            ProviderError::new(format!("Request failed: {}", e), kind)
        })?;

        let status = resp.status();
        tracing::info!(
            endpoint = %endpoint.url,
            ttfb_ms = resp_start.elapsed().as_millis(),
            status = status.as_u16(),
            "Upstream response header received (sglang relay)"
        );
        let body_resp = resp.bytes().await.map_err(|e| {
            ProviderError::new(format!("Failed to read response body: {}", e), ErrorKind::Parse)
        })?;
        tracing::info!(
            endpoint = %endpoint.url,
            body_size = body_resp.len(),
            total_ms = resp_start.elapsed().as_millis(),
            "Upstream full response received (sglang relay)"
        );

        if !status.is_success() {
            let resp_text = String::from_utf8_lossy(&body_resp);
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %resp_text, "sglang relay request failed");
            return Err(ProviderError::new(
                format!("Upstream request failed with status {}", status.as_u16()),
                kind,
            ));
        }

        if body_resp.is_empty() {
            return Ok(Value::Object(serde_json::Map::new()));
        }
        let resp_body: Value = serde_json::from_slice(&body_resp)
            .map_err(|e| ProviderError::new(format!("Failed to parse response: {}", e), ErrorKind::Parse))?;
        Ok(resp_body)
    }
}
