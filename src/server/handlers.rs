use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use bytes::Bytes;
use chrono::Utc;
use futures::stream::StreamExt;
use futures::Future;
use futures::Stream;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use uuid::Uuid;

use crate::balancer::LoadBalancer;
use crate::cache::GateStatus;
use crate::config::types::EndpointConfig;
use crate::domain::usage::UsageRecord;
use crate::provider::{is_retryable_error, ErrorKind};
use crate::server::AppState;
use crate::service::moderation::FilterBlocked;

// ── Error type ────────────────────────────────────────────────────

#[derive(Debug)]
pub enum GatewayError {
    Auth(String),
    RateLimit(String),
    Route(String),
    BadRequest(String),
    Upstream(String),
    Internal(String),
    PaymentRequired(String),
}

impl GatewayError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Auth(_) => StatusCode::UNAUTHORIZED,
            Self::RateLimit(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Route(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PaymentRequired(_) => StatusCode::PAYMENT_REQUIRED,
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Auth(m)
            | Self::RateLimit(m)
            | Self::Route(m)
            | Self::BadRequest(m)
            | Self::Upstream(m)
            | Self::PaymentRequired(m) => m,
            Self::Internal(_) => "Internal server error",
        }
    }
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": {
                "message": self.message(),
                "type": "gateway_error",
            }
        });
        (self.status(), Json(body)).into_response()
    }
}

impl From<crate::service::auth::AuthError> for GatewayError {
    fn from(e: crate::service::auth::AuthError) -> Self {
        Self::Auth(e.0)
    }
}

impl From<crate::service::routing::RouteError> for GatewayError {
    fn from(e: crate::service::routing::RouteError) -> Self {
        Self::Route(e.0)
    }
}

impl From<crate::ratelimit::RateLimitError> for GatewayError {
    fn from(e: crate::ratelimit::RateLimitError) -> Self {
        Self::RateLimit(e.0)
    }
}

impl From<crate::provider::ProviderError> for GatewayError {
    fn from(e: crate::provider::ProviderError) -> Self {
        Self::Upstream(e.0)
    }
}

