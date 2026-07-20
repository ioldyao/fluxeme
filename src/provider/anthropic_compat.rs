//! Converts between Anthropic /v1/messages and OpenAI /v1/chat/completions formats.
//!
//! Used when a channel with `anthropic_compat=true` (OpenAI provider) receives
//! an Anthropic-format request.  The request is converted to OpenAI format for
//! upstream forwarding, and the response is converted back to Anthropic format.
//!
//! The [`AnthropicCompatAdapter`] wraps any [`ProviderAdapter`] and transparently
//! intercepts `messages()` / `messages_stream()` calls, converting the body and
//! response so that an OpenAI channel can serve Anthropic-format requests.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::Stream;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use super::{ProviderAdapter, ProviderError, StreamResult};
use crate::config::types::EndpointConfig;

// ── Adapter wrapper ─────────────────────────────────────────────────
// Transparently intercepts messages() / messages_stream() so that an
// OpenAI channel can serve Anthropic-format requests via the compat flag.

/// Wraps a [`ProviderAdapter`] (typically the OpenAI adapter) and
/// transparently converts Anthropic `/v1/messages` calls to OpenAI
/// `/v1/chat/completions` calls, and vice-versa for responses.
pub struct AnthropicCompatAdapter {
    inner: Arc<dyn ProviderAdapter>,
}

impl AnthropicCompatAdapter {
    pub fn new(inner: Arc<dyn ProviderAdapter>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl ProviderAdapter for AnthropicCompatAdapter {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        self.inner.chat_complete(endpoint, body).await
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        self.inner.chat_complete_stream(endpoint, body).await
    }

    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let openai_body = anthropic_to_openai(&body);
        let resp = self.inner.chat_complete(endpoint, openai_body).await?;
        tracing::info!(
            model = %model,
            openai_usage = %resp.get("usage").map(|u| u.to_string()).unwrap_or_default(),
            "anthropic_compat: raw OpenAI response usage"
        );
        let converted = openai_to_anthropic_response(&resp, &model);
        tracing::info!(
            model = %model,
            input_tokens = converted["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens = converted["usage"]["output_tokens"].as_u64().unwrap_or(0),
            "anthropic_compat: converted Anthropic response usage"
        );
        Ok(converted)
    }

    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let openai_body = anthropic_to_openai(&body);
        let stream = self.inner.chat_complete_stream(endpoint, openai_body).await?;
        Ok(wrap_openai_sse_for_anthropic(stream, model))
    }
}

// ── Request conversion ──────────────────────────────────────────────

/// Convert an Anthropic `/v1/messages` request body into an OpenAI
/// `/v1/chat/completions` request body.
pub fn anthropic_to_openai(body: &Value) -> Value {
    let mut messages: Vec<Value> = Vec::new();

    // system (top-level string or content-block array) → system message
    if let Some(s) = body.get("system").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            messages.push(json!({"role": "system", "content": s}));
        }
    } else if let Some(arr) = body.get("system").and_then(|v| v.as_array()) {
        let text = blocks_to_text(arr);
        if !text.is_empty() {
            messages.push(json!({"role": "system", "content": text}));
        }
    }

    // messages
    if let Some(anthropic_msgs) = body.get("messages").and_then(|v| v.as_array()) {
        for msg in anthropic_msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = convert_content(msg.get("content"));
            messages.push(json!({"role": role, "content": content}));
        }
    }

    let mut openai = json!({
        "model": body.get("model").cloned().unwrap_or(Value::Null),
        "messages": messages,
    });

    // shared parameters
    for key in &[
        "max_tokens", "temperature", "top_p", "stop", "stream",
        "frequency_penalty", "presence_penalty",
    ] {
        if let Some(v) = body.get(key) {
            openai[key] = v.clone();
        }
    }

    // max_tokens → max_completion_tokens (newer OpenAI API)
    if body.get("max_tokens").is_some() {
        openai["max_completion_tokens"] = body["max_tokens"].clone();
    }

    openai
}

