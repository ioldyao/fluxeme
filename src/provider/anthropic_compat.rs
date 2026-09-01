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
    async fn relay(
        &self,
        endpoint: &EndpointConfig,
        path: &str,
        body: Value,
    ) -> Result<Value, ProviderError> {
        self.inner.relay(endpoint, path, body).await
    }

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
        tracing::debug!(
            model = %model,
            openai_usage = %resp.get("usage").map(|u| u.to_string()).unwrap_or_default(),
            "anthropic_compat: raw OpenAI response usage"
        );
        let converted = openai_to_anthropic_response(&resp, &model);
        tracing::debug!(
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
        let stream = self
            .inner
            .chat_complete_stream(endpoint, openai_body)
            .await?;
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

    // messages — Anthropic content blocks → OpenAI chat messages:
    //  - text/image blocks → content parts
    //  - tool_result blocks (user msg) → separate {role: "tool"} messages
    //    so the upstream sees tool results in multi-turn conversations
    //  - tool_use blocks (assistant msg) → tool_calls array
    if let Some(anthropic_msgs) = body.get("messages").and_then(|v| v.as_array()) {
        for msg in anthropic_msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            match msg.get("content") {
                Some(Value::String(s)) => {
                    messages.push(json!({"role": role, "content": s}));
                }
                Some(Value::Array(blocks)) => {
                    let mut parts: Vec<Value> = Vec::new();
                    let mut tool_calls: Vec<Value> = Vec::new();
                    for block in blocks {
                        match block.get("type").and_then(|v| v.as_str()) {
                            Some("text") | Some("image") => {
                                if let Some(p) = convert_block(block) {
                                    parts.push(p);
                                }
                            }
                            Some("tool_result") => {
                                if !parts.is_empty() {
                                    messages.push(json!({"role": role, "content": compact_parts(std::mem::take(&mut parts))}));
                                }
                                let tool_use_id = block
                                    .get("tool_use_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let mut content = convert_tool_result_content(block.get("content"));
                                if block
                                    .get("is_error")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false)
                                {
                                    if let Value::String(ref mut s) = content {
                                        s.insert_str(0, "[tool_error] ");
                                    }
                                }
                                messages.push(json!({
                                    "role": "tool",
                                    "tool_call_id": tool_use_id,
                                    "content": content,
                                }));
                            }
                            Some("tool_use") => {
                                let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                let input = block
                                    .get("input")
                                    .cloned()
                                    .unwrap_or_else(|| Value::Object(Default::default()));
                                tool_calls.push(json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string()),
                                    }
                                }));
                            }
                            _ => {
                                // thinking / server_tool_use / others: not
                                // representable in OpenAI chat format
                            }
                        }
                    }
                    if !parts.is_empty() {
                        let mut m = json!({"role": role, "content": compact_parts(parts)});
                        if !tool_calls.is_empty() {
                            m["tool_calls"] = Value::Array(tool_calls);
                        }
                        messages.push(m);
                    } else if !tool_calls.is_empty() {
                        messages
                            .push(json!({"role": role, "content": "", "tool_calls": tool_calls}));
                    }
                }
                _ => {
                    messages.push(json!({"role": role, "content": ""}));
                }
            }
        }
    }

    let mut openai = json!({
        "model": body.get("model").cloned().unwrap_or(Value::Null),
        "messages": messages,
    });

    // Shared parameters. `max_tokens` is intentionally excluded here: it is
    // an Anthropic field and must be translated to the OpenAI field below,
    // never forwarded alongside it.
    for key in &[
        "temperature",
        "top_p",
        "top_k",
        "stop",
        "stream",
        "frequency_penalty",
        "presence_penalty",
    ] {
        if let Some(v) = body.get(key) {
            openai[key] = v.clone();
        }
    }

    // Anthropic `stop_sequences` → OpenAI `stop`
    if let Some(v) = body.get("stop_sequences") {
        openai["stop"] = v.clone();
    }

    // Translate to the OpenAI-compatible field. Do not preserve the
    // Anthropic spelling: upstream OpenAI APIs reject both fields together.
    if let Some(v) = body
        .get("max_completion_tokens")
        .cloned()
        .or_else(|| body.get("max_tokens").cloned())
    {
        openai["max_completion_tokens"] = v;
    }

    // Anthropic tools → OpenAI function tools. Claude Code sends `tools`;
    // dropping them means the upstream never sees the tool definitions and
    // tool-call conversations break.
    if let Some(tools) = body.get("tools").and_then(|v| v.as_array()) {
        let converted: Vec<Value> = tools
            .iter()
            .filter_map(|t| {
                let name = t.get("name").and_then(|v| v.as_str())?;
                Some(json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": t.get("description").cloned().unwrap_or(Value::String(String::new())),
                        "parameters": t.get("input_schema").cloned().unwrap_or(
                            json!({"type": "object", "properties": {}}),
                        ),
                    }
                }))
            })
            .collect();
        if !converted.is_empty() {
            openai["tools"] = Value::Array(converted);
        }
    }

    // tool_choice: Anthropic auto/any/tool/none → OpenAI auto/required/function/none
    if let Some(tc) = body.get("tool_choice") {
        openai["tool_choice"] = match tc.get("type").and_then(|v| v.as_str()) {
            Some("any") => json!("required"),
            Some("none") => json!("none"),
            Some("tool") => tc
                .get("name")
                .and_then(|v| v.as_str())
                .map(|name| json!({"type": "function", "function": {"name": name}}))
                .unwrap_or(json!("auto")),
            _ => json!("auto"),
        };
    }

    // Ask the upstream for the final usage chunk in streaming responses
    // (stream_options.include_usage) so cache-hit tokens can be converted
    // back; non-streaming responses always carry usage.
    if openai
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        match openai.get_mut("stream_options") {
            Some(Value::Object(opts)) => {
                opts.insert("include_usage".into(), Value::Bool(true));
            }
            _ => {
                openai["stream_options"] = json!({"include_usage": true});
            }
        }
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

    // message.tool_calls → Anthropic tool_use content blocks
    let tool_calls = openai_resp
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("tool_calls"))
        .and_then(|v| v.as_array());

    let mut content_blocks: Vec<Value> = Vec::new();
    if !content.is_empty() {
        content_blocks.push(json!({"type": "text", "text": content}));
    }
    if let Some(tcs) = tool_calls {
        for tc in tcs {
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let input: Value = serde_json::from_str(args)
                .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
            let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            content_blocks.push(json!({
                "type": "tool_use",
                "id": if id.is_empty() {
                    format!("toolu_{}", Uuid::new_v4().simple())
                } else {
                    id.to_string()
                },
                "name": name,
                "input": input,
            }));
        }
    }
    if content_blocks.is_empty() {
        content_blocks.push(json!({"type": "text", "text": ""}));
    }

    let usage = openai_resp.get("usage");
    // Try standard OpenAI field names first, then fall back to
    // alternative naming used by some OpenAI-compatible endpoints.
    let prompt_tokens = usage
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
    // OpenAI `prompt_tokens` INCLUDES cached tokens; Anthropic semantics
    // are "total input = input_tokens + cache_read_input_tokens +
    // cache_creation_input_tokens", so input_tokens must exclude them.
    let cache_read = usage
        .and_then(|u| u.get("prompt_tokens_details"))
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_write = usage
        .and_then(|u| u.get("prompt_tokens_details"))
        .and_then(|d| d.get("cache_write_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let input_tokens = prompt_tokens.saturating_sub(cache_read);

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
        "content": content_blocks,
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
    if cache_write > 0 {
        resp["usage"]["cache_creation_input_tokens"] = json!(cache_write);
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
    started: bool,
    phase: Phase,
    /// Anthropic block index of the open text block, if any.
    text_block_index: Option<usize>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    finish_reason: Option<String>,
    /// OpenAI tool index → (anthropic block index, anthropic tool id, name).
    tools: std::collections::HashMap<usize, (usize, String, String)>,
    block_seq: usize,
}

enum Phase {
    Start,
    InText,
    InTool,
    Done,
}

impl ConvertState {
    fn new(message_id: String, model: String) -> Self {
        Self {
            message_id,
            model,
            started: false,
            phase: Phase::Start,
            text_block_index: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            finish_reason: None,
            tools: std::collections::HashMap::new(),
            block_seq: 0,
        }
    }

    async fn ensure_started(&mut self, tx: &mpsc::Sender<String>) {
        if self.started {
            return;
        }
        self.started = true;
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
    }

    async fn close_text_block(&mut self, tx: &mpsc::Sender<String>) {
        if let Some(index) = self.text_block_index.take() {
            let _ = tx
                .send(format!(
                    "event: content_block_stop\ndata: {}\n\n",
                    serde_json::to_string(&json!({"type": "content_block_stop", "index": index}))
                        .unwrap_or_default()
                ))
                .await;
        }
    }

    async fn ingest(&mut self, val: &Value, tx: &mpsc::Sender<String>) {
        if matches!(self.phase, Phase::Done) {
            return;
        }

        // accumulate usage
        if let Some(u) = val.get("usage") {
            let p = u
                .get("prompt_tokens")
                .or_else(|| u.get("input_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let c = u
                .get("completion_tokens")
                .or_else(|| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            // OpenAI prompt_tokens includes cached tokens; Anthropic
            // input_tokens must exclude them (total input = input_tokens +
            // cache_read + cache_creation).
            let cached = u
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cache_write = u
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cache_write_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if p > 0 {
                self.input_tokens = p.saturating_sub(cached);
            }
            if cached > 0 {
                self.cache_read_tokens = cached;
            }
            if cache_write > 0 {
                self.cache_creation_tokens = cache_write;
            }
            if c > 0 {
                self.output_tokens = c;
            }
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

        let tool_calls = choices
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("tool_calls"))
            .and_then(|v| v.as_array());

        let finish = choices
            .and_then(|c| c.get("finish_reason"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());

        // text content delta
        if let Some(text) = content {
            if matches!(self.phase, Phase::Start) {
                self.ensure_started(tx).await;
                let index = self.block_seq;
                self.block_seq += 1;
                let _ = tx
                    .send(format!(
                        "event: content_block_start\ndata: {}\n\n",
                        serde_json::to_string(&json!({
                            "type": "content_block_start",
                            "index": index,
                            "content_block": {"type": "text", "text": ""},
                        }))
                        .unwrap_or_default()
                    ))
                    .await;
                self.text_block_index = Some(index);
                self.phase = Phase::InText;
            } else if matches!(self.phase, Phase::InTool) {
                // interleaved text after a tool block started — open another
                // text block
                let index = self.block_seq;
                self.block_seq += 1;
                let _ = tx
                    .send(format!(
                        "event: content_block_start\ndata: {}\n\n",
                        serde_json::to_string(&json!({
                            "type": "content_block_start",
                            "index": index,
                            "content_block": {"type": "text", "text": ""},
                        }))
                        .unwrap_or_default()
                    ))
                    .await;
                self.text_block_index = Some(index);
            }
            if let Some(index) = self.text_block_index {
                let delta = json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": {"type": "text_delta", "text": text},
                });
                let _ = tx
                    .send(format!(
                        "event: content_block_delta\ndata: {}\n\n",
                        serde_json::to_string(&delta).unwrap_or_default()
                    ))
                    .await;
            }
        }

        // tool_calls deltas → tool_use content blocks + input_json_delta
        if let Some(tcs) = tool_calls {
            for tc in tcs {
                let Some(oi) = tc.get("index").and_then(|v| v.as_u64()) else {
                    continue;
                };
                let oi = oi as usize;
                if !self.tools.contains_key(&oi) {
                    self.ensure_started(tx).await;
                    self.close_text_block(tx).await;
                    let name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let id = format!("toolu_{}", Uuid::new_v4().simple());
                    let index = self.block_seq;
                    self.block_seq += 1;
                    let block = json!({
                        "type": "content_block_start",
                        "index": index,
                        "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}},
                    });
                    let _ = tx
                        .send(format!(
                            "event: content_block_start\ndata: {}\n\n",
                            serde_json::to_string(&block).unwrap_or_default()
                        ))
                        .await;
                    self.tools.insert(oi, (index, id, name));
                    self.phase = Phase::InTool;
                }
                let Some((index, _, _)) = self.tools.get(&oi) else {
                    continue;
                };
                let args = tc
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !args.is_empty() {
                    let delta = json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "input_json_delta", "partial_json": args},
                    });
                    let _ = tx
                        .send(format!(
                            "event: content_block_delta\ndata: {}\n\n",
                            serde_json::to_string(&delta).unwrap_or_default()
                        ))
                        .await;
                }
            }
        }

        // finish — record the reason but do NOT finalize here. OpenAI sends
        // the final usage chunk AFTER the finish_reason chunk (right before
        // [DONE]); finalizing now would set Phase::Done and the subsequent
        // ingest would drop the usage chunk. The terminal message_delta /
        // message_stop are emitted when the stream ends (see the wrapper).
        if let Some(fr) = finish {
            self.finish_reason = Some(fr.to_string());
        }
    }

    async fn finish(&mut self, tx: &mpsc::Sender<String>) {
        if matches!(self.phase, Phase::Done) {
            return;
        }
        // Emit message_start even when no content arrived, so the client
        // always gets a well-formed terminal sequence.
        if !self.started {
            self.ensure_started(tx).await;
        }
        self.phase = Phase::Done;

        self.close_text_block(tx).await;
        // close each open tool block, ordered by anthropic block index
        let mut tool_blocks: Vec<usize> = self.tools.values().map(|(i, _, _)| *i).collect();
        tool_blocks.sort_unstable();
        for index in tool_blocks {
            let _ = tx
                .send(format!(
                    "event: content_block_stop\ndata: {}\n\n",
                    serde_json::to_string(&json!({"type": "content_block_stop", "index": index}))
                        .unwrap_or_default()
                ))
                .await;
        }

        let stop = match self.finish_reason.as_deref() {
            Some("stop") => "end_turn",
            Some("length") => "max_tokens",
            Some("tool_calls") => "tool_use",
            Some(s) => s,
            None => "end_turn",
        };

        let mut usage_json = json!({
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
        });
        if self.cache_read_tokens > 0 {
            usage_json["cache_read_input_tokens"] = json!(self.cache_read_tokens);
        }
        if self.cache_creation_tokens > 0 {
            usage_json["cache_creation_input_tokens"] = json!(self.cache_creation_tokens);
        }
        let delta = json!({
            "type": "message_delta",
            "delta": {"stop_reason": stop, "stop_sequence": null},
            "usage": usage_json,
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

/// Convert a single Anthropic content block to its OpenAI equivalent.
/// Returns `None` for blocks that have no OpenAI representation (thinking,
/// server_tool_use, etc.).
fn convert_block(block: &Value) -> Option<Value> {
    match block.get("type").and_then(|v| v.as_str()) {
        Some("text") => block
            .get("text")
            .and_then(|v| v.as_str())
            .map(|t| json!({"type": "text", "text": t})),
        Some("image") => {
            let src = block.get("source")?;
            let mime = src
                .get("media_type")
                .and_then(|v| v.as_str())
                .unwrap_or("image/jpeg");
            let data = src.get("data").and_then(|v| v.as_str()).unwrap_or("");
            Some(json!({
                "type": "image_url",
                "image_url": {"url": format!("data:{};base64,{}", mime, data)},
            }))
        }
        _ => None,
    }
}

/// OpenAI tool-message content: accepts a string or an array of text/image
/// parts. A single text part is collapsed to a plain string.
fn convert_tool_result_content(raw: Option<&Value>) -> Value {
    match raw {
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(Value::Array(blocks)) => {
            let parts: Vec<Value> = blocks.iter().filter_map(convert_block).collect();
            if parts.len() == 1 {
                if let Some(t) = parts[0].get("text").and_then(|v| v.as_str()) {
                    return Value::String(t.to_string());
                }
            }
            if parts.is_empty() {
                Value::String(String::new())
            } else {
                Value::Array(parts)
            }
        }
        _ => Value::String(String::new()),
    }
}

/// Collapse a list of OpenAI content parts to a plain string when it holds
/// exactly one text part, otherwise keep the array.
fn compact_parts(parts: Vec<Value>) -> Value {
    if parts.len() == 1 {
        if let Some(t) = parts[0].get("text").and_then(|v| v.as_str()) {
            return Value::String(t.to_string());
        }
    }
    Value::Array(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[test]
    #[test]
    fn forwards_tool_result_and_tool_use_messages() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "user", "content": "list the dir"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_01", "name": "Bash", "input": {"command": "ls"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_01", "content": "a.txt\nb.txt"}
                ]},
            ],
            "tools": [{"name": "Bash", "input_schema": {"type": "object"}}],
        });
        let openai = anthropic_to_openai(&body);
        let msgs = openai["messages"].as_array().expect("messages");
        assert_eq!(msgs.len(), 3);
        // assistant tool_use block → tool_calls on the assistant message
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["tool_calls"][0]["id"], "toolu_01");
        assert_eq!(msgs[1]["tool_calls"][0]["function"]["name"], "Bash");
        assert_eq!(
            msgs[1]["tool_calls"][0]["function"]["arguments"],
            "{\"command\":\"ls\"}"
        );
        // tool_result block → separate OpenAI tool message
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "toolu_01");
        assert_eq!(msgs[2]["content"], "a.txt\nb.txt");
    }

    #[test]
    fn translates_anthropic_max_tokens_to_openai_only() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 4096,
        });
        let openai = anthropic_to_openai(&body);
        assert_eq!(openai["max_completion_tokens"], 4096);
        assert!(openai.get("max_tokens").is_none());
    }

    #[test]
    fn prefers_existing_openai_completion_token_field() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 4096,
            "max_completion_tokens": 2048,
        });
        let openai = anthropic_to_openai(&body);
        assert_eq!(openai["max_completion_tokens"], 2048);
        assert!(openai.get("max_tokens").is_none());
    }

    #[test]
    fn forwards_tools_to_openai() {
        let body = json!({
            "model": "m",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{
                "name": "Bash",
                "description": "run a command",
                "input_schema": {"type": "object", "properties": {"command": {"type": "string"}}},
            }],
            "tool_choice": {"type": "any"},
        });
        let openai = anthropic_to_openai(&body);
        let tools = openai["tools"].as_array().expect("tools forwarded");
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "Bash");
        assert_eq!(tools[0]["function"]["parameters"]["type"], "object");
        assert_eq!(openai["tool_choice"], "required");
    }

    #[test]
    fn converts_tool_calls_in_non_streaming_response() {
        let resp = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "let me run",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "Bash", "arguments": "{\"command\":\"ls\"}"},
                    }],
                },
                "finish_reason": "tool_calls",
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5},
        });
        let anthropic = openai_to_anthropic_response(&resp, "m");
        let blocks = anthropic["content"].as_array().expect("content blocks");
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["name"], "Bash");
        assert_eq!(blocks[1]["input"]["command"], "ls");
        assert!(!blocks[1]["id"].as_str().unwrap().is_empty());
        assert_eq!(anthropic["stop_reason"], "tool_use");
    }

    #[test]
    fn maps_cache_fields_and_excludes_cached_from_input() {
        let resp = json!({
            "choices": [{
                "message": {"role": "assistant", "content": "hi"},
                "finish_reason": "stop",
            }],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 10,
                "total_tokens": 110,
                "prompt_tokens_details": {"cached_tokens": 40, "cache_write_tokens": 60},
            },
        });
        let anthropic = openai_to_anthropic_response(&resp, "m");
        // input_tokens excludes cached (Anthropic: total = input + cache_read + cache_creation)
        assert_eq!(anthropic["usage"]["input_tokens"], 60);
        assert_eq!(anthropic["usage"]["output_tokens"], 10);
        assert_eq!(anthropic["usage"]["cache_read_input_tokens"], 40);
        assert_eq!(anthropic["usage"]["cache_creation_input_tokens"], 60);
    }

    #[tokio::test]
    async fn streams_cache_fields_to_anthropic_usage() {
        let chunks = vec![
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n"
                .to_string(),
            // Real upstream order: finish_reason chunk comes BEFORE the
            // usage chunk — the converter must not finalize on the former.
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n".to_string(),
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"prompt_tokens_details\":{\"cached_tokens\":40,\"cache_write_tokens\":60}}}\n\n"
                .to_string(),
            "data: [DONE]\n\n".to_string(),
        ];
        let stream = futures::stream::iter(chunks);
        let mut out = wrap_openai_sse_for_anthropic(Box::pin(stream), "m".to_string());
        let mut all = String::new();
        while let Some(chunk) = out.next().await {
            all.push_str(&chunk);
        }
        assert!(
            all.contains("\"cache_read_input_tokens\":40"),
            "missing cache read: {all}"
        );
        assert!(
            all.contains("\"cache_creation_input_tokens\":60"),
            "missing cache creation: {all}"
        );
        assert!(
            all.contains("\"input_tokens\":60"),
            "input should exclude cached: {all}"
        );
    }

    #[tokio::test]
    async fn streams_tool_calls_to_anthropic_sse() {
        let chunks = vec![
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"Bash\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n"
                .to_string(),
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"command\\\":\\\"ls\\\"}\"}}]},\"finish_reason\":null}]}\n\n"
                .to_string(),
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n".to_string(),
            "data: [DONE]\n\n".to_string(),
        ];
        let stream = futures::stream::iter(chunks);
        let mut out = wrap_openai_sse_for_anthropic(Box::pin(stream), "m".to_string());
        let mut all = String::new();
        while let Some(chunk) = out.next().await {
            all.push_str(&chunk);
        }
        assert!(
            all.contains("\"type\":\"content_block_start\""),
            "missing block start: {all}"
        );
        assert!(
            all.contains("\"type\":\"tool_use\""),
            "missing tool_use block: {all}"
        );
        assert!(
            all.contains("input_json_delta"),
            "missing input_json_delta: {all}"
        );
        assert!(all.contains("partial_json"), "missing partial_json: {all}");
        assert!(
            all.contains("\"stop_reason\":\"tool_use\""),
            "missing tool_use stop: {all}"
        );
        assert!(all.contains("message_stop"), "missing message_stop: {all}");
    }
}