impl From<FilterBlocked> for GatewayError {
    fn from(e: FilterBlocked) -> Self {
        Self::BadRequest(e.0)
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn trim_model(body: &mut Value) -> Result<String, GatewayError> {
    let model_val = body["model"].clone();
    let s = model_val
        .as_str()
        .ok_or_else(|| GatewayError::BadRequest("Missing 'model' field".into()))?
        .trim()
        .to_string();
    if s.is_empty() {
        return Err(GatewayError::BadRequest("'model' field is empty".into()));
    }
    body["model"] = Value::String(s.clone());
    Ok(s)
}

/// Move inline `role: "system"` messages to the top-level Anthropic `system`
/// field. Claude Code occasionally sends system prompts as inline messages
/// with role="system", which SGLang's /v1/messages rejects (only "user" and
/// "assistant" are allowed in the messages array).
fn normalize_messages_body(body: &mut Value) {
    let existing_system = body
        .get("system")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };

    // Collect inline system messages into a single string
    let mut system_text = String::new();
    let mut filtered = Vec::new();

    for msg in messages.drain(..) {
        if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
            if let Some(content) = msg.get("content") {
                match content {
                    Value::String(s) => {
                        if !system_text.is_empty() { system_text.push('\n'); }
                        system_text.push_str(s);
                    }
                    Value::Array(blocks) => {
                        for block in blocks {
                            if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                                if !system_text.is_empty() { system_text.push('\n'); }
                                system_text.push_str(t);
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else {
            filtered.push(msg);
        }
    }

    *messages = filtered;

    // Merge extracted inline system text with any pre-existing top-level system
    if !system_text.is_empty() {
        let merged = if existing_system.is_empty() {
            system_text
        } else {
            format!("{}\n{}", system_text, existing_system)
        };
        body["system"] = Value::String(merged);
    }
}

/// Check whether the user's wallet balance is sufficient for this request.
///
/// Three-tier check:
///   1. Redis gate_status (fast path)
///   2. In-memory gate cache (populated by inspection task, avoids SQLite
///      mutex contention during Redis outages)
///   3. SQLite `get_wallet_balance` (source of truth, final fallback)
async fn check_wallet_balance(
    state: &AppState,
    user_id: &str,
) -> Result<(), GatewayError> {
    match state.cache.get_gate_status(user_id).await {
        Ok(Some(GateStatus::Blocked)) => {
            return Err(GatewayError::PaymentRequired("Insufficient balance".into()));
        }
        Ok(Some(_)) => return Ok(()), // ok or low — pass through
        Ok(None) => {} // fall through to local cache
        Err(e) => {
            tracing::warn!(user_id, "Gate status read error, trying local cache: {}", e);
        }
    }
    // Second fallback — in-memory gate cache (no Redis, no SQLite mutex)
    {
        let guard = state.gate_cache.read().await;
        if let Some(status) = guard.get(user_id) {
            return match status {
                GateStatus::Blocked => Err(GatewayError::PaymentRequired("Insufficient balance".into())),
                _ => Ok(()),
            };
        }
    }
    // Final fallback — read from SQLite directly
    let (balance, frozen) = state.db.get_wallet_balance(user_id)
        .await
        .map_err(|e| GatewayError::Internal(e.0))?;
    if balance - frozen < 0.0001 {
        return Err(GatewayError::PaymentRequired("Insufficient balance".into()));
    }
    Ok(())
}

/// Non-standard reasoning field names from various providers to normalize.
const REASONING_ALIASES: &[&str] = &["reasoning", "thinking", "thinking_content"];

fn rename_to_reasoning_content(obj: &mut serde_json::Map<String, Value>) {
    if obj.contains_key("reasoning_content") {
        return;
    }
    for &alias in REASONING_ALIASES {
        if let Some(val) = obj.remove(alias) {
            obj.insert("reasoning_content".into(), val);
            return;
        }
    }
}

fn normalize_reasoning_inner(val: &mut Value) {
    if let Some(choices) = val.get_mut("choices").and_then(|c| c.as_array_mut()) {
        for choice in choices.iter_mut() {
            if let Some(msg) = choice.get_mut("message").and_then(|m| m.as_object_mut()) {
                rename_to_reasoning_content(msg);
            }
            if let Some(delta) = choice.get_mut("delta").and_then(|m| m.as_object_mut()) {
                rename_to_reasoning_content(delta);
            }
        }
    }
}

fn normalize_sse_reasoning(data: &str) -> String {
    let mut out = String::with_capacity(data.len());
    for line in data.lines() {
        let trimmed = line.trim();
        if let Some(json_str) = trimmed.strip_prefix("data: ") {
            if json_str.trim() == "[DONE]" {
                out.push_str(line);
                out.push('\n');
                continue;
            }
            if let Ok(mut val) = serde_json::from_str::<Value>(json_str) {
                normalize_reasoning_inner(&mut val);
                let indent = &line[..line.len() - trimmed.len()];
                out.push_str(indent);
                out.push_str("data: ");
                out.push_str(&serde_json::to_string(&val).unwrap_or_default());
                out.push('\n');
            } else {
                out.push_str(line);
                out.push('\n');
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

// ── Helpers ─────────────────────────────────────────────────────────

fn extract_client_ip(headers: &HeaderMap, addr: SocketAddr) -> String {
    if let Some(fwd) = headers.get("x-forwarded-for") {
        if let Ok(s) = fwd.to_str() {
            if let Some(ip) = s.split(',').next().map(|s| s.trim()) {
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }
    if let Some(real) = headers.get("x-real-ip") {
        if let Ok(s) = real.to_str() {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    addr.ip().to_string()
}

struct RouteTarget {
    channel_id: String,
    endpoint: EndpointConfig,
    adapter: Arc<dyn crate::provider::ProviderAdapter>,
    balancer: Arc<LoadBalancer>,
}

impl RouteTarget {
    /// Try the next available endpoint from the balancer.
    /// Returns `false` if no more endpoints available.
    fn retry_next(&mut self) -> bool {
        if let Some((idx, ep)) = self.balancer.as_health_aware().select() {
            self.endpoint = ep.clone();
            true
        } else {
            false
        }
    }
}

fn resolve_route(state: &AppState, channel_id: &str) -> Result<RouteTarget, GatewayError> {
    let (provider_name, balancer) = state
        .routing
        .get_route(channel_id)
        .ok_or_else(|| GatewayError::Internal("Channel route unavailable".into()))?;

    let adapter = state
        .providers
        .get(provider_name.as_str())
        .ok_or_else(|| GatewayError::Internal("Provider not available".into()))?;

    let (idx, endpoint) = balancer
        .as_health_aware()
        .select()
        .ok_or_else(|| GatewayError::Internal("No available endpoints".into()))?;

    Ok(RouteTarget {
        channel_id: channel_id.to_string(),
        endpoint: endpoint.clone(),
        adapter,
        balancer,
    })
}

// ── Streaming ─────────────────────────────────────────────────────

/// Extract reasoning and output content from raw SSE data.
/// Returns (reasoning, content) extracted from delta chunks.
fn extract_sse_content(data: &str) -> (String, String) {
    let mut reasoning = String::new();
    let mut content = String::new();
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "data: [DONE]" || trimmed.starts_with("event: ") {
            continue;
        }
        let json_str = trimmed.strip_prefix("data: ").unwrap_or(trimmed);
        if let Ok(val) = serde_json::from_str::<Value>(json_str) {
            // OpenAI format: choices[0].delta.{reasoning_content, content}
            if let Some(delta) = val.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("delta")) {
                if let Some(text) = delta.get("reasoning") // normalized name (from normalize_sse_reasoning)
                    .or_else(|| delta.get("reasoning_content"))
                    .and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    reasoning.push_str(text);
                }
                if let Some(text) = delta.get("content").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    content.push_str(text);
                }
            }
            // Anthropic format: content_block_delta delta.{thinking, text}
            if val.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                if let Some(delta) = val.get("delta") {
                    if delta.get("type").and_then(|t| t.as_str()) == Some("thinking_delta") {
                        if let Some(text) = delta.get("thinking").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                            reasoning.push_str(text);
                        }
                    }
                    if delta.get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                        if let Some(text) = delta.get("text").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                            content.push_str(text);
                        }
                    }
                }
            }
        }
    }
    (reasoning, content)
}

/// Parse token usage from accumulated SSE data.
/// Scans forward, taking the max for each token type — handles both
/// OpenAI (final chunk has all usage) and Anthropic (message_start has
/// prompt_tokens, message_delta has completion_tokens).
fn parse_sse_usage(data: &str) -> (u64, u64, u64) {
    let mut p_tokens = 0u64;
    let mut c_tokens = 0u64;
    let mut cache_hit = 0u64;
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "data: [DONE]" || trimmed.starts_with("event: ") {
            continue;
        }
        let json_str = trimmed.strip_prefix("data: ").unwrap_or(trimmed);
        if let Ok(val) = serde_json::from_str::<Value>(json_str) {
            // OpenAI format: {usage: {prompt_tokens, completion_tokens, prompt_tokens_details: {cached_tokens}}}
            if let Some(usage) = val.get("usage") {
                if usage.is_null() { continue; }
                let p = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let c = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                if p > p_tokens { p_tokens = p; }
                if c > c_tokens { c_tokens = c; }
                if let Some(details) = usage.get("prompt_tokens_details") {
                    if let Some(cached) = details.get("cached_tokens").and_then(|v| v.as_u64()) {
                        if cached > cache_hit { cache_hit = cached; }
                    }
                }
            }
            // Anthropic message_start: {type: "message_start", message: {usage: {input_tokens, output_tokens, cache_read_input_tokens}}}
            if val.get("type").and_then(|t| t.as_str()) == Some("message_start") {
                if let Some(msg) = val.get("message") {
                    if let Some(usage) = msg.get("usage") {
                        if let Some(p) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                            if p > p_tokens { p_tokens = p; }
                        }
                        if let Some(c) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                            if c > c_tokens { c_tokens = c; }
                        }
                        if let Some(cached) = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()) {
                            if cached > cache_hit { cache_hit = cached; }
                        }
                    }
                }
            }
            // Anthropic message_delta: {type: "message_delta", usage: {output_tokens}}
            if val.get("type").and_then(|t| t.as_str()) == Some("message_delta") {
                if let Some(usage) = val.get("usage") {
                    if let Some(c) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                        if c > c_tokens { c_tokens = c; }
                    }
                }
            }
        }
    }
    (p_tokens, c_tokens, cache_hit)
}

