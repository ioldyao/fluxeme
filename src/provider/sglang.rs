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

        // Default max_new_tokens if client doesn't specify
        let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64());
        sampling.insert("max_new_tokens".into(), Value::Number(max_tokens.unwrap_or(4096).into()));
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

    /// Convert OpenAI messages to SGLang text format.
    fn messages_to_text(messages: Option<&Value>) -> String {
        let msgs = match messages.and_then(|m| m.as_array()) {
            Some(a) => a,
            None => return String::new(),
        };

        let mut parts: Vec<String> = Vec::with_capacity(msgs.len());
        for msg in msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = Self::extract_content(msg.get("content"));

            match role {
                "system" => parts.push(format!("<|system|>\n{}", content)),
                "user" => parts.push(format!("<|user|>\n{}", content)),
                "assistant" => parts.push(format!("<|assistant|>\n{}", content)),
                _ => parts.push(content),
            }
        }
        parts.push("<|assistant|>\n".to_string());
        parts.join("\n")
    }

    /// Extract text from a content field (handles both string and array-of-blocks).
    fn extract_content(content: Option<&Value>) -> String {
        match content {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter_map(|b| {
                    if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                        b.get("text").and_then(|v| v.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        }
    }

    /// Convert Anthropic-format request body to SGLang native `/generate` format.
    fn to_native_anthropic(body: &Value) -> Value {
        let system = body.get("system").and_then(|v| v.as_str()).unwrap_or("");
        let text = Self::anthropic_messages_to_text(system, body.get("messages"));

        let mut sampling = serde_json::Map::new();

        // Default max_new_tokens if client doesn't specify
        let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64());
        sampling.insert("max_new_tokens".into(), Value::Number(max_tokens.unwrap_or(4096).into()));
        if let Some(v) = body.get("temperature").and_then(|v| v.as_f64()) {
            sampling.insert("temperature".into(), Value::from(v));
        }
        if let Some(v) = body.get("top_p").and_then(|v| v.as_f64()) {
            sampling.insert("top_p".into(), Value::from(v));
        }
        if let Some(v) = body.get("top_k").and_then(|v| v.as_u64()) {
            sampling.insert("top_k".into(), Value::Number(v.into()));
        }
        if let Some(v) = body.get("stop_sequences") {
            sampling.insert("stop".into(), v.clone());
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

    /// Convert Anthropic messages + system prompt to SGLang text format.
    fn anthropic_messages_to_text(system: &str, messages: Option<&Value>) -> String {
        let mut parts: Vec<String> = Vec::new();

        if !system.is_empty() {
            parts.push(format!("<|system|>\n{}", system));
        }

        let msgs = match messages.and_then(|m| m.as_array()) {
            Some(a) => a,
            None => {
                parts.push("<|assistant|>\n".to_string());
                return parts.join("\n");
            }
        };

        for msg in msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = Self::extract_content(msg.get("content"));
            match role {
                "user" => parts.push(format!("<|user|>\n{}", content)),
                "assistant" => parts.push(format!("<|assistant|>\n{}", content)),
                _ => parts.push(content),
            }
        }
        parts.push("<|assistant|>\n".to_string());
        parts.join("\n")
    }

    /// Extract token counts and finish_reason from SGLang `meta_info`.
    fn extract_meta(body: &Value) -> (u64, u64, u64, String) {
        let prompt = body["meta_info"]["prompt_tokens"].as_u64().unwrap_or(0);
        let completion = body["meta_info"]["completion_tokens"].as_u64().unwrap_or(0);
        let cached = body["meta_info"]["cached_tokens"].as_u64().unwrap_or(0);
        let finish = body["meta_info"]["finish_reason"]["type"]
            .as_str()
            .unwrap_or("stop")
            .to_string();
        (prompt, completion, cached, finish)
    }

    /// Map SGLang finish_reason to OpenAI format.
    fn map_finish_reason(sglang_reason: &str) -> &str {
        match sglang_reason {
            "length" | "eos_token" => "length",
            _ => "stop",
        }
    }

    /// Map SGLang finish_reason to Anthropic stop_reason.
    fn map_stop_reason(sglang_reason: &str) -> &str {
        match sglang_reason {
            "length" | "eos_token" => "max_tokens",
            _ => "end_turn",
        }
    }

    /// Convert SGLang `/generate` response to OpenAI-compatible format.
    fn to_openai(body: &Value, model: &str) -> Value {
        let text = body
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let (prompt_tokens, completion_tokens, cached_tokens, reason) = Self::extract_meta(body);
        let finish_reason = Self::map_finish_reason(&reason);

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
                "finish_reason": finish_reason,
            }],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": prompt_tokens + completion_tokens,
                "prompt_tokens_details": {
                    "cached_tokens": cached_tokens,
                },
            }
        })
    }

    /// Convert SGLang `/generate` response to Anthropic-compatible format.
    fn to_anthropic(body: &Value, model: &str) -> Value {
        let text = body
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let (prompt_tokens, completion_tokens, cached_tokens, reason) = Self::extract_meta(body);
        let stop_reason = Self::map_stop_reason(&reason);

        serde_json::json!({
            "id": format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', "")),
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": text}],
            "model": model,
            "stop_reason": stop_reason,
            "stop_sequence": null,
            "usage": {
                "input_tokens": prompt_tokens,
                "output_tokens": completion_tokens,
                "cache_read_input_tokens": cached_tokens,
            }
        })
    }

    /// Produce an Anthropic-format `content_block_delta` SSE event string.
    fn anthropic_delta_event(text: &str) -> String {
        let payload = serde_json::json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {
                "type": "text_delta",
                "text": text,
            }
        });
        format!("event: content_block_delta\ndata: {}\n\n", serde_json::to_string(&payload).unwrap_or_default())
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

                                    let is_final = val.get("meta_info").is_some();
                                    let finish_reason = if is_final {
                                        let reason = val["meta_info"]["finish_reason"]["type"]
                                            .as_str()
                                            .unwrap_or("stop");
                                        Some(Self::map_finish_reason(reason))
                                    } else {
                                        None
                                    };

                                    let mut chunk = serde_json::json!({
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

                                    // Include real usage in the final chunk
                                    if is_final {
                                        let (p, c, cached, _) = Self::extract_meta(&val);
                                        chunk["usage"] = serde_json::json!({
                                            "prompt_tokens": p,
                                            "completion_tokens": c,
                                            "total_tokens": p + c,
                                            "prompt_tokens_details": {
                                                "cached_tokens": cached,
                                            },
                                        });
                                    }
                                    let _ = tx
                                        .send(format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap_or_default()))
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
                            let is_final = val.get("meta_info").is_some();
                            let finish_reason = if is_final {
                                let reason = val["meta_info"]["finish_reason"]["type"]
                                    .as_str()
                                    .unwrap_or("stop");
                                Some(Self::map_finish_reason(reason))
                            } else {
                                Some("stop")
                            };
                            let mut chunk = serde_json::json!({
                                "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
                                "object": "chat.completion.chunk",
                                "model": model,
                                "choices": [{
                                    "index": 0,
                                    "delta": {"content": text},
                                    "finish_reason": finish_reason,
                                }]
                            });
                            if is_final {
                                let (p, c, cached, _) = Self::extract_meta(&val);
                                chunk["usage"] = serde_json::json!({
                                    "prompt_tokens": p,
                                    "completion_tokens": c,
                                    "total_tokens": p + c,
                                    "prompt_tokens_details": {"cached_tokens": cached},
                                });
                            }
                            let _ = tx
                                .send(format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap_or_default()))
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

    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();
        let native = Self::to_native_anthropic(&body);
        let resp = self.send_generate(endpoint, native).await?;
        Ok(Self::to_anthropic(&resp, &model))
    }

    async fn messages_stream(
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

        let native = Self::to_native_anthropic(&body);
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
            "Sending anthropic-format stream request to upstream (sglang)"
        );

        let req = client.post(&url).headers(headers).json(&stream_body).timeout(timeout);
        let response = req.send().await
            .map_err(|e| {
                let kind = classify_reqwest_error(&e);
                ProviderError::new(format!("Stream request failed: {}", e), kind)
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %body, "sglang anthropic-format stream request failed");
            return Err(ProviderError::new(
                format!("Upstream request failed with status {}", status.as_u16()),
                kind,
            ));
        }

        let (tx, rx) = tokio::sync::mpsc::channel::<String>(1024);
        let mut byte_stream = response.bytes_stream();
        let msg_id = format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));

        tokio::spawn(async move {
            let mut buffer = String::new();
            let mut has_sent_start = false;

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
                                    // Emit remaining anthropic events before done
                                    if has_sent_start {
                                        let _ = tx.send("event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string()).await;
                                        let delta_payload = serde_json::json!({
                                            "type": "message_delta",
                                            "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                                            "usage": {"output_tokens": 0}
                                        });
                                        let _ = tx.send(format!("event: message_delta\ndata: {}\n\n", serde_json::to_string(&delta_payload).unwrap_or_default())).await;
                                        let _ = tx.send("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string()).await;
                                    }
                                    let _ = tx.send("data: [DONE]\n\n".to_string()).await;
                                    return;
                                }
                                if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                                    let text = val
                                        .get("text")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");

                                    if !has_sent_start {
                                        let start_msg = serde_json::json!({
                                            "type": "message_start",
                                            "message": {
                                                "id": msg_id,
                                                "type": "message",
                                                "role": "assistant",
                                                "content": [],
                                                "model": model,
                                                "stop_reason": null,
                                                "stop_sequence": null,
                                                "usage": {"input_tokens": 0, "output_tokens": 0}
                                            }
                                        });
                                        let _ = tx.send(format!("event: message_start\ndata: {}\n\n", serde_json::to_string(&start_msg).unwrap_or_default())).await;
                                        let _ = tx.send("event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string()).await;
                                        has_sent_start = true;
                                    }

                                    // Check for final chunk with meta_info
                                    let is_final = val.get("meta_info").is_some();

                                    if !text.is_empty() {
                                        let _ = tx.send(Self::anthropic_delta_event(text)).await;
                                    }

                                    if is_final {
                                        let (_, c_tokens, _, reason) = Self::extract_meta(&val);
                                        let stop_reason = Self::map_stop_reason(&reason);
                                        let _ = tx.send("event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string()).await;
                                        let delta_payload = serde_json::json!({
                                            "type": "message_delta",
                                            "delta": {"stop_reason": stop_reason, "stop_sequence": null},
                                            "usage": {"output_tokens": c_tokens}
                                        });
                                        let _ = tx.send(format!("event: message_delta\ndata: {}\n\n", serde_json::to_string(&delta_payload).unwrap_or_default())).await;
                                        let _ = tx.send("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string()).await;
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

            // Flush remaining buffer
            if !buffer.is_empty() {
                if let Some(json_str) = buffer.strip_prefix("data: ") {
                    if let Ok(val) = serde_json::from_str::<Value>(json_str.trim()) {
                        if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
                            if !text.is_empty() {
                                if !has_sent_start {
                                    let start_msg = serde_json::json!({
                                        "type": "message_start",
                                        "message": {
                                            "id": &msg_id,
                                            "type": "message",
                                            "role": "assistant",
                                            "content": [],
                                            "model": &model,
                                            "stop_reason": null,
                                            "stop_sequence": null,
                                            "usage": {"input_tokens": 0, "output_tokens": 0}
                                        }
                                    });
                                    let _ = tx.send(format!("event: message_start\ndata: {}\n\n", serde_json::to_string(&start_msg).unwrap_or_default())).await;
                                    let _ = tx.send("event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string()).await;
                                    has_sent_start = true;
                                }
                                let _ = tx.send(Self::anthropic_delta_event(text)).await;
                            }
                        }
                    }
                }
            }

            if has_sent_start {
                let _ = tx.send("event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string()).await;
                let delta_payload = serde_json::json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                    "usage": {"output_tokens": 0}
                });
                let _ = tx.send(format!("event: message_delta\ndata: {}\n\n", serde_json::to_string(&delta_payload).unwrap_or_default())).await;
                let _ = tx.send("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string()).await;
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
