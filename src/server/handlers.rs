use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use bytes::Bytes;
use chrono::Utc;
use futures::stream::StreamExt;
use futures::Stream;
use serde_json::Value;
use uuid::Uuid;

use crate::balancer::LoadBalancer;
use crate::config::types::EndpointConfig;
use crate::domain::usage::UsageRecord;
use crate::provider::ProviderError;
use crate::server::AppState;

// ── Error type ────────────────────────────────────────────────────

#[derive(Debug)]
pub enum GatewayError {
    Auth(String),
    RateLimit(String),
    Route(String),
    BadRequest(String),
    Upstream(String),
    Internal(String),
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
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Auth(m)
            | Self::RateLimit(m)
            | Self::Route(m)
            | Self::BadRequest(m)
            | Self::Upstream(m)
            | Self::Internal(m) => m,
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

/// Non-standard reasoning field names from various providers to normalize.
const REASONING_ALIASES: &[&str] = &["reasoning", "thinking", "thinking_content"];

/// Max retry attempts across different endpoints (per-endpoint failures are recorded on the breaker).
const MAX_RETRIES: u32 = 2;

/// Whether a `ProviderError` is safe to retry (5xx or network failure).
/// 4xx errors are never retryable — retrying them would be wasted effort.
fn is_retryable_error(e: &ProviderError) -> bool {
    e.0.starts_with("Request failed") || e.0.starts_with("Upstream returned 5")
}

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

struct RouteTarget {
    channel_id: String,
    endpoint: EndpointConfig,
    adapter: Arc<dyn crate::provider::ProviderAdapter>,
    balancer: Arc<LoadBalancer>,
    endpoint_idx: usize,
}

impl RouteTarget {
    /// Try the next available endpoint from the balancer.
    /// Returns `false` if no more endpoints available.
    fn retry_next(&mut self) -> bool {
        if let Some((idx, ep)) = self.balancer.as_health_aware().select() {
            self.endpoint_idx = idx;
            self.endpoint = ep.clone();
            true
        } else {
            false
        }
    }
}

fn resolve_route(state: &AppState, channel_id: &str) -> Result<RouteTarget, GatewayError> {
    let (provider_name, balancer, _endpoints) = state
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
        endpoint_idx: idx,
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
        if trimmed.is_empty() || trimmed == "data: [DONE]" {
            continue;
        }
        let json_str = trimmed.strip_prefix("data: ").unwrap_or(trimmed);
        if let Ok(val) = serde_json::from_str::<Value>(json_str) {
            if let Some(delta) = val.get("choices").and_then(|c| c.get(0)).and_then(|c| c.get("delta")) {
                if let Some(text) = delta.get("reasoning_content").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    reasoning.push_str(text);
                }
                if let Some(text) = delta.get("content").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    content.push_str(text);
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
fn parse_sse_usage(data: &str) -> (u64, u64) {
    let mut p_tokens = 0u64;
    let mut c_tokens = 0u64;
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "data: [DONE]" {
            continue;
        }
        let json_str = trimmed.strip_prefix("data: ").unwrap_or(trimmed);
        if let Ok(val) = serde_json::from_str::<Value>(json_str) {
            if let Some(usage) = val.get("usage") {
                let p = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let c = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                if p > p_tokens { p_tokens = p; }
                if c > c_tokens { c_tokens = c; }
            }
        }
    }
    (p_tokens, c_tokens)
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
    recorded: bool,
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
        let (p_tokens, c_tokens) = parse_sse_usage(&self.resp_buf);

        self.usage.record(UsageRecord {
            timestamp: Utc::now().to_rfc3339(),
            request_id: self.request_id.clone(),
            user_id: self.user_id.clone(),
            user_name: self.user_name.clone(),
            channel_id: self.channel_id.clone(),
            model: self.model.clone(),
            prompt_tokens: p_tokens,
            completion_tokens: c_tokens,
            total_tokens: p_tokens + c_tokens,
            latency_ms,
            status_code: if completed { 200 } else { 499 },
            success: completed,
            request_body: self.req_body.clone(),
            api_key_name: Some(self.api_key_name.clone()),
            reasoning_body: {
                let (reasoning, _) = extract_sse_content(&self.resp_buf);
                Some(if reasoning.len() > 102400 {
                    reasoning.chars().take(102400).collect()
                } else {
                    reasoning
                })
            },
            response_body: {
                let (_, content) = extract_sse_content(&self.resp_buf);
                Some(if content.len() > 102400 {
                    content.chars().take(102400).collect()
                } else {
                    content
                })
            },
        });
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
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    let stream_result = adapter.chat_complete_stream(&endpoint, body).await;

    match stream_result {
        Ok(stream) => {
            let stream = stream.map(|data| normalize_sse_reasoning(&data));
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
                recorded: false,
            };

            let body_stream = usage_stream.map(|data| {
                Ok::<_, std::convert::Infallible>(Bytes::from(data))
            });

            Ok(Response::builder()
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("connection", "keep-alive")
                .header("access-control-allow-origin", "*")
                .body(Body::from_stream(body_stream))
                .map_err(|e| GatewayError::Internal(format!("Response build error: {}", e)))?)
        }
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            let (p_tokens, c_tokens) = parse_sse_usage("");
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
                latency_ms,
                status_code: 502,
                success: false,
                request_body: req_body,
                response_body: None,
                reasoning_body: None,
                api_key_name: Some(api_key_name),
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
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    let mut last_error: Option<ProviderError> = None;

    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            if !route.retry_next() {
                break;
            }
        }

        let result = route.adapter.chat_complete(&route.endpoint, body.clone()).await;
        let latency_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(mut resp) => {
                route.balancer.as_health_aware().record_success(route.endpoint_idx);
                normalize_reasoning_inner(&mut resp);

                let prompt_tokens = resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
                let completion_tokens = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0);

                let reasoning = resp.get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("reasoning_content"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

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
                    latency_ms,
                    status_code: 200,
                    success: true,
                    request_body: req_body,
                    response_body: serde_json::to_string(&resp).ok(),
                    reasoning_body: reasoning,
                    api_key_name: Some(api_key_name),
                });

