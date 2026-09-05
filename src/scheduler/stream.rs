// ── Streaming helpers (moved from server/handlers/stream.rs) ──

use std::pin::Pin;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::Future;
use futures::Stream;
use serde_json::Value;

use crate::domain::usage::UsageRecord;
use crate::service::endpoint_pool::ModelEndpointRuntime;
use rust_decimal::Decimal;

/// Extract reasoning and output content from raw SSE data.
/// Returns (reasoning, content) extracted from delta chunks.
pub(crate) fn extract_sse_content(data: &str) -> (String, String) {
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
            if let Some(delta) = val
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"))
            {
                if let Some(text) = delta
                    .get("reasoning") // normalized name (from normalize_sse_reasoning)
                    .or_else(|| delta.get("reasoning_content"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    reasoning.push_str(text);
                }
                if let Some(text) = delta
                    .get("content")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                {
                    content.push_str(text);
                }
            }
            // Anthropic format: content_block_delta delta.{thinking, text}
            if val.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                if let Some(delta) = val.get("delta") {
                    if delta.get("type").and_then(|t| t.as_str()) == Some("thinking_delta") {
                        if let Some(text) = delta
                            .get("thinking")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            reasoning.push_str(text);
                        }
                    }
                    if delta.get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                        if let Some(text) = delta
                            .get("text")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
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
pub(crate) fn parse_sse_usage(data: &str) -> (u64, u64, u64, u64) {
    let mut p_tokens = 0u64;
    let mut c_tokens = 0u64;
    let mut cache_hit = 0u64;
    let mut cache_write = 0u64;
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "data: [DONE]" || trimmed.starts_with("event: ") {
            continue;
        }
        let json_str = trimmed.strip_prefix("data: ").unwrap_or(trimmed);
        if let Ok(val) = serde_json::from_str::<Value>(json_str) {
            // OpenAI format: {usage: {prompt_tokens, completion_tokens, prompt_tokens_details: {cached_tokens}}}
            if let Some(usage) = val.get("usage") {
                if usage.is_null() {
                    continue;
                }
                let p = usage
                    .get("prompt_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let c = usage
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if p > p_tokens {
                    p_tokens = p;
                }
                if c > c_tokens {
                    c_tokens = c;
                }
                if let Some(details) = usage.get("prompt_tokens_details") {
                    if let Some(cached) = details.get("cached_tokens").and_then(|v| v.as_u64()) {
                        if cached > cache_hit {
                            cache_hit = cached;
                        }
                    }
                    if let Some(write) = details.get("cache_write_tokens").and_then(|v| v.as_u64())
                    {
                        if write > cache_write {
                            cache_write = write;
                        }
                    }
                }
                // OpenAI prompt_tokens includes cached tokens; subtract them
                // so prompt_tokens represents the billable input (matching
                // new-api semantics).
                if cache_hit > 0 || cache_write > 0 {
                    p_tokens = p_tokens.saturating_sub(cache_hit + cache_write);
                }
            }
            // Anthropic message_start: {type: "message_start", message: {usage: {input_tokens, output_tokens, cache_read_input_tokens}}}
            if val.get("type").and_then(|t| t.as_str()) == Some("message_start") {
                if let Some(msg) = val.get("message") {
                    if let Some(usage) = msg.get("usage") {
                        if let Some(p) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                            if p > p_tokens {
                                p_tokens = p;
                            }
                        }
                        if let Some(c) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                            if c > c_tokens {
                                c_tokens = c;
                            }
                        }
                        if let Some(cached) = usage
                            .get("cache_read_input_tokens")
                            .and_then(|v| v.as_u64())
                        {
                            if cached > cache_hit {
                                cache_hit = cached;
                            }
                        }
                        if let Some(create) = usage
                            .get("cache_creation_input_tokens")
                            .and_then(|v| v.as_u64())
                        {
                            if create > cache_write {
                                cache_write = create;
                            }
                        }
                    }
                }
            }
            // Anthropic message_delta: {type: "message_delta", usage: {output_tokens, ...}}
            // (converted anthropic_compat streams also carry
            //  cache_read_input_tokens here — message_start has none)
            if val.get("type").and_then(|t| t.as_str()) == Some("message_delta") {
                if let Some(usage) = val.get("usage") {
                    if let Some(p) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                        if p > p_tokens {
                            p_tokens = p;
                        }
                    }
                    if let Some(c) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                        if c > c_tokens {
                            c_tokens = c;
                        }
                    }
                    if let Some(cached) = usage
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64())
                    {
                        if cached > cache_hit {
                            cache_hit = cached;
                        }
                    }
                    if let Some(create) = usage
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64())
                    {
                        if create > cache_write {
                            cache_write = create;
                        }
                    }
                }
            }
        }
    }
    (p_tokens, c_tokens, cache_hit, cache_write)
}