// ── SSE buffering stream ────────────────────────────────────────────

const MAX_SSE_BUF: usize = 1024 * 1024;

/// Check whether a leftover buffer (incomplete SSE tail) contains only
/// valid `data:` JSON lines.  Used at EOF to avoid forwarding truncated
/// events to the client.
fn sse_tail_is_valid(tail: &str) -> bool {
    for line in tail.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "data: [DONE]" {
            continue;
        }
        if let Some(json_str) = trimmed.strip_prefix("data: ") {
            if serde_json::from_str::<Value>(json_str).is_err() {
                return false;
            }
        }
    }
    true
}

/// Buffers incoming stream data at `\n\n` boundaries so downstream code
/// always receives complete SSE events.  This prevents malformed JSON when
/// a TCP segment splits a `data: {...}` line across two chunks.
///
/// Safety mechanisms:
/// - Buffer capped at 1 MB — beyond that an error event is emitted and the
///   stream is closed.
/// - At EOF any leftover data that doesn't form valid JSON is silently
///   dropped (with a warning) instead of forwarded to the client.
struct SseBuffer<S> {
    inner: S,
    buf: String,
    overflow_error: Option<String>,
}

impl<S: Stream<Item = String> + Unpin> Stream for SseBuffer<S> {
    type Item = String;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 1) Deliver a pending overflow error event first
        if let Some(err) = self.overflow_error.take() {
            return Poll::Ready(Some(err));
        }

        // 2) Yield complete events from the existing buffer
        if let Some(pos) = self.buf.find("\n\n") {
            let complete = self.buf[..pos + 2].to_string();
            self.buf = self.buf[pos + 2..].to_string();
            return Poll::Ready(Some(complete));
        }

        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(data)) => {
                    // 3) Buffer-overflow protection
                    if self.buf.len() + data.len() > MAX_SSE_BUF {
                        tracing::warn!(
                            buf_len = self.buf.len(),
                            "SSE buffer exceeded {} byte limit, terminating stream",
                            MAX_SSE_BUF,
                        );
                        self.overflow_error = Some(
                            "data: {\"error\":\"buffer_overflow\",\"message\":\"SSE buffer exceeded 1MB limit\"}\n\n"
                                .to_string(),
                        );
                        // Discard accumulated data and signal overflow
                        // on the next poll
                        return Poll::Ready(None);
                    }

                    self.buf.push_str(&data);
                    if let Some(pos) = self.buf.find("\n\n") {
                        let complete = self.buf[..pos + 2].to_string();
                        self.buf = self.buf[pos + 2..].to_string();
                        return Poll::Ready(Some(complete));
                    }
                }
                Poll::Ready(None) => {
                    if !self.buf.is_empty() {
                        if sse_tail_is_valid(&self.buf) {
                            let remaining = std::mem::take(&mut self.buf);
                            return Poll::Ready(Some(remaining));
                        }
                        tracing::warn!(
                            buf_len = self.buf.len(),
                            first = &self.buf.chars().take(80).collect::<String>(),
                            "Dropping invalid SSE tail at stream EOF"
                        );
                        self.buf.clear();
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

// ── Usage-tracking stream wrapper ─────────────────────────────────

struct UsageTrackingStream<S> {
    inner: S,
    resp_buf: String,
    usage: crate::service::UsageService,
    request_id: String,
    user_id: String,
    user_name: String,
    api_key_name: String,
    channel_id: String,
    model: String,
    start: Instant,
    req_body: Option<String>,
    api_format: String,
    recorded: bool,
    client_ip: String,
    endpoint_id: Option<i64>,
}

impl<S: Stream<Item = String> + Unpin> Stream for UsageTrackingStream<S> {
    type Item = String;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(data)) => {
                self.resp_buf.push_str(&data);
                Poll::Ready(Some(data))
            }
            Poll::Ready(None) => {
                self.record_usage(true);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> Drop for UsageTrackingStream<S> {
    fn drop(&mut self) {
        if !self.recorded {
            self.record_usage(false);
        }
    }
}

impl<S> UsageTrackingStream<S> {
    fn record_usage(&mut self, completed: bool) {
        if self.recorded {
            return;
        }
        self.recorded = true;

        let latency_ms = self.start.elapsed().as_millis() as u64;
        let (mut p_tokens, mut c_tokens, cache_hit) = parse_sse_usage(&self.resp_buf);

        // For cancelled streams (status 499): if SSE has content but no usage
        // data arrived (usage is only in the final chunk), estimate from text
        // length. Rough: ~4 chars/token for English, ~2 for CJK.
        if !completed && p_tokens == 0 && c_tokens == 0 {
            let (reasoning, content) = extract_sse_content(&self.resp_buf);
            let total_content = reasoning.len() + content.len();
            if total_content > 0 {
                if let Some(ref body) = self.req_body {
                    p_tokens = (body.len() / 4).max(1) as u64;
                }
                c_tokens = (total_content / 3).max(1) as u64;
            }
        }

        self.usage.record_with_endpoint(UsageRecord {
            timestamp: Utc::now().to_rfc3339(),
            request_id: self.request_id.clone(),
            user_id: self.user_id.clone(),
            user_name: self.user_name.clone(),
            channel_id: self.channel_id.clone(),
                model: self.model.clone(),
            prompt_tokens: p_tokens,
            completion_tokens: c_tokens,
            total_tokens: p_tokens + c_tokens,
            cache_hit_input_tokens: cache_hit,
            latency_ms,
            status_code: if completed { 200 } else { 499 },
            success: completed,
            request_body: self.req_body.clone(),
            api_key_name: Some(self.api_key_name.clone()),
            api_format: self.api_format.clone(),
            reasoning_body: {
                let (reasoning, _) = extract_sse_content(&self.resp_buf);
                Some(if reasoning.len() > 102400 {
                    reasoning.chars().take(102400).collect()
                } else {
                    reasoning
                })
            },
            response_body: {
                let (reasoning, content) = extract_sse_content(&self.resp_buf);
                let text = if content.is_empty() { reasoning } else { content };
                Some(if text.len() > 102400 {
                    text.chars().take(102400).collect()
                } else {
                    text
                })
            },
            stream: true,
            prompt_price: 0.0,
            completion_price: 0.0,
            cache_read_price: 0.0,
            client_ip: Some(self.client_ip.clone()),
        }, self.endpoint_id);
    }
}

// ── Idle-timeout stream wrapper ────────────────────────────────────

/// Wraps a stream with an idle timeout. If no data arrives within the
/// timeout window, an error SSE event is emitted and the stream terminates.
///
/// The first timeout waits `first_byte_timeout`; subsequent timeouts use
/// `idle_timeout`.  This lets callers set a generous initial allowance
/// for model "thinking" before tightening the per-chunk expectation.
struct IdleTimeoutStream {
    inner: Pin<Box<dyn Stream<Item = String> + Send>>,
    first_byte_timeout: Duration,
    idle_timeout: Duration,
    sleep: Pin<Box<tokio::time::Sleep>>,
    has_received_data: bool,
    timed_out: bool,
}

impl IdleTimeoutStream {
    fn new(
        inner: Pin<Box<dyn Stream<Item = String> + Send>>,
        first_byte_timeout: Duration,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            inner,
            first_byte_timeout,
            idle_timeout,
            sleep: Box::pin(tokio::time::sleep(first_byte_timeout)),
            has_received_data: false,
            timed_out: false,
        }
    }
}

impl Stream for IdleTimeoutStream {
    type Item = String;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.timed_out {
            return Poll::Ready(None);
        }

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(data)) => {
                if !this.has_received_data {
                    this.has_received_data = true;
                }
                this.sleep.as_mut().reset(
                    tokio::time::Instant::now() + this.idle_timeout,
                );
                Poll::Ready(Some(data))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => {
                if this.sleep.as_mut().poll(cx).is_ready() {
                    tracing::warn!(
                        first_byte = !this.has_received_data,
                        "Stream idle timeout reached"
                    );
                    this.timed_out = true;
                    Poll::Ready(Some(
                        "data: {\"error\":\"idle_timeout\",\"message\":\"Stream idle timeout\"}\n\n"
                            .to_string(),
                    ))
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

async fn handle_streaming(
    state: &AppState,
    adapter: Arc<dyn crate::provider::ProviderAdapter>,
    endpoint: EndpointConfig,
    body: Value,
    request_id: String,
    user_id: String,
    user_name: String,
    api_key_name: String,
    channel_id: String,
    model: String,
    start: Instant,
    client_ip: String,
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    let stream_result = adapter.chat_complete_stream(&endpoint, body).await;

    match stream_result {
        Ok(stream) => {
            // Real-time routing event is broadcast by UsageService.record() when
            // the UsageTrackingStream completes (avoids double-counting).
            let (first_byte_timeout, idle_timeout) = {
                let gw = state.gateway_config.read().unwrap();
                (
                    Duration::from_secs(gw.stream_first_byte_timeout_secs),
                    Duration::from_secs(gw.stream_idle_timeout_secs),
                )
            };
            let stream = IdleTimeoutStream::new(stream, first_byte_timeout, idle_timeout);
            let stream = SseBuffer { inner: stream, buf: String::new(), overflow_error: None }
                .map(|data| normalize_sse_reasoning(&data));
            let usage_stream = UsageTrackingStream {
                inner: stream,
                resp_buf: String::new(),
                usage: state.usage.clone(),
                request_id,
                user_id,
                user_name,
                api_key_name,
                channel_id,
                model,
                start,
                req_body,
                api_format: "openai".to_string(),
                recorded: false,
                client_ip,
                endpoint_id: endpoint.id,
            };

            let body_stream = usage_stream.map(|data| {
                Ok::<_, std::convert::Infallible>(Bytes::from(data))
            });

            Ok(Response::builder()
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("connection", "keep-alive")
                .body(Body::from_stream(body_stream))
                .map_err(|e| GatewayError::Internal(format!("Response build error: {}", e)))?)
        }
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            let (p_tokens, c_tokens, cache_hit) = parse_sse_usage("");
            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id,
                user_name,
                channel_id,
                model,
                prompt_tokens: p_tokens,
                completion_tokens: c_tokens,
                total_tokens: p_tokens + c_tokens,
                cache_hit_input_tokens: cache_hit,
                latency_ms,
                status_code: 502,
                success: false,
                request_body: req_body,
                response_body: None,
                reasoning_body: None,
                api_key_name: Some(api_key_name),
                api_format: "openai".to_string(),
                stream: true,
                prompt_price: 0.0,
                completion_price: 0.0,
                cache_read_price: 0.0,
                client_ip: Some(client_ip),
            });
            Err(GatewayError::Upstream(e.0))
        }
    }
}

// ── Messages streaming (Anthropic-native format) ──────────────────

async fn handle_messages_streaming(
    state: &AppState,
    adapter: Arc<dyn crate::provider::ProviderAdapter>,
    endpoint: EndpointConfig,
    body: Value,
    request_id: String,
    user_id: String,
    user_name: String,
    api_key_name: String,
    channel_id: String,
    model: String,
    start: Instant,
    client_ip: String,
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    let stream_result = adapter.messages_stream(&endpoint, body).await;

    match stream_result {
        Ok(stream) => {
            // Real-time routing event is broadcast by UsageService.record() when
            // the UsageTrackingStream completes (avoids double-counting).
            let (first_byte_timeout, idle_timeout) = {
                let gw = state.gateway_config.read().unwrap();
                (
                    Duration::from_secs(gw.stream_first_byte_timeout_secs),
                    Duration::from_secs(gw.stream_idle_timeout_secs),
                )
            };
            let stream = IdleTimeoutStream::new(stream, first_byte_timeout, idle_timeout);
            let stream = SseBuffer { inner: stream, buf: String::new(), overflow_error: None };
            let usage_stream = UsageTrackingStream {
                inner: stream,
                resp_buf: String::new(),
                usage: state.usage.clone(),
                request_id,
                user_id,
                user_name,
                api_key_name,
                channel_id,
                model,
                start,
                req_body,
                api_format: "anthropic".to_string(),
                recorded: false,
                client_ip,
                endpoint_id: endpoint.id,
            };

            let body_stream = usage_stream.map(|data| {
                Ok::<_, std::convert::Infallible>(Bytes::from(data))
            });

            Ok(Response::builder()
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("connection", "keep-alive")
                .body(Body::from_stream(body_stream))
                .map_err(|e| GatewayError::Internal(format!("Response build error: {}", e)))?)
        }
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            let (p_tokens, c_tokens, cache_hit) = parse_sse_usage("");
            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id,
                user_name,
                channel_id,
                model,
                prompt_tokens: p_tokens,
                completion_tokens: c_tokens,
                total_tokens: p_tokens + c_tokens,
                cache_hit_input_tokens: cache_hit,
                latency_ms,
                status_code: 502,
                success: false,
                request_body: req_body,
                response_body: None,
                reasoning_body: None,
                api_key_name: Some(api_key_name),
                api_format: "anthropic".to_string(),
                stream: true,
                prompt_price: 0.0,
                completion_price: 0.0,
                cache_read_price: 0.0,
                client_ip: Some(client_ip),
            });
            Err(GatewayError::Upstream(e.0))
        }
    }
}