                return Ok(Json(resp).into_response());
            }
            Err(e) if is_retryable_error(&e) => {
                route.balancer.as_health_aware().record_failure(route.endpoint_idx);
                last_error = Some(e);
                // Continue to next retry attempt
            }
            Err(e) => {
                // Non-retryable (4xx etc.) — don't record failure on the breaker,
                // return immediately without retrying.
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
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: None,
                    reasoning_body: None,
                    api_key_name: None,
                });
                return Err(GatewayError::Upstream(e.0));
            }
        }
    }

    // All retry attempts exhausted without success
    let latency_ms = start.elapsed().as_millis() as u64;
    let err_msg = last_error.map(|e| e.0).unwrap_or_else(|| "All endpoints unavailable".to_string());
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
        latency_ms,
        status_code: 502,
        success: false,
        request_body: req_body,
        response_body: None,
        reasoning_body: None,
        api_key_name: None,
    });
    Err(GatewayError::Upstream(err_msg))
}

// ── Handlers ──────────────────────────────────────────────────────

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();

    let user = state.auth.authenticate(&headers)?;
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

    let (channel_id, upstream_model) = state.routing.route(&user.user_id, &model)?;
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }
    let mut route = resolve_route(&state, &channel_id)?;
    let is_streaming = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    if is_streaming {
        handle_streaming(
            &state, route.adapter, route.endpoint, body,
            request_id, user.user_id, user.user_name, user.api_key_name, route.channel_id, model, start,
        )
        .await
    } else {
        handle_non_streaming(
            &state, &mut route, body,
            request_id, user.user_id, user.user_name, user.api_key_name, channel_id, model, start,
        )
        .await
    }
}