// ── Non-streaming response conversion ───────────────────────────────

/// Convert an OpenAI `/v1/chat/completions` non-streaming response into
/// an Anthropic `/v1/messages` response.
pub fn openai_to_anthropic_response(openai_resp: &Value, model: &str) -> Value {
    let msg_id = format!("msg_{}", Uuid::new_v4().simple());

    let content = openai_resp
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let usage = openai_resp.get("usage");
    // Try standard OpenAI field names first, then fall back to
    // alternative naming used by some OpenAI-compatible endpoints.
    let input_tokens = usage
        .and_then(|u| {
            u.get("prompt_tokens")
                .or_else(|| u.get("input_tokens"))
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|u| {
            u.get("completion_tokens")
                .or_else(|| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
        })
        .unwrap_or(0);
    let cache_read = usage
        .and_then(|u| u.get("prompt_tokens_details"))
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let finish = openai_resp
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str());
    let stop_reason = match finish {
        Some("stop") => "end_turn",
        Some("length") => "max_tokens",
        Some("tool_calls") => "tool_use",
        _ => "end_turn",
    };

    let mut resp = json!({
        "id": msg_id,
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": content}],
        "model": model,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        },
    });

    if cache_read > 0 {
        resp["usage"]["cache_read_input_tokens"] = json!(cache_read);
    }

    resp
}

// ── Streaming conversion ────────────────────────────────────────────

/// Wraps an OpenAI SSE string stream so that every chunk is converted to
/// Anthropic SSE format on the fly.
pub fn wrap_openai_sse_for_anthropic(
    inner: Pin<Box<dyn Stream<Item = String> + Send>>,
    model: String,
) -> StreamResult {
    let message_id = format!("msg_{}", Uuid::new_v4().simple());
    let (tx, rx) = mpsc::channel::<String>(64);

    tokio::spawn(async move {
        let mut buf = String::new();
        let mut state = ConvertState::new(message_id, model);
        tokio::pin!(inner);

        while let Some(chunk) = futures::StreamExt::next(&mut inner).await {
            buf.push_str(&chunk);
            while let Some(pos) = buf.find("\n\n") {
                let raw = buf[..pos].to_string();
                buf = buf[pos + 2..].to_string();
                for line in raw.lines() {
                    let line = line.trim();
                    if line.is_empty() || line == "data: [DONE]" {
                        continue;
                    }
                    let data = line.strip_prefix("data: ").unwrap_or(line);
                    if let Ok(val) = serde_json::from_str::<Value>(data) {
                        state.ingest(&val, &tx).await;
                    }
                }
            }
        }

        // drain remaining partial data
        if !buf.is_empty() && buf != "data: [DONE]" {
            for line in buf.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let data = line.strip_prefix("data: ").unwrap_or(line);
                if let Ok(val) = serde_json::from_str::<Value>(data) {
                    state.ingest(&val, &tx).await;
                }
            }
        }

        state.finish(&tx).await;
    });

    Box::pin(ReceiverStream::new(rx))
}

// ── Internal streaming state machine ────────────────────────────────

struct ConvertState {
    message_id: String,
    model: String,
    phase: Phase,
    input_tokens: u64,
    output_tokens: u64,
    finish_reason: Option<String>,
}

enum Phase {
    Start,
    InContent,
    Done,
}

impl ConvertState {
    fn new(message_id: String, model: String) -> Self {
        Self {
            message_id,
            model,
            phase: Phase::Start,
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: None,
        }
    }