// ── Non-streaming ─────────────────────────────────────────────────

async fn handle_non_streaming(
    state: &AppState,
    route: &mut RouteTarget,
    body: Value,
    request_id: String,
    user_id: String,
    user_name: String,
    api_key_name: String,
    channel_id: String,
    model: String,
    start: Instant,
    cache_key: Option<String>,
    client_ip: String,
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    let max_retries = {
        let gw = state.gateway_config.read().unwrap();
        gw.max_retries
    };
    let mut retry_count = 0u32;

    let err_msg: String = loop {
        let result = route.adapter.chat_complete(&route.endpoint, body.clone()).await;

        match result {
            Ok(mut resp) => {
                normalize_reasoning_inner(&mut resp);

                let prompt_tokens = resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
                let completion_tokens = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0);
                let cache_hit = resp["usage"]["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);

                let reasoning = resp.get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("reasoning_content"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                let latency_ms = start.elapsed().as_millis() as u64;
                state.usage.record_with_endpoint(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id,
                    user_id: user_id.clone(),
                    user_name,
                    channel_id,

                    model,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens + completion_tokens,
                    cache_hit_input_tokens: cache_hit,
                    latency_ms,
                    status_code: 200,
                    success: true,
                    request_body: req_body.clone(),
                    response_body: serde_json::to_string(&resp).ok(),
                    reasoning_body: reasoning,
                    api_key_name: Some(api_key_name),
                    api_format: "openai".to_string(),
                    stream: false,
                    prompt_price: 0.0,
                    completion_price: 0.0,
                    cache_read_price: 0.0,
                    client_ip: Some(client_ip.clone()),
                }, route.endpoint.id);

                // Cache the response for non-streaming requests
                if let Some(ref ck) = cache_key {
                    if let Ok(body_str) = serde_json::to_string(&resp) {
                        let ttl = state.gateway_config.read().unwrap().cache_ttl_secs;
                        let _ = state.cache.set(&user_id, ck, &body_str, ttl).await;
                    }
                }

                let mut resp = Json(resp).into_response();
                resp.headers_mut().insert("x-cache", HeaderValue::from_static("MISS"));
                return Ok(resp);
            }
            Err(e) if e.kind() == ErrorKind::ConnectFailed => {
                // Connect failure: try next endpoint without consuming
                // retry budget or recording on the circuit breaker.
                if !route.retry_next() {
                    break e.0;
                }
                continue;
            }
            Err(e) if is_retryable_error(&e) => {
                if retry_count >= max_retries {
                    break e.0;
                }
                retry_count += 1;
                if !route.retry_next() {
                    break e.0;
                }
            }
            Err(e) => {
                // Non-retryable (4xx etc.) — don't record failure on the breaker,
                // return immediately without retrying.
                let latency_ms = start.elapsed().as_millis() as u64;
                state.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: request_id.clone(),
                    user_id,
                    user_name,
                    channel_id,
                    model,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cache_hit_input_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: None,
                    reasoning_body: None,
                    api_key_name: None,
                    api_format: "openai".to_string(),
                    stream: false,
                    prompt_price: 0.0,
                    completion_price: 0.0,
                    cache_read_price: 0.0,
                    client_ip: Some(client_ip.clone()),
                });
                tracing::error!(request_id = %request_id, endpoint = %route.endpoint.url, error = %e.0, "Upstream request failed");
                return Err(GatewayError::Upstream(e.0));
            }
        }
    };

    // All retry attempts exhausted without success
    let latency_ms = start.elapsed().as_millis() as u64;
    state.usage.record(UsageRecord {
        timestamp: Utc::now().to_rfc3339(),
        request_id,
        user_id,
        user_name,
        channel_id,
        model,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cache_hit_input_tokens: 0,
        latency_ms,
        status_code: 502,
        success: false,
        request_body: req_body,
        response_body: None,
        reasoning_body: None,
        api_key_name: None,
        api_format: "openai".to_string(),
        stream: false,
        prompt_price: 0.0,
        completion_price: 0.0,
        cache_read_price: 0.0,
        client_ip: Some(client_ip),
    });
    Err(GatewayError::Upstream(err_msg))
}