pub async fn messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();

    let user = state.auth.authenticate(&headers)?;
    let model = trim_model(&mut body)?;

    if let Some(ref allowed) = user.allowed_models {
        if !allowed.contains(&model) {
            return Err(GatewayError::Auth(format!("Model '{}' not allowed for this API key", model)));
        }
    }

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm)?;
        state.rate_limiter.check_tpm(&user.user_id, tpm, estimate_tokens_anthropic(&body))?;
    }

    let (channel_id, upstream_model) = state.routing.route(&user.user_id, &model)?;
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }
    let mut route = resolve_route(&state, &channel_id)?;
    let is_streaming = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    if is_streaming {
        handle_streaming(
            &state, route.adapter, route.endpoint, body,
            request_id, user.user_id, user.user_name, user.api_key_name, route.channel_id, model, start,
        )
        .await
    } else {
        handle_non_streaming(
            &state, &mut route, body,
            request_id, user.user_id, user.user_name, user.api_key_name, channel_id, model, start,
        )
        .await
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

    let (channel_id, upstream_model) = state.routing.route(&user.user_id, &model)?;
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }
    let mut route = resolve_route(state, &channel_id)?;
    let latency_ms = start.elapsed().as_millis() as u64;
    let req_body = serde_json::to_string(&body).ok();
    let mut last_error: Option<ProviderError> = None;

    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            if !route.retry_next() {
                break;
            }
        }

        let result = route.adapter.relay(&route.endpoint, upstream_path, body.clone()).await;

        match result {
            Ok(mut resp) => {
                route.balancer.as_health_aware().record_success(route.endpoint_idx);
                normalize_reasoning_inner(&mut resp);
                let prompt_tokens = resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
                let completion_tokens = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0);

                let reasoning = resp.get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("reasoning_content"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

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
                    latency_ms,
                    status_code: 200,
                    success: true,
                    request_body: req_body,
                    response_body: serde_json::to_string(&resp).ok(),
                    reasoning_body: reasoning,
                    api_key_name: Some(user.api_key_name.clone()),
                });

                return Ok(Json(resp).into_response());
            }
            Err(e) if is_retryable_error(&e) => {
                route.balancer.as_health_aware().record_failure(route.endpoint_idx);
                last_error = Some(e);
            }
            Err(e) => {
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
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: None,
                    reasoning_body: None,
                    api_key_name: Some(user.api_key_name.clone()),
                });
                return Err(GatewayError::from(e));
            }
        }
    }

    let err_msg = last_error.map(|e| e.0).unwrap_or_else(|| "All endpoints unavailable".to_string());
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
        latency_ms,
        status_code: 502,
        success: false,
        request_body: req_body,
        response_body: None,
        reasoning_body: None,
        api_key_name: Some(user.api_key_name),
    });
    Err(GatewayError::Upstream(err_msg))
}

pub async fn completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/v1/completions",
        Uuid::new_v4().to_string(), Instant::now()).await
}

pub async fn embeddings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/v1/embeddings",
        Uuid::new_v4().to_string(), Instant::now()).await
}

pub async fn tokenize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/tokenize",
        Uuid::new_v4().to_string(), Instant::now()).await
}

pub async fn detokenize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/detokenize",
        Uuid::new_v4().to_string(), Instant::now()).await
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
) -> Result<Json<Value>, GatewayError> {
    let user = state.auth.authenticate(&headers)?;
    let subs = state.db.list_subscriptions(&user.user_id).unwrap_or_default();
    let subscribed: std::collections::HashSet<String> = subs.iter().map(|m| m.id.clone()).collect();

    let models: Vec<Value> = state.routing.list_display_models()
        .into_iter()
        .filter(|m| subscribed.contains(m["upstream_id"].as_str().unwrap_or("")))
        .collect();

    Ok(Json(serde_json::json!({
        "object": "list",
        "data": models,
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