    async fn ingest(&mut self, val: &Value, tx: &mpsc::Sender<String>) {
        if matches!(self.phase, Phase::Done) {
            return;
        }

        // accumulate usage
        if let Some(u) = val.get("usage") {
            let p = u.get("prompt_tokens")
                .or_else(|| u.get("input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let c = u.get("completion_tokens")
                .or_else(|| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if p > 0 {
                self.input_tokens = p;
            }
            if c > 0 {
                self.output_tokens = c;
            }
            tracing::info!(
                p, c,
                input_tokens = self.input_tokens,
                output_tokens = self.output_tokens,
                "anthropic_compat stream: usage chunk received"
            );
        }

        let choices = val
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first());

        let content = choices
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        let finish = choices
            .and_then(|c| c.get("finish_reason"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        // transition Start → InContent on first data
        if matches!(self.phase, Phase::Start) {
            if content.is_some() || finish.is_some() {
                let start = json!({
                    "type": "message_start",
                    "message": {
                        "id": self.message_id,
                        "type": "message",
                        "role": "assistant",
                        "content": [],
                        "model": self.model,
                        "stop_reason": null,
                        "stop_sequence": null,
                        "usage": {
                            "input_tokens": self.input_tokens,
                            "output_tokens": 0,
                        }
                    }
                });
                let _ = tx
                    .send(format!(
                        "event: message_start\ndata: {}\n\n",
                        serde_json::to_string(&start).unwrap_or_default()
                    ))
                    .await;
                let _ = tx
                    .send("event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_string())
                    .await;
                self.phase = Phase::InContent;
            } else {
                return;
            }
        }

        // content delta
        if let Some(text) = content {
            let delta = json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": text},
            });
            let _ = tx
                .send(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    serde_json::to_string(&delta).unwrap_or_default()
                ))
                .await;
        }

        // finish
        if let Some(fr) = finish {
            self.finish_reason = Some(fr.to_string());
            self.finish(tx).await;
        }
    }

    async fn finish(&mut self, tx: &mpsc::Sender<String>) {
        if matches!(self.phase, Phase::Done | Phase::Start) {
            return;
        }
        tracing::info!(
            input_tokens = self.input_tokens,
            output_tokens = self.output_tokens,
            "anthropic_compat stream: finish — emitting message_delta"
        );
        self.phase = Phase::Done;

        let _ = tx
            .send("event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n".to_string())
            .await;

        let stop = match self.finish_reason.as_deref() {
            Some("stop") => "end_turn",
            Some("length") => "max_tokens",
            Some("tool_calls") => "tool_use",
            Some(s) => s,
            None => "end_turn",
        };

        let delta = json!({
            "type": "message_delta",
            "delta": {"stop_reason": stop, "stop_sequence": null},
            "usage": {"input_tokens": self.input_tokens, "output_tokens": self.output_tokens},
        });
        let _ = tx
            .send(format!(
                "event: message_delta\ndata: {}\n\n",
                serde_json::to_string(&delta).unwrap_or_default()
            ))
            .await;
        let _ = tx
            .send("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_string())
            .await;
    }
}

// ── Shared helpers ──────────────────────────────────────────────────

fn blocks_to_text(blocks: &[Value]) -> String {
    let mut text = String::new();
    for b in blocks {
        if b.get("type").and_then(|v| v.as_str()) == Some("text") {
            if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                text.push_str(t);
            }
        }
    }
    text
}

fn convert_content(raw: Option<&Value>) -> Value {
    match raw {
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(Value::Array(blocks)) => {
            let mut text = String::new();
            let mut images: Vec<Value> = Vec::new();
            for b in blocks {
                match b.get("type").and_then(|v| v.as_str()) {
                    Some("text") => {
                        if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                            text.push_str(t);
                        }
                    }
                    Some("image") => {
                        if let Some(src) = b.get("source") {
                            let mime = src
                                .get("media_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("image/jpeg");
                            let data = src.get("data").and_then(|v| v.as_str()).unwrap_or("");
                            images.push(json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": format!("data:{};base64,{}", mime, data),
                                }
                            }));
                        }
                    }
                    _ => {}
                }
            }
            if images.is_empty() {
                Value::String(text)
            } else {
                let mut parts: Vec<Value> = Vec::new();
                if !text.is_empty() {
                    parts.push(json!({"type": "text", "text": text}));
                }
                parts.extend(images);
                Value::Array(parts)
            }
        }
        _ => Value::String(String::new()),
    }
}