// ── Messages non-streaming (Anthropic-native format) ──────────────

async fn handle_messages_non_streaming(
    state: &AppState,
    route: &mut RouteTarget,
    body: Value,
    request_id: String,
    user_id: String,
    user_name: String,
    api_key_name: String,
    channel_id: String,
    model: String,
    start: Instant,
    client_ip: String,
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    let max_retries = {
        let gw = state.gateway_config.read().unwrap();
        gw.max_retries
    };
    let mut retry_count = 0u32;

    let err_msg: String = loop {
        let result = route.adapter.messages(&route.endpoint, body.clone()).await;

        match result {
            Ok(resp) => {

                let prompt_tokens = resp["usage"]["input_tokens"].as_u64().unwrap_or(0);
                let completion_tokens = resp["usage"]["output_tokens"].as_u64().unwrap_or(0);
                let cache_hit = resp["usage"]["cache_read_input_tokens"].as_u64().unwrap_or(0);

                let reasoning = resp.get("content")
                    .and_then(|c| c.as_array())
                    .and_then(|blocks| {
                        blocks.iter().find_map(|b| {
                            if b["type"] == "thinking" {
                                b["thinking"].as_str()
                            } else if b["type"] == "redacted_thinking" {
                                b["data"].as_str()
                            } else {
                                None
                            }
                        })
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                    });

                let latency_ms = start.elapsed().as_millis() as u64;
                state.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id,
                    user_id,
                    user_name,
                    channel_id,
                    model,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens + completion_tokens,
                    cache_hit_input_tokens: cache_hit,
                    latency_ms,
                    status_code: 200,
                    success: true,
                    request_body: req_body,
                    response_body: serde_json::to_string(&resp).ok(),
                    reasoning_body: reasoning,
                    api_key_name: Some(api_key_name),
                    api_format: "anthropic".to_string(),
                    stream: false,
                    prompt_price: 0.0,
                    completion_price: 0.0,
                    cache_read_price: 0.0,
                    client_ip: Some(client_ip.clone()),
                });

                return Ok(Json(resp).into_response());
            }
            Err(e) if e.kind() == ErrorKind::ConnectFailed => {
                if !route.retry_next() {
                    break e.0;
                }
                continue;
            }
            Err(e) if is_retryable_error(&e) => {
                if retry_count >= max_retries {
                    break e.0;
                }
                retry_count += 1;
                if !route.retry_next() {
                    break e.0;
                }
            }
            Err(e) => {
                let latency_ms = start.elapsed().as_millis() as u64;
                state.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: request_id.clone(),
                    user_id,
                    user_name,
                    channel_id,
                    model,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cache_hit_input_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: None,
                    reasoning_body: None,
                    api_key_name: None,
                    api_format: "anthropic".to_string(),
                    stream: false,
                    prompt_price: 0.0,
                    completion_price: 0.0,
                    cache_read_price: 0.0,
                    client_ip: Some(client_ip.clone()),
                });
                tracing::error!(request_id = %request_id, endpoint = %route.endpoint.url, error = %e.0, "Messages upstream request failed");
                return Err(GatewayError::Upstream(e.0));
            }
        }
    };

    let latency_ms = start.elapsed().as_millis() as u64;
    state.usage.record(UsageRecord {
        timestamp: Utc::now().to_rfc3339(),
        request_id,
        user_id,
        user_name,
        channel_id,
        model,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cache_hit_input_tokens: 0,
        latency_ms,
        status_code: 502,
        success: false,
        request_body: req_body,
        response_body: None,
        reasoning_body: None,
        api_key_name: None,
        api_format: "anthropic".to_string(),
        stream: false,
        prompt_price: 0.0,
        completion_price: 0.0,
        cache_read_price: 0.0,
        client_ip: Some(client_ip),
    });
    Err(GatewayError::Upstream(err_msg))
}

