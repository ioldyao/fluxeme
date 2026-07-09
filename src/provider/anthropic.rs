use std::pin::Pin;

use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::Value;

use super::{ProviderAdapter, ProviderError, StreamResult};
use crate::config::types::EndpointConfig;
use crate::provider::shared_client;

pub struct AnthropicAdapter;

#[async_trait::async_trait]
impl ProviderAdapter for AnthropicAdapter {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let client = shared_client();

        let url = format!("{}/messages", endpoint.url.trim_end_matches('/'));
        let anthropic_body = openai_to_anthropic(&body);

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

        let resp = client
            .post(&url)
            .headers(headers)
            .json(&anthropic_body)
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
                status,
                resp_body
            )));
        }

        Ok(anthropic_to_openai(&resp_body))
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let client = shared_client();

        let url = format!("{}/messages", endpoint.url.trim_end_matches('/'));
        let anthropic_body = openai_to_anthropic(&body);

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

        let response = client
            .post(&url)
            .headers(headers)
            .json(&anthropic_body)
            .send()
            .await
            .map_err(|e| ProviderError(format!("Stream request failed: {}", e)))?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ProviderError(format!("Upstream returned {}: {}", status, text)));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(1024);
        let mut byte_stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut current_event = String::new();
            let mut current_data = String::new();

            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));

                        while let Some(newline_pos) = buffer.find('\n') {
                            let line = buffer[..newline_pos].to_string();
                            buffer = buffer[newline_pos + 1..].to_string();

                            if let Some(event_name) = line.strip_prefix("event: ") {
                                current_event = event_name.to_string();
                            } else if let Some(data_str) = line.strip_prefix("data: ") {
                                current_data = data_str.to_string();
                            } else if line.is_empty() && !current_data.is_empty() {
                                let event = std::mem::take(&mut current_event);
                                let data = std::mem::take(&mut current_data);
                                if let Some(openai_sse) = anthropic_sse_to_openai(&event, &data) {
                                    if let Err(e) = tx.send(openai_sse).await {
                                        tracing::warn!("Failed to send SSE event: {:?}", e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if let Err(e) = tx.send(format!("data: {{\"error\":\"{}\"}}\n\n", e)).await {
                            tracing::warn!("Failed to send error SSE: {:?}", e);
                        }
                        break;
                    }
                }
            }

            if let Err(e) = tx.send("data: [DONE]\n\n".to_string()).await {
                tracing::warn!("Failed to send DONE event: {:?}", e);
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Pin::from(Box::new(stream)))
    }
}

fn openai_to_anthropic(body: &Value) -> Value {
    let model = body["model"].as_str().unwrap_or("claude-3-5-sonnet-20241022");
    let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(1024);
    let stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut anthropic = serde_json::Map::new();
    anthropic.insert("model".into(), Value::String(model.to_string()));
    anthropic.insert("max_tokens".into(), Value::Number(max_tokens.into()));
    anthropic.insert("stream".into(), Value::Bool(stream));

    if let Some(messages) = body.get("messages") {
        anthropic.insert("messages".into(), messages.clone());
    }

    if let Some(temp) = body.get("temperature") {
        anthropic.insert("temperature".into(), temp.clone());
    }

    if let Some(top_p) = body.get("top_p") {
        anthropic.insert("top_p".into(), top_p.clone());
    }

    if let Some(stop) = body.get("stop") {
        anthropic.insert("stop_sequences".into(), stop.clone());
    }

    Value::Object(anthropic)
}

fn anthropic_to_openai(body: &Value) -> Value {
    let id = body["id"].as_str().unwrap_or("msg_unknown");
    let model = body["model"].as_str().unwrap_or("unknown");
    let content_text = body["content"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|block| block["text"].as_str())
        .unwrap_or("");

    let finish_reason = match body["stop_reason"].as_str() {
        Some("end_turn") | Some("stop") => "stop",
        Some("max_tokens") | Some("length") => "length",
        Some("tool_use") => "tool_calls",
        _ => "stop",
    };

    let input_tokens = body["usage"]["input_tokens"].as_u64().unwrap_or(0);
    let output_tokens = body["usage"]["output_tokens"].as_u64().unwrap_or(0);

    serde_json::json!({
        "id": id,
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content_text
            },
            "finish_reason": finish_reason
        }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        }
    })
}

fn anthropic_sse_to_openai(event: &str, data: &str) -> Option<String> {
    match event {
        "message_start" => {
            if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                if let Some(msg) = parsed.get("message") {
                    let model = msg["model"].as_str().unwrap_or("claude");
                    let input_tokens = msg
                        .get("usage")
                        .and_then(|u| u.get("input_tokens"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let openai_chunk = serde_json::json!({
                        "id": msg["id"],
                        "object": "chat.completion.chunk",
                        "model": model,
                        "choices": [{
                            "index": 0,
                            "delta": {"role": "assistant", "content": ""},
                            "finish_reason": null
                        }],
                        "usage": {
                            "prompt_tokens": input_tokens,
                            "completion_tokens": 0,
                        }
                    });
                    return Some(format!("data: {}\n\n", openai_chunk));
                }
            }
            None
        }
        "content_block_start" => {
            if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                if let Some(block) = parsed.get("content_block") {
                    if block["type"] == "text" {
                        let text = block["text"].as_str().unwrap_or("");
                        if !text.is_empty() {
                            let openai_chunk = serde_json::json!({
                                "object": "chat.completion.chunk",
                                "choices": [{
                                    "index": 0,
                                    "delta": {"content": text},
                                    "finish_reason": null
                                }]
                            });
                            return Some(format!("data: {}\n\n", openai_chunk));
                        }
                    }
                }
            }
            None
        }
        "content_block_delta" => {
            if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                if let Some(delta) = parsed.get("delta") {
                    if delta["type"] == "text_delta" {
                        let text = delta["text"].as_str().unwrap_or("");
                        if !text.is_empty() {
                            let openai_chunk = serde_json::json!({
                                "object": "chat.completion.chunk",
                                "choices": [{
                                    "index": 0,
                                    "delta": {"content": text},
                                    "finish_reason": null
                                }]
                            });
                            return Some(format!("data: {}\n\n", openai_chunk));
                        }
                    }
                }
            }
            None
        }
        "message_delta" => {
            if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                let stop_reason = parsed["delta"]["stop_reason"].as_str();
                let finish = match stop_reason {
                    Some("end_turn") | Some("stop") => "stop",
                    Some("max_tokens") => "length",
                    _ => "stop",
                };

                let output_tokens = parsed["usage"]["output_tokens"].as_u64().unwrap_or(0);

                let openai_chunk = serde_json::json!({
                    "object": "chat.completion.chunk",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": finish
                    }],
                    "usage": {
                        "prompt_tokens": 0,
                        "completion_tokens": output_tokens
                    }
                });
                return Some(format!("data: {}\n\n", openai_chunk));
            }
            None
        }
        "message_stop" => {
            return None;
        }
        "ping" => None,
        _ => None,
    }
}
