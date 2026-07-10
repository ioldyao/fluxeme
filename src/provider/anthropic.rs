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
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let client = shared_client();

        let url = format!("{}/messages", endpoint.url.trim_end_matches('/'));
        let anthropic_body = openai_to_anthropic(&body);

        let headers = build_anthropic_headers(endpoint)?;

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

        let headers = build_anthropic_headers(endpoint)?;

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

                                // Handle tool_use events first
                                if let Some(sse) = handle_tool_event(&event, &data) {
                                    if tx.send(sse).await.is_err() {
                                        break;
                                    }
                                    continue;
                                }

                                if let Some(openai_sse) = anthropic_sse_to_openai(&event, &data) {
                                    if tx.send(openai_sse).await.is_err() {
                                        break;
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

            let _ = tx.send("data: [DONE]\n\n".to_string()).await;
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Pin::from(Box::new(stream)))
    }

    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let client = shared_client();
        let url = format!("{}/messages", endpoint.url.trim_end_matches('/'));
        let headers = build_anthropic_headers(endpoint)?;

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
                status,
                resp_body
            )));
        }

        Ok(resp_body)
    }

    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let client = shared_client();
        let url = format!("{}/messages", endpoint.url.trim_end_matches('/'));
        let headers = build_anthropic_headers(endpoint)?;

        let response = client
            .post(&url)
            .headers(headers)
            .json(&body)
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
}

fn openai_to_anthropic(body: &Value) -> Value {
    let model = body["model"].as_str().unwrap_or("claude-3-5-sonnet-20241022");
    let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(1024);
    let stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut anthropic = serde_json::Map::new();
    anthropic.insert("model".into(), Value::String(model.to_string()));
    anthropic.insert("max_tokens".into(), Value::Number(max_tokens.into()));
    anthropic.insert("stream".into(), Value::Bool(stream));

    // Extract system prompt from messages (role: system → top-level system field)
    // and keep the rest as messages
    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        let mut msgs = Vec::new();
        let mut system_parts: Vec<String> = Vec::new();

        for msg in messages {
            if msg["role"].as_str() == Some("system") {
                if let Some(text) = msg["content"].as_str() {
                    system_parts.push(text.to_string());
                }
            } else {
                msgs.push(msg.clone());
            }
        }

        if !system_parts.is_empty() {
            anthropic.insert("system".into(), Value::String(system_parts.join("\n")));
        }
        anthropic.insert("messages".into(), Value::Array(msgs));
    }

    // Pass through optional scalar fields
    for key in ["temperature", "top_p", "top_k"] {
        if let Some(v) = body.get(key) {
            anthropic.insert(key.to_string(), v.clone());
        }
    }

    // stop → stop_sequences (Anthropic expects array only, OpenAI accepts string or array)
    if let Some(stop) = body.get("stop") {
        match stop {
            Value::String(s) => {
                anthropic.insert("stop_sequences".into(), Value::Array(vec![Value::String(s.clone())]));
            }
            Value::Array(arr) => {
                anthropic.insert("stop_sequences".into(), Value::Array(arr.clone()));
            }
            _ => {}
        }
    }

    // Tools: OpenAI format → Anthropic format
    // OpenAI:  {"type":"function","function":{"name":"x","description":"...","parameters":{...}}}
    // Anthropic: {"name":"x","description":"...","input_schema":{...}}
    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        let converted: Vec<Value> = tools.iter().filter_map(|tool| {
            let func = tool.get("function")?;
            let name = func["name"].as_str()?;
            let mut t = serde_json::Map::new();
            t.insert("name".into(), Value::String(name.to_string()));
            if let Some(desc) = func["description"].as_str() {
                if !desc.is_empty() {
                    t.insert("description".into(), Value::String(desc.to_string()));
                }
            }
            if let Some(params) = func.get("parameters") {
                t.insert("input_schema".into(), params.clone());
            }
            Some(Value::Object(t))
        }).collect();
        if !converted.is_empty() {
            anthropic.insert("tools".into(), Value::Array(converted));
        }
    }

    // tool_choice: OpenAI format → Anthropic format
    // OpenAI:  {"type":"function","function":{"name":"x"}}  or  "auto" / "none"
    // Anthropic: {"type":"tool","name":"x"}  or  "auto" / {"type":"none"} / "any"
    if let Some(tc) = body.get("tool_choice") {
        match tc {
            Value::String(s) if s == "auto" => {
                anthropic.insert("tool_choice".into(), Value::String("auto".to_string()));
            }
            Value::String(s) if s == "none" => {
                anthropic.insert(
                    "tool_choice".into(),
                    serde_json::json!({"type": "none"}),
                );
            }
            Value::Object(obj) => {
                if obj.get("type").and_then(|v| v.as_str()) == Some("function") {
                    if let Some(name) = obj.get("function").and_then(|f| f["name"].as_str()) {
                        let mut ac = serde_json::Map::new();
                        ac.insert("type".into(), Value::String("tool".into()));
                        ac.insert("name".into(), Value::String(name.to_string()));
                        anthropic.insert("tool_choice".into(), Value::Object(ac));
                    }
                }
            }
            _ => {}
        }
    }

    // Metadata (e.g. user_id)
    if let Some(meta) = body.get("metadata") {
        anthropic.insert("metadata".into(), meta.clone());
    }

    Value::Object(anthropic)
}