// ── Handlers ──────────────────────────────────────────────────────

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();

    let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);
    let content_len = headers.get("content-length").and_then(|v| v.to_str().ok()).unwrap_or("unknown");

    let user = state.auth.authenticate(&headers)?;
    let model = trim_model(&mut body)?;

    tracing::info!(request_id, user = %user.user_id, model = %model, body_size = %body_size, content_length = %content_len, "Incoming request");

    if let Some(ref allowed) = user.allowed_models {
        if !allowed.contains(&model) {
            return Err(GatewayError::Auth(format!("Model '{}' not allowed for this API key", model)));
        }
    }

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm)?;
        state.rate_limiter.check_tpm(&user.user_id, tpm, estimate_tokens(&body))?;
    }

    // ── Concurrency cap per user (bounds TOCTOU between gate check and deduction) ──
    let _permit = state.concurrency.try_acquire(&user.user_id, user.concurrency_limit).await
        .map_err(|_| GatewayError::RateLimit("Too many concurrent requests".into()))?;

    // ── Wallet balance check (Redis gate_status → local cache → SQLite) ──
    if state.gateway_config.read().unwrap().billing_enabled {
        check_wallet_balance(&*state, &user.user_id).await?;
    }

    let (channel_id, upstream_model) = state.routing.route(&user.user_id, &model).await?;
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }
    let mut route = resolve_route(&state, &channel_id)?;
    let is_streaming = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let client_ip = extract_client_ip(&headers, addr);

    tracing::info!(request_id, channel = %channel_id, endpoint = %route.endpoint.url, "Routing resolved");

    // ── Content filter check (request body) ──
    let content_filter_enabled = state.db.get_setting("content_moderation_enabled").await
        .ok().flatten()
        .map(|v| v != "false")
        .unwrap_or(false);
    if content_filter_enabled {
        let body_str = serde_json::to_string(&body).unwrap_or_default();
        match state.content_filter.check_request(&body_str, Some(&channel_id)) {
        crate::service::moderation::FilterOutcome::Blocked(rule_name) => {
            tracing::warn!(request_id, rule = %rule_name, "Request blocked by content filter");
            return Err(GatewayError::BadRequest(format!(
                "Request blocked by content filter rule: {}",
                rule_name
            )));
        }
        crate::service::moderation::FilterOutcome::Masked(masked) => {
            if let Ok(v) = serde_json::from_str(&masked) {
                body = v;
                tracing::info!(request_id, "Request body masked by content filter");
            }
        }
        crate::service::moderation::FilterOutcome::Pass => {}
        }
    }

    // ── Cache check (non-streaming only) ──
    let cache_key = if !is_streaming {
        let raw_key = format!(
            "{}:{}",
            model,
            serde_json::to_string(&body).unwrap_or_default()
        );
        let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
        match state.cache.get(&user.user_id, &hash).await {
            Ok(Some(cached)) => {
                tracing::info!(request_id, "Cache HIT for model {}", model);
                if let Ok(val) = serde_json::from_str::<Value>(&cached) {
                    let mut resp = Json(val).into_response();
                    resp.headers_mut()
                        .insert("x-cache", HeaderValue::from_static("HIT"));
                    return Ok(resp);
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(request_id, "Cache GET error: {}", e),
        }
        Some(hash)
    } else {
        None
    };

    let handler_timeout = Duration::from_secs(
        state.gateway_config.read().unwrap().handler_timeout_secs,
    );
    let state_clone = state.clone();
    let rid = request_id.clone();

    let client_ip_clone = client_ip.clone();
    let result = tokio::time::timeout(handler_timeout, async move {
        if is_streaming {
            handle_streaming(
                &state_clone, route.adapter, route.endpoint, body,
                request_id, user.user_id, user.user_name, user.api_key_name, route.channel_id, model, start, client_ip,
            )
            .await
        } else {
            handle_non_streaming(
                &state_clone, &mut route, body,
                request_id, user.user_id, user.user_name, user.api_key_name, channel_id, model, start, cache_key, client_ip_clone,
            )
            .await
        }
    })
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => {
            tracing::error!(rid, handler_timeout_s = handler_timeout.as_secs(), "Chat completions handler timed out");
            Err(GatewayError::Upstream("Request timed out".into()))
        }
    }
}