// ── Stream termination classification ─────────────────────────────

/// Shared termination flag threaded from the terminating stream layers into
/// `UsageTrackingStream`.
///
/// Values:
/// - `0` = clean EOF (`StreamTermination::Clean`) — the inner stream ended normally.
/// - `1` = `IdleTimeoutStream` fired and emitted its error SSE event.
/// - `2` = `SseBuffer` hit the buffer cap and emitted its overflow error event.
///
/// Any non-clean value means `UsageTrackingStream`'s EOF was synthetic and must
/// be finalised as a failure (not a success); when the stream is dropped early
/// on a non-clean flag, that failure outranks the default cancelled/499.
///
/// A small enum (`StreamTermination`) provides the ergonomic classification
/// while the shared flag stays an `Arc<AtomicU8>` so `IdleTimeoutStream` and
/// `SseBuffer` can poke it without trait plumbing or `Unpin` complications.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamTermination {
    /// Inner stream reached a natural end.
    Clean,
    /// `IdleTimeoutStream`'s idle timeout fired.
    IdleTimeout,
    /// `SseBuffer` hit the 1 MB cap.
    BufferOverflow,
}

impl StreamTermination {
    pub(crate) const fn as_u8(self) -> u8 {
        match self {
            StreamTermination::Clean => 0,
            StreamTermination::IdleTimeout => 1,
            StreamTermination::BufferOverflow => 2,
        }
    }

    pub(crate) fn from_u8(value: u8) -> StreamTermination {
        match value {
            1 => StreamTermination::IdleTimeout,
            2 => StreamTermination::BufferOverflow,
            _ => StreamTermination::Clean,
        }
    }
}

/// Shared `Arc<AtomicU8>` termination flag. Cloning hands out another handle
/// to the same flag.
#[derive(Clone, Debug, Default)]
pub(crate) struct StreamTerminationFlag(Arc<AtomicU8>);

impl StreamTerminationFlag {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicU8::new(0)))
    }

    pub(crate) fn set(&self, termination: StreamTermination) {
        // Precedence-safe: only ever raise to non-clean.
        self.0.store(termination.as_u8(), Ordering::SeqCst);
    }

    pub(crate) fn get(&self) -> StreamTermination {
        StreamTermination::from_u8(self.0.load(Ordering::SeqCst))
    }
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
/// - Buffer capped at 1 MB — beyond that an error event is emitted and the
///   stream is closed.
/// - At EOF any leftover data that doesn't form valid JSON is silently
///   dropped (with a warning) instead of forwarded to the client.
pub(crate) struct SseBuffer<S> {
    pub(crate) inner: S,
    pub(crate) buf: String,
    pub(crate) overflow_error: Option<String>,
    pub(crate) terminated: bool,
    pub(crate) termination: StreamTerminationFlag,
}

