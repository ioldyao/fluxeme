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
fn parse_sse_usage(data: &str) -> (u64, u64, u64, u64) {
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
    original_model: String,
    start: Instant,
    req_body: Option<String>,
    api_format: String,
    recorded: bool,
    client_ip: String,
    endpoint_id: Option<i64>,
    endpoint_url: Option<String>,
    /// Team scope of the request. None = personal.
    team_id: Option<String>,
    upstream_started_at: Instant,
    ttft_ms: Option<u64>,
    /// Circuit-breaker feedback for the streaming request: record_success
    /// when the stream completes cleanly. Client disconnects / mid-stream
    /// drops are not fed into the breaker — they aren't upstream failures.
    balancer: Option<Arc<LoadBalancer>>,
    endpoint_idx: usize,
    reservation: Option<crate::service::token_reservation::ReservationFinalizer>,
}

impl<S: Stream<Item = String> + Unpin> Stream for UsageTrackingStream<S> {
    type Item = String;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(data)) => {
                if self.ttft_ms.is_none() && !data.is_empty() {
                    self.ttft_ms = Some(self.upstream_started_at.elapsed().as_millis() as u64);
                    self.usage.mark_first_byte(&self.request_id);
                }
                self.resp_buf.push_str(&data);
                Poll::Ready(Some(data))
            }
            Poll::Ready(None) => {
                // A clean EOF is only successful when the stream was not
                // terminated by the timeout wrapper. Timeout/overflow paths
                // are represented by Drop and therefore partial-settle.
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

        // Live-traffic breaker feedback: a clean stream completion means the
        // endpoint is healthy. Mid-stream drops (client disconnect) are not
        // recorded — see the `balancer` field docs.
        if completed {
            if let Some(b) = &self.balancer {
                b.as_health_aware().record_success(self.endpoint_idx);
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
                reservation.release_partial(
                    p_tokens,
                    c_tokens,
                    cache_hit,
                    cache_write,
                    "client disconnected",
                );
            }
        }

        self.usage.record_with_endpoint(
            UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
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
struct IdleTimeoutStream {
    inner: Pin<Box<dyn Stream<Item = String> + Send>>,
    #[allow(dead_code)]
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