pub async fn messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();

    let body_size = serde_json::to_string(&body).map(|s| s.len()).unwrap_or(0);

    let user = state.auth.authenticate(&headers)?;
    let model = trim_model(&mut body)?;

    tracing::info!(request_id, user = %user.user_id, model = %model, body_size = %body_size, "Incoming messages request");

    if let Some(ref allowed) = user.allowed_models {
        if !allowed.contains(&model) {
            return Err(GatewayError::Auth(format!("Model '{}' not allowed for this API key", model)));
        }
    }

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm)?;
        state.rate_limiter.check_tpm(&user.user_id, tpm, estimate_tokens_anthropic(&body))?;
    }

    // ── Concurrency cap per user (bounds TOCTOU between gate check and deduction) ──
    let _permit = state.concurrency.try_acquire(&user.user_id, user.concurrency_limit).await
        .map_err(|_| GatewayError::RateLimit("Too many concurrent requests".into()))?;

    // ── Wallet balance check (Redis gate_status → local cache → SQLite) ──
    if state.gateway_config.read().unwrap().billing_enabled {
        check_wallet_balance(&*state, &user.user_id).await?;
    }

    let (channel_id, upstream_model) = state.routing.route(&user.user_id, &model).await?;
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }
    // Normalize Claude-Code-style inline system messages to the Anthropic
    // top-level "system" field.  SGLang's /v1/messages rejects role=system
    // in the messages array (only "user"/"assistant" are allowed).
    normalize_messages_body(&mut body);
    let mut route = resolve_route(&state, &channel_id)?;

    // If the resolved channel has anthropic_compat enabled (OpenAI provider
    // accepting Anthropic-format requests), wrap the adapter so that
    // messages()/messages_stream() transparently convert between formats.
    if let Some(ref ch) = state.routing.get_channel(&channel_id) {
        if ch.anthropic_compat && ch.provider == "openai" {
            route.adapter = Arc::new(
                crate::provider::anthropic_compat::AnthropicCompatAdapter::new(route.adapter.clone()),
            );
        }
    }

    let is_streaming = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let client_ip = extract_client_ip(&headers, addr);

    tracing::info!(request_id, channel = %channel_id, endpoint = %route.endpoint.url, "Messages routing resolved");

    // ── Content filter check (request body) ──
    let content_filter_enabled = state.db.get_setting("content_moderation_enabled").await
        .ok().flatten()
        .map(|v| v != "false")
        .unwrap_or(false);
    if content_filter_enabled {
        let body_str = serde_json::to_string(&body).unwrap_or_default();
        match state.content_filter.check_request(&body_str, Some(&channel_id)) {
        crate::service::moderation::FilterOutcome::Blocked(rule_name) => {
            tracing::warn!(request_id, rule = %rule_name, "Messages request blocked by content filter");
            return Err(GatewayError::BadRequest(format!(
                "Request blocked by content filter rule: {}",
                rule_name
            )));
        }
        crate::service::moderation::FilterOutcome::Masked(masked) => {
            if let Ok(v) = serde_json::from_str(&masked) {
                body = v;
                tracing::info!(request_id, "Messages request body masked by content filter");
            }
        }
        crate::service::moderation::FilterOutcome::Pass => {}
        }
    }

    let handler_timeout = Duration::from_secs(
        state.gateway_config.read().unwrap().handler_timeout_secs,
    );
    let state_clone = state.clone();
    let rid = request_id.clone();
    let client_ip_clone = client_ip.clone();

    let result = tokio::time::timeout(handler_timeout, async move {
        if is_streaming {
            handle_messages_streaming(
                &state_clone, route.adapter, route.endpoint, body,
                request_id, user.user_id, user.user_name, user.api_key_name, route.channel_id, model, start, client_ip,
            )
            .await
        } else {
            handle_messages_non_streaming(
                &state_clone, &mut route, body,
                request_id, user.user_id, user.user_name, user.api_key_name, channel_id, model, start, client_ip_clone,
            )
            .await
        }
    })
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => {
            tracing::error!(rid, handler_timeout_s = handler_timeout.as_secs(), "Messages handler timed out");
            Err(GatewayError::Upstream("Request timed out".into()))
        }
    }
}

// ── Relay ─────────────────────────────────────────────────────────