impl<S: Stream<Item = String> + Unpin> Stream for SseBuffer<S> {
    type Item = String;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 1) If the overflow error event was queued but not yet delivered
        //    (the overflow poll returned a buffered complete event first),
        //    deliver it now.
        if let Some(err) = self.overflow_error.take() {
            return Poll::Ready(Some(err));
        }
        // The overflow error was already delivered: the following EOF is
        // synthetic and classified as overflow.
        if self.terminated {
            return Poll::Ready(None);
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
                    // 2) Buffer-overflow protection
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
                        // Classify the upcoming (synthetic) EOF as overflow so
                        // UsageTrackingStream does not record a false success.
                        self.termination.set(StreamTermination::BufferOverflow);
                        self.terminated = true;
                        // Emit the error event now; the following poll returns
                        // the synthetic EOF classified above.
                        return Poll::Ready(Some(self.overflow_error.take().unwrap()));
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

pub(crate) struct UsageTrackingStream<S> {
    pub(crate) inner: S,
    pub(crate) resp_buf: String,
    pub(crate) usage: crate::service::UsageService,
    pub(crate) request_id: String,
    pub(crate) user_id: String,
    pub(crate) user_name: String,
    pub(crate) api_key_name: String,
    pub(crate) channel_id: String,
    pub(crate) model: String,
    pub(crate) original_model: String,
    pub(crate) start: Instant,
    pub(crate) req_body: Option<String>,
    pub(crate) api_format: String,
    pub(crate) recorded: bool,
    pub(crate) client_ip: String,
    pub(crate) endpoint_id: Option<i64>,
    pub(crate) endpoint_url: Option<String>,
    /// Team scope of the request. None = personal.
    pub(crate) team_id: Option<String>,
    pub(crate) upstream_started_at: Instant,
    pub(crate) ttft_ms: Option<u64>,
    /// Circuit-breaker feedback for the streaming request: record_success
    /// when the stream completes cleanly. Client disconnects / mid-stream
    /// drops are not fed into the breaker — they aren't upstream failures.
    pub(crate) runtime: Option<Arc<ModelEndpointRuntime>>,
    pub(crate) endpoint_idx: usize,
    pub(crate) reservation: Option<crate::service::token_reservation::ReservationFinalizer>,
    /// Exactly-once request lifecycle finalizer. Finalized at EOF (succeeded
    /// with accumulated tokens) or on premature drop (cancelled/499); if the
    /// stream is abandoned before reaching this point, `RequestLifecycle`'s
    /// own Drop emits the unfinalized fallback event.
    pub(crate) lifecycle: Option<Arc<crate::observability::lifecycle::RequestLifecycle>>,
    /// Shared termination classification. `IdleTimeoutStream` and `SseBuffer`
    /// set a non-clean value before yielding their synthetic EOF so this layer
    /// finalizes the failure instead of a false success.
    pub(crate) termination: StreamTerminationFlag,
}

impl<S: Stream<Item = String> + Unpin> Stream for UsageTrackingStream<S> {
    type Item = String;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(data)) => {
                if self.ttft_ms.is_none() && !data.is_empty() {
                    self.ttft_ms = Some(self.upstream_started_at.elapsed().as_millis() as u64);
                    if let Some(lifecycle) = self.lifecycle.as_ref() {
                        lifecycle.set_ttft(self.ttft_ms.unwrap_or(0));
                    }
                    self.usage.mark_first_byte(&self.request_id);
                }
                self.resp_buf.push_str(&data);
                Poll::Ready(Some(data))
            }
            Poll::Ready(None) => {
                // Classify EOF: a clean EOF is a success; an EOF produced by the
                // idle-timeout or buffer-overflow layers is a synthetic failure.
                match self.termination.get() {
                    StreamTermination::Clean => self.record_usage(true, None),
                    StreamTermination::IdleTimeout => {
                        self.record_usage(false, Some(StreamTermination::IdleTimeout))
                    }
                    StreamTermination::BufferOverflow => {
                        self.record_usage(false, Some(StreamTermination::BufferOverflow))
                    }
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> Drop for UsageTrackingStream<S> {
    fn drop(&mut self) {
        if self.recorded {
            return;
        }
        // Precedence: a non-clean termination flag marks an upstream-side
        // failure (idle timeout / buffer overflow) that outranks the default
        // cancelled/499 client-disconnect classification.
        match self.termination.get() {
            StreamTermination::Clean => self.record_usage(false, None),
            StreamTermination::IdleTimeout => {
                self.record_usage(false, Some(StreamTermination::IdleTimeout))
            }
            StreamTermination::BufferOverflow => {
                self.record_usage(false, Some(StreamTermination::BufferOverflow))
            }
        }
    }
}

impl<S> UsageTrackingStream<S> {
    fn record_usage(&mut self, completed: bool, termination: Option<StreamTermination>) {
        if self.recorded {
            return;
        }
        self.recorded = true;

        // Live-traffic breaker feedback: a clean stream completion means the
        // endpoint is healthy. Mid-stream drops (client disconnect) are not
        // recorded — see the `balancer` field docs.
        if completed {
            if let Some(runtime) = &self.runtime {
                if let Some(state) = runtime.endpoints.get(self.endpoint_idx) {
                    state.breaker.record_success();
                }
            }
        }

        let latency_ms = self.start.elapsed().as_millis() as u64;
        let (mut p_tokens, mut c_tokens, cache_hit, cache_write) = parse_sse_usage(&self.resp_buf);

        // If no token usage data was in the SSE stream (some upstream
        // providers omit usage in streaming mode), estimate from content
        // length.  Rough: ~4 chars/token for English, ~2 for CJK.
        if p_tokens == 0 && c_tokens == 0 {
            let (reasoning, content) = extract_sse_content(&self.resp_buf);
            let total_content = reasoning.len() + content.len();
            if total_content > 0 {
                if let Some(ref body) = self.req_body {
                    p_tokens = (body.len() / 4).max(1) as u64;
                }
                c_tokens = (total_content / 3).max(1) as u64;
            }
        }

        // Finalize the request lifecycle at stream end:
        // - clean EOF → succeeded with the accumulated tokens;
        // - idle timeout → failed/504 response_stream/stream_idle_timeout;
        // - buffer overflow → failed/502 response_stream/stream_buffer_overflow;
        // - premature end (client disconnect / abort) → cancelled/499.
        if let Some(lifecycle) = self.lifecycle.as_ref() {
            lifecycle.add_tokens(p_tokens, c_tokens, cache_hit, cache_write);
            lifecycle.set_attempts(1, if completed { Some(1) } else { None });
            if completed {
                lifecycle.finalize_success();
            } else {
                match termination {
                    Some(StreamTermination::IdleTimeout) => {
                        lifecycle.finalize_failed(
                            504,
                            crate::observability::lifecycle::RequestError::new(
                                "response_stream",
                                "stream_idle_timeout",
                            ),
                        );
                    }
                    Some(StreamTermination::BufferOverflow) => {
                        lifecycle.finalize_failed(
                            502,
                            crate::observability::lifecycle::RequestError::new(
                                "response_stream",
                                "stream_buffer_overflow",
                            ),
                        );
                    }
                    _ => {
                        lifecycle.mark_client_disconnected();
                        lifecycle.finalize_cancelled(
                            499,
                            crate::observability::lifecycle::RequestError::new(
                                "response_stream",
                                "client_disconnect",
                            ),
                        );
                    }
                }
            }
        }

        if let Some(reservation) = &self.reservation {
            if completed {
                reservation.settle_usage(
                    p_tokens,
                    c_tokens,
                    cache_hit,
                    cache_write,
                    true,
                    "completed",
                );
            } else {
                let reason = match termination {
                    Some(StreamTermination::IdleTimeout) => "stream idle timeout",
                    Some(StreamTermination::BufferOverflow) => "stream buffer overflow",
                    _ => "client disconnected",
                };
                reservation.release_partial(p_tokens, c_tokens, cache_hit, cache_write, reason);
            }
        }

        self.usage.record_with_endpoint(
            UsageRecord {
                timestamp: chrono::Utc::now().to_rfc3339(),
                request_id: self.request_id.clone(),
                user_id: self.user_id.clone(),
                user_name: self.user_name.clone(),
                channel_id: self.channel_id.clone(),
                model: self.model.clone(),
                prompt_tokens: p_tokens,
                completion_tokens: c_tokens,
                total_tokens: p_tokens + cache_hit + c_tokens,
                cache_hit_input_tokens: cache_hit,
                cache_write_tokens: cache_write,
                latency_ms,
                status_code: if completed {
                    200
                } else {
                    match termination {
                        Some(StreamTermination::IdleTimeout) => 504,
                        Some(StreamTermination::BufferOverflow) => 502,
                        _ => 499,
                    }
                },
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
                    let text = if content.is_empty() {
                        reasoning
                    } else {
                        content
                    };
                    Some(if text.len() > 102400 {
                        text.chars().take(102400).collect()
                    } else {
                        text
                    })
                },
                stream: true,
                prompt_price: Decimal::ZERO,
                completion_price: Decimal::ZERO,
                cache_read_price: Decimal::ZERO,
                cache_write_price: Decimal::ZERO,
                client_ip: Some(self.client_ip.clone()),
                endpoint_id: self.endpoint_id,
                endpoint_url: self.endpoint_url.clone(),
                original_model: self.original_model.clone(),
                team_id: self.team_id.clone(),
                ttft_ms: self.ttft_ms,
                account_type: self
                    .team_id
                    .as_ref()
                    .map(|_| "team")
                    .or(Some("user"))
                    .map(String::from),
                billing_group_id: None,
                billing_group_name: None,
                billing_payment_mode: None,
            },
            self.endpoint_id,
        );
    }
}

// ── Idle-timeout stream wrapper ────────────────────────────────────

/// Wraps a stream with an idle timeout. If no data arrives within the
/// timeout window, an error SSE event is emitted and the stream terminates.
///
/// The first timeout waits `first_byte_timeout`; subsequent timeouts use
/// `idle_timeout`.  This lets callers set a generous initial allowance
/// for model "thinking" before tightening the per-chunk expectation.
pub(crate) struct IdleTimeoutStream {
    inner: Pin<Box<dyn Stream<Item = String> + Send>>,
    #[allow(dead_code)]
    first_byte_timeout: Duration,
    idle_timeout: Duration,
    sleep: Pin<Box<tokio::time::Sleep>>,
    has_received_data: bool,
    timed_out: bool,
    pub(crate) termination: StreamTerminationFlag,
}

impl IdleTimeoutStream {
    pub(crate) fn new(
        inner: Pin<Box<dyn Stream<Item = String> + Send>>,
        first_byte_timeout: Duration,
        idle_timeout: Duration,
    ) -> Self {
        Self::with_termination(
            inner,
            first_byte_timeout,
            idle_timeout,
            StreamTerminationFlag::new(),
        )
    }

    pub(crate) fn with_termination(
        inner: Pin<Box<dyn Stream<Item = String> + Send>>,
        first_byte_timeout: Duration,
        idle_timeout: Duration,
        termination: StreamTerminationFlag,
    ) -> Self {
        Self {
            inner,
            first_byte_timeout,
            idle_timeout,
            sleep: Box::pin(tokio::time::sleep(first_byte_timeout)),
            has_received_data: false,
            timed_out: false,
            termination,
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
                this.sleep
                    .as_mut()
                    .reset(tokio::time::Instant::now() + this.idle_timeout);
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
                    // Classify the upcoming EOF as idle-timeout so the
                    // UsageTrackingStream layer finalizes failure, not success.
                    this.termination.set(StreamTermination::IdleTimeout);
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{stream, StreamExt};

    #[test]
    fn termination_flag_classifies_clean_idle_and_overflow_eof() {
        let flag = StreamTerminationFlag::new();
        assert_eq!(flag.get(), StreamTermination::Clean);
        flag.set(StreamTermination::IdleTimeout);
        assert_eq!(flag.get(), StreamTermination::IdleTimeout);
        flag.set(StreamTermination::BufferOverflow);
        assert_eq!(flag.get(), StreamTermination::BufferOverflow);
    }

    #[tokio::test]
    async fn idle_timeout_sets_flag_before_error_and_eof() {
        let flag = StreamTerminationFlag::new();
        let inner = Box::pin(stream::pending::<String>());
        let mut wrapped = IdleTimeoutStream::with_termination(
            inner,
            Duration::from_millis(1),
            Duration::from_millis(1),
            flag.clone(),
        );
        // Let the (real) 1ms idle timer expire before the first poll, so the
        // first `next()` yields the idle-timeout error event deterministically.
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(wrapped.next().await.unwrap().contains("idle_timeout"));
        assert_eq!(flag.get(), StreamTermination::IdleTimeout);
        assert!(wrapped.next().await.is_none());
    }

    #[tokio::test]
    async fn buffer_overflow_sets_flag_before_error_and_eof() {
        let flag = StreamTerminationFlag::new();
        let inner = stream::iter(vec!["x".repeat(MAX_SSE_BUF + 1)]);
        let mut wrapped = SseBuffer {
            inner,
            buf: String::new(),
            overflow_error: None,
            terminated: false,
            termination: flag.clone(),
        };
        assert!(wrapped.next().await.unwrap().contains("buffer_overflow"));
        assert_eq!(flag.get(), StreamTermination::BufferOverflow);
        assert!(wrapped.next().await.is_none());
    }
}