fn anthropic_to_openai(body: &Value) -> Value {
    let id = body["id"].as_str().unwrap_or("msg_unknown");
    let model = body["model"].as_str().unwrap_or("unknown");

    // Parse content blocks — extract text and tool_calls
    let mut content_text: Option<String> = None;
    let mut tool_calls = Vec::new();

    if let Some(content) = body["content"].as_array() {
        for block in content {
            match block["type"].as_str() {
                Some("text") => {
                    content_text = block["text"].as_str().map(|s| s.to_string());
                }
                Some("tool_use") => {
                    let id = block["id"].as_str().unwrap_or("");
                    let name = block["name"].as_str().unwrap_or("");
                    let arguments = block["input"].to_string();
                    tool_calls.push(serde_json::json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments
                        }
                    }));
                }
                _ => {}
            }
        }
    }

    let finish_reason = match body["stop_reason"].as_str() {
        Some("end_turn") => "stop",
        Some("max_tokens") => "length",
        Some("tool_use") => "tool_calls",
        Some("stop_sequence") => "stop",
        _ => "stop",
    };

    let input_tokens = body["usage"]["input_tokens"].as_u64().unwrap_or(0);
    let output_tokens = body["usage"]["output_tokens"].as_u64().unwrap_or(0);

    let mut message = serde_json::json!({
        "role": "assistant",
        "content": content_text
    });
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
        if content_text.is_none() {
            message["content"] = Value::Null;
        }
    }

    serde_json::json!({
        "id": id,
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": finish_reason
        }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens + output_tokens
        }
    })
}

/// Handle Anthropic tool_use SSE events and convert to OpenAI format.
fn handle_tool_event(event: &str, data: &str) -> Option<String> {
    if event == "content_block_start" {
        let parsed: Value = serde_json::from_str(data).ok()?;
        if parsed["content_block"]["type"] == "tool_use" {
            let id = parsed["content_block"]["id"].as_str().unwrap_or("");
            let name = parsed["content_block"]["name"].as_str().unwrap_or("");
            return Some(format!("data: {}\n\n", serde_json::json!({
                "object": "chat.completion.chunk",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "index": 0,
                            "id": id,
                            "type": "function",
                            "function": { "name": name, "arguments": "" }
                        }]
                    },
                    "finish_reason": null
                }]
            })));
        }
        return None;
    }

    if event == "content_block_delta" {
        let parsed: Value = serde_json::from_str(data).ok()?;
        if parsed["delta"]["type"] == "input_json_delta" {
            let partial = parsed["delta"]["partial_json"].as_str()?;
            if partial.is_empty() {
                return None;
            }
            return Some(format!("data: {}\n\n", serde_json::json!({
                "object": "chat.completion.chunk",
                "choices": [{
                    "index": 0,
                    "delta": {
                        "tool_calls": [{
                            "index": parsed["index"].as_u64().unwrap_or(0) as usize,
                            "function": { "arguments": partial }
                        }]
                    },
                    "finish_reason": null
                }]
            })));
        }
    }

    None
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
                    Some("end_turn") | Some("stop") | Some("stop_sequence") => "stop",
                    Some("max_tokens") => "length",
                    Some("tool_use") => "tool_calls",
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