async fn relay_to_upstream(
    state: &AppState,
    headers: &HeaderMap,
    mut body: Value,
    upstream_path: &str,
    request_id: String,
    start: Instant,
    client_ip: String,
) -> Result<Response, GatewayError> {
    let user = state.auth.authenticate(headers)?;
    let model = trim_model(&mut body)?;

    if let Some(ref allowed) = user.allowed_models {
        if !allowed.contains(&model) {
            return Err(GatewayError::Auth(format!("Model '{}' not allowed for this API key", model)));
        }
    }

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm)?;
        state.rate_limiter.check_tpm(&user.user_id, tpm, estimate_tokens(&body))?;
    }

    // ── Concurrency cap per user (bounds TOCTOU between gate check and deduction) ──
    let _permit = state.concurrency.try_acquire(&user.user_id, user.concurrency_limit).await
        .map_err(|_| GatewayError::RateLimit("Too many concurrent requests".into()))?;

    // ── Wallet balance check (Redis gate_status → local cache → SQLite) ──
    if state.gateway_config.read().unwrap().billing_enabled {
        check_wallet_balance(state, &user.user_id).await?;
    }

    let (channel_id, upstream_model) = state.routing.route(&user.user_id, &model).await?;
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }
    let mut route = resolve_route(state, &channel_id)?;

    // ── Content filter check (request body) ──
    let content_filter_enabled = state.db.get_setting("content_moderation_enabled").await
        .ok().flatten()
        .map(|v| v != "false")
        .unwrap_or(false);
    if content_filter_enabled {
        let body_str = serde_json::to_string(&body).unwrap_or_default();
        match state.content_filter.check_request(&body_str, Some(&channel_id)) {
        crate::service::moderation::FilterOutcome::Blocked(rule_name) => {
            tracing::warn!(request_id, rule = %rule_name, "Relay request blocked by content filter");
            return Err(GatewayError::BadRequest(format!(
                "Request blocked by content filter rule: {}",
                rule_name
            )));
        }
        crate::service::moderation::FilterOutcome::Masked(masked) => {
            if let Ok(v) = serde_json::from_str(&masked) {
                body = v;
            }
        }
        crate::service::moderation::FilterOutcome::Pass => {}
        }
    }

    let req_body = serde_json::to_string(&body).ok();
        let max_retries = {
        let gw = state.gateway_config.read().unwrap();
        gw.max_retries
    };
    let mut retry_count = 0u32;

    let err_msg: String = loop {
        let result = route.adapter.relay(&route.endpoint, upstream_path, body.clone()).await;

        match result {
            Ok(mut resp) => {
                normalize_reasoning_inner(&mut resp);
                let prompt_tokens = resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
                let completion_tokens = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0);
                let cache_hit = resp["usage"]["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);

                let reasoning = resp.get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("reasoning_content"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                let latency_ms = start.elapsed().as_millis() as u64;
                state.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id,
                    user_id: user.user_id,
                    user_name: user.user_name,
                    channel_id: route.channel_id,
                    model,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens + completion_tokens,
                    cache_hit_input_tokens: cache_hit,
                    latency_ms,
                    status_code: 200,
                    success: true,
                    request_body: req_body,
                    response_body: serde_json::to_string(&resp).ok(),
                    reasoning_body: reasoning,
                    api_key_name: Some(user.api_key_name.clone()),
                    api_format: "relay".to_string(),
                    stream: false,
                    prompt_price: 0.0,
                    completion_price: 0.0,
                    cache_read_price: 0.0,
                    client_ip: Some(client_ip.clone()),
                });

                return Ok(Json(resp).into_response());
            }
            Err(e) if e.kind() == ErrorKind::ConnectFailed => {
                if !route.retry_next() {
                    break e.0;
                }
                continue;
            }
            Err(e) if is_retryable_error(&e) => {
                if retry_count >= max_retries {
                    break e.0;
                }
                retry_count += 1;
                if !route.retry_next() {
                    break e.0;
                }
            }
            Err(e) => {
                let latency_ms = start.elapsed().as_millis() as u64;
                state.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id,
                    user_id: user.user_id,
                    user_name: user.user_name,
                    channel_id: route.channel_id,
                    model,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cache_hit_input_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: None,
                    reasoning_body: None,
                    api_key_name: Some(user.api_key_name.clone()),
                    api_format: "relay".to_string(),
                    stream: false,
                    prompt_price: 0.0,
                    completion_price: 0.0,
                    cache_read_price: 0.0,
                    client_ip: Some(client_ip.clone()),
                });
                return Err(GatewayError::from(e));
            }
        }
    };

    let latency_ms = start.elapsed().as_millis() as u64;
    state.usage.record(UsageRecord {
        timestamp: Utc::now().to_rfc3339(),
        request_id,
        user_id: user.user_id,
        user_name: user.user_name,
        channel_id: route.channel_id,
        model,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cache_hit_input_tokens: 0,
        latency_ms,
        status_code: 502,
        success: false,
        request_body: req_body,
        response_body: None,
        reasoning_body: None,
        api_key_name: Some(user.api_key_name),
        api_format: "relay".to_string(),
        stream: false,
        prompt_price: 0.0,
        completion_price: 0.0,
        cache_read_price: 0.0,
        client_ip: Some(client_ip),
    });
    Err(GatewayError::Upstream(err_msg))
}

pub async fn completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/v1/completions",
        Uuid::new_v4().to_string(), Instant::now(), String::new()).await
}

pub async fn embeddings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/v1/embeddings",
        Uuid::new_v4().to_string(), Instant::now(), String::new()).await
}

pub async fn batches(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/v1/messages/batches",
        Uuid::new_v4().to_string(), Instant::now(), String::new()).await
}

pub async fn tokenize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/tokenize",
        Uuid::new_v4().to_string(), Instant::now(), String::new()).await
}

pub async fn detokenize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/detokenize",
        Uuid::new_v4().to_string(), Instant::now(), String::new()).await
}

// ── Other ─────────────────────────────────────────────────────────

pub async fn health() -> Json<Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, GatewayError> {
    let user = state.auth.authenticate(&headers)?;
    let subs = state.db.list_subscriptions(&user.user_id).await.unwrap_or_default();
    let subscribed: std::collections::HashSet<String> = subs.iter().map(|m| m.id.clone()).collect();

    let mut models: Vec<Value> = state.routing.list_display_models()
        .into_iter()
        .filter(|m| subscribed.contains(m["upstream_id"].as_str().unwrap_or("")))
        .collect();

    let limit: usize = params.get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
        .min(1000);

    let after_id = params.get("after_id");
    let before_id = params.get("before_id");

    if let Some(after) = after_id {
        if let Some(pos) = models.iter().position(|m| m["id"].as_str() == Some(after)) {
            models = models.split_off(pos + 1);
        }
    }
    if let Some(before) = before_id {
        if let Some(pos) = models.iter().position(|m| m["id"].as_str() == Some(before)) {
            models.truncate(pos);
        }
    }

    let has_more = models.len() > limit;
    models.truncate(limit);

    let first_id = models.first().and_then(|m| m["id"].as_str().map(|s| s.to_string()));
    let last_id = models.last().and_then(|m| m["id"].as_str().map(|s| s.to_string()));

    Ok(Json(serde_json::json!({
        "data": models,
        "first_id": first_id,
        "has_more": has_more,
        "last_id": last_id,
    })))
}

// ── Token estimators ──────────────────────────────────────────────

fn estimate_tokens(body: &Value) -> u64 {
    let total_chars: usize = body["messages"]
        .as_array()
        .map(|msgs| {
            msgs.iter()
                .filter_map(|m| m["content"].as_str())
                .map(|s| s.len())
                .sum()
        })
        .unwrap_or(0);
    (total_chars / 4) as u64
}

fn estimate_tokens_anthropic(body: &Value) -> u64 {
    let total_chars: usize = body["messages"]
        .as_array()
        .map(|msgs| {
            msgs.iter()
                .map(|m| match &m["content"] {
                    Value::String(s) => s.len(),
                    Value::Array(arr) => arr.iter()
                        .filter_map(|c| c["text"].as_str())
                        .map(|s| s.len())
                        .sum(),
                    _ => 0,
                })
                .sum()
        })
        .unwrap_or(0);
    (total_chars / 4) as u64
}
