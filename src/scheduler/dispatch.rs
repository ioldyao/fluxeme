// ── Dispatch: endpoint selection + retry + per-format execution ──
//
// The `Dispatch` value is the per-request scheduling state: which channel,
// which endpoint, which provider adapter, and which endpoints/channels have
// already been attempted. `call_with_retry` is the shared retry skeleton that
// the openai/anthropic/relay/count_tokens/input_tokens formats all use
// (connect failures don't consume the retry budget; retryable errors do;
// non-retryable errors return immediately without breaker feedback). The
// per-format executors translate a successful upstream response into the
// gateway response + usage record.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::response::{IntoResponse, Json, Response};
use bytes::Bytes;
use chrono::Utc;
use futures::StreamExt;
use serde_json::Value;

use crate::config::types::EndpointConfig;
use crate::domain::usage::UsageRecord;
use crate::observability::lifecycle::{RequestError, RequestLifecycle};
use crate::provider::{
    is_retryable_error, ErrorKind, ProviderAdapter, ProviderError, ProviderRegistry,
};
use crate::scheduler::helpers::{normalize_reasoning_inner, normalize_sse_reasoning, GatewayError};
use crate::scheduler::stream::{
    parse_sse_usage, IdleTimeoutStream, SseBuffer, UsageTrackingStream,
};
use crate::service::routing::RoutingService;
use crate::service::token_reservation::ReservationFinalizer;
use rust_decimal::Decimal;

/// Fixed per-request metadata shared by all executors. `channel_id` here is
/// the channel the request was originally routed to (used for usage records);
/// `Dispatch::channel_id` may change when a retry crosses to another channel.
pub(crate) struct DispatchCtx {
    pub(crate) request_id: String,
    pub(crate) user_id: String,
    pub(crate) user_name: String,
    pub(crate) api_key_name: String,
    pub(crate) channel_id: String,
    pub(crate) model: String,
    pub(crate) orig_model: String,
    pub(crate) start: Instant,
    pub(crate) client_ip: String,
    pub(crate) team_id: Option<String>,
    /// "team" when the request carries a team context, else "user".
    pub(crate) account_type: Option<String>,
}

/// Per-request scheduling state: current endpoint/adapter plus the set of
/// already-attempted endpoints. Endpoint selection is single-level over the
/// model's flattened pool; `retry_next` is the RecoveryPolicy's endpoint-level
/// retry (same-channel scope first, then any other eligible endpoint). The
/// LoadBalancer / selection engine never sees failures.
pub(crate) struct Dispatch {
    pub(crate) channel_id: String,
    pub(crate) endpoint: EndpointConfig,
    pub(crate) endpoint_idx: usize,
    pub(crate) adapter: Arc<dyn ProviderAdapter>,
    pub(crate) runtime: Arc<crate::service::endpoint_pool::ModelEndpointRuntime>,
    pub(crate) routing: Arc<RoutingService>,
    pub(crate) providers: Arc<ProviderRegistry>,
    pub(crate) model: String,
    pub(crate) upstream_model: Option<String>,
    /// Original numeric request value, retained so a retry to another endpoint
    /// re-applies that endpoint's own cap rather than the previous cap.
    pub(crate) requested_max_tokens: Option<u64>,
    /// Endpoint identities already attempted by this request. A retry must
    /// never immediately revisit the same endpoint.
    pub(crate) attempted_endpoint_ids: HashSet<i64>,
    pub(crate) attempted_endpoint_indexes: HashSet<usize>,
}

impl Dispatch {
    /// Re-select another endpoint from the model's flattened pool, excluding
    /// every endpoint this request already tried. Scope policy: prefer another
    /// endpoint in the same channel, then expand to any eligible endpoint.
    /// Returns `false` when nothing remains.
    fn retry_next(&mut self) -> bool {
        if let Some(id) = self.endpoint.id {
            self.attempted_endpoint_ids.insert(id);
        }
        self.attempted_endpoint_indexes.insert(self.endpoint_idx);
        // same_channel_first: try the rest of the current channel first.
        if let Some(idx) = self.runtime.select_healthy_excluding(
            Some(&self.channel_id),
            None,
            &self.attempted_endpoint_ids,
            &self.attempted_endpoint_indexes,
        ) {
            self.apply_endpoint(idx);
            return true;
        }
        if let Some(idx) = self.runtime.select_healthy_excluding(
            None,
            None,
            &self.attempted_endpoint_ids,
            &self.attempted_endpoint_indexes,
        ) {
            self.apply_endpoint(idx);
            return true;
        }
        false
    }

    fn apply_endpoint(&mut self, idx: usize) {
        let state = &self.runtime.endpoints[idx];
        self.endpoint_idx = idx;
        self.endpoint = state.endpoint.clone();
        self.channel_id = state.channel_id.clone();
        self.upstream_model = state.upstream_model.clone();
        if let Some(adapter) = self.providers.get(&state.provider) {
            self.adapter = adapter;
        }
    }

    /// Restore the original request budget and apply the current endpoint's
    /// cap before each attempt. A retry to another endpoint must not inherit
    /// the previous endpoint's (possibly lower) cap.
    fn body_for_attempt(&self, body: &Value) -> Value {
        let mut body = body.clone();
        if let Some(requested) = self.requested_max_tokens {
            body["max_tokens"] = Value::from(requested);
        }
        clamp_max_tokens(&mut body, self.endpoint.max_tokens);
        body
    }

    /// Feed a successful upstream call into the current endpoint's breaker.
    fn report_success(&self) {
        if let Some(state) = self.runtime.endpoints.get(self.endpoint_idx) {
            state.breaker.record_success();
        }
    }

    /// Feed an upstream failure (connect / 5xx / timeout) into the breaker.
    fn report_failure(&mut self) {
        if let Some(state) = self.runtime.endpoints.get(self.endpoint_idx) {
            state.breaker.record_failure();
        }
    }
}

/// Resolve the endpoint from the model's flattened endpoint pool. A system
/// rule may constrain the candidate set to one channel (`channel_scope`).
pub(crate) fn resolve_dispatch(
    svc: &crate::scheduler::SchedulerService,
    model: &str,
    channel_scope: Option<&str>,
    upstream_model: Option<&str>,
) -> Result<Dispatch, GatewayError> {
    let plan = svc
        .routing
        .route_model_endpoint(model, upstream_model, channel_scope, &[])
        .map_err(GatewayError::from)?;
    let adapter = svc
        .providers
        .get(&plan.provider_name)
        .ok_or_else(|| GatewayError::Internal("Provider not available".into()))?;
    Ok(Dispatch {
        channel_id: plan.channel_id,
        endpoint: plan.endpoint,
        endpoint_idx: plan.endpoint_idx,
        adapter,
        runtime: plan.runtime,
        routing: svc.routing.clone(),
        providers: svc.providers.clone(),
        model: model.to_string(),
        upstream_model: plan.upstream_model,
        requested_max_tokens: None,
        attempted_endpoint_ids: HashSet::new(),
        attempted_endpoint_indexes: HashSet::new(),
    })
}

/// Clamp a request's output-token budget to the selected model binding's
/// optional upstream limit. Missing, null, and non-numeric values are left
/// untouched so existing validation/upstream behavior is preserved.
pub(crate) fn clamp_max_tokens(body: &mut Value, limit: Option<u32>) {
    let Some(limit) = limit else {
        return;
    };
    let Some(requested) = body.get("max_tokens").and_then(Value::as_u64) else {
        return;
    };
    if requested > u64::from(limit) {
        body["max_tokens"] = Value::from(limit);
    }
}

impl crate::scheduler::SchedulerService {
    /// Shared retry skeleton.
    ///
    /// Returns `Ok(Value)` on success (breaker success is recorded by each
    /// executor, matching the original handler feedback points). On failure
    /// returns `(error, retry_count)`:
    /// - non-retryable errors return immediately **without** breaker feedback;
    /// - connect failures / retryable errors exhaust the budget then call
    ///   `report_failure` before returning.
    async fn call_with_retry(
        &self,
        dispatch: &mut Dispatch,
        max_retries: u32,
        body: Value,
        mut call: impl FnMut(
                &Dispatch,
                Value,
            ) -> futures::future::BoxFuture<'static, Result<Value, ProviderError>>
            + Send,
    ) -> Result<Value, (ProviderError, u32)> {
        let mut retry_count = 0u32;
        loop {
            let attempt_body = dispatch.body_for_attempt(&body);
            match call(dispatch, attempt_body).await {
                Ok(resp) => {
                    return Ok(resp);
                }
                Err(e) if e.kind() == ErrorKind::ConnectFailed => {
                    // Connect failure: try next endpoint without consuming
                    // retry budget. Only feed the breaker when the request
                    // ultimately fails (no more endpoints to try).
                    if !dispatch.retry_next() {
                        dispatch.report_failure();
                        return Err((e, retry_count));
                    }
                }
                Err(e) if is_retryable_error(&e) => {
                    if retry_count >= max_retries {
                        dispatch.report_failure();
                        return Err((e, retry_count));
                    }
                    retry_count += 1;
                    if !dispatch.retry_next() {
                        dispatch.report_failure();
                        return Err((e, retry_count));
                    }
                }
                Err(e) => {
                    // Non-retryable (4xx etc.) — no breaker feedback.
                    return Err((e, retry_count));
                }
            }
        }
    }

    // ── OpenAI chat format ───────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn exec_openai_stream(
        &self,
        ctx: DispatchCtx,
        adapter: Arc<dyn ProviderAdapter>,
        endpoint: EndpointConfig,
        runtime: Arc<crate::service::endpoint_pool::ModelEndpointRuntime>,
        endpoint_idx: usize,
        body: Value,
        reservation: Option<ReservationFinalizer>,
        lifecycle: Arc<RequestLifecycle>,
    ) -> Result<Response, GatewayError> {
        let req_body = serde_json::to_string(&body).ok();
        self.flow_tracker
            .mark_upstream_started(&ctx.request_id, Utc::now().to_rfc3339());
        let stream_result = adapter.chat_complete_stream(&endpoint, body).await;

        match stream_result {
            Ok(stream) => {
                // Real-time routing event is broadcast by UsageService.record() when
                // the UsageTrackingStream completes (avoids double-counting).
                let (first_byte_timeout, idle_timeout) = {
                    let gw = self.gateway_config.read().unwrap();
                    (
                        Duration::from_secs(gw.stream_first_byte_timeout_secs),
                        Duration::from_secs(gw.stream_idle_timeout_secs),
                    )
                };
                let stream = IdleTimeoutStream::new(stream, first_byte_timeout, idle_timeout);
                let stream = SseBuffer {
                    inner: stream,
                    buf: String::new(),
                    overflow_error: None,
                }
                .map(|data| normalize_sse_reasoning(&data));
                let usage_stream = UsageTrackingStream {
                    inner: stream,
                    resp_buf: String::new(),
                    usage: self.usage.clone(),
                    request_id: ctx.request_id,
                    user_id: ctx.user_id,
                    user_name: ctx.user_name,
                    api_key_name: ctx.api_key_name,
                    channel_id: ctx.channel_id.clone(),
                    model: ctx.model,
                    start: ctx.start,
                    req_body,
                    api_format: "openai".to_string(),
                    recorded: false,
                    client_ip: ctx.client_ip,
                    endpoint_id: endpoint.id,
                    endpoint_url: Some(endpoint.url.clone()),
                    original_model: ctx.orig_model.clone(),
                    runtime: Some(runtime),
                    team_id: ctx.team_id,
                    upstream_started_at: Instant::now(),
                    ttft_ms: None,
                    endpoint_idx,
                    reservation,
                    lifecycle: Some(lifecycle),
                };

                let body_stream =
                    usage_stream.map(|data| Ok::<_, std::convert::Infallible>(Bytes::from(data)));

                Ok(Response::builder()
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .header("connection", "keep-alive")
                    .body(Body::from_stream(body_stream))
                    .map_err(|e| GatewayError::Internal(format!("Response build error: {}", e)))?)
            }
            Err(e) => {
                if let Some(reservation) = &reservation {
                    reservation.release("stream upstream request failed");
                }
                if let Some(state) = runtime.endpoints.get(endpoint_idx) {
                    state.breaker.record_failure();
                }
                tracing::error!(
                    request_id = %ctx.request_id,
                    channel = %ctx.channel_id,
                    model = %ctx.model,
                    endpoint = %endpoint.url,
                    error = %e.0,
                    "Streaming upstream request failed",
                );
                lifecycle.finalize_failed(
                    502,
                    RequestError::new("upstream", "upstream_error").with_message(e.0.clone()),
                );
                let err_body = serde_json::json!({"error": {"message": &e.0}}).to_string();
                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                let (p_tokens, c_tokens, cache_hit, cache_write) = parse_sse_usage("");
                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id,
                    user_id: ctx.user_id,
                    user_name: ctx.user_name,
                    channel_id: ctx.channel_id.clone(),
                    model: ctx.model,
                    prompt_tokens: p_tokens,
                    completion_tokens: c_tokens,
                    total_tokens: p_tokens + cache_hit + c_tokens,
                    cache_hit_input_tokens: cache_hit,
                    cache_write_tokens: cache_write,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: Some(err_body),
                    reasoning_body: None,
                    api_key_name: Some(ctx.api_key_name),
                    api_format: "openai".to_string(),
                    stream: true,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip),
                    endpoint_id: endpoint.id,
                    endpoint_url: Some(endpoint.url.clone()),
                    original_model: ctx.orig_model,
                    team_id: ctx.team_id,
                    ttft_ms: None,
                    account_type: ctx.account_type.clone(),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });
                Err(GatewayError::Upstream(e.0))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn exec_openai_non_stream(
        &self,
        ctx: DispatchCtx,
        dispatch: &mut Dispatch,
        body: Value,
        cache_key: Option<String>,
        cached_response: Option<Value>,
        reservation: Option<ReservationFinalizer>,
        lifecycle: &RequestLifecycle,
    ) -> Result<Response, GatewayError> {
        let req_body = serde_json::to_string(&body).ok();
        let mut cached_response = cached_response;
        let mut retry_count = 0u32;
        let served_from_cache = cached_response.is_some();
        if cached_response.is_none() {
            self.flow_tracker
                .mark_upstream_started(&ctx.request_id, Utc::now().to_rfc3339());
        }
        let max_retries = { self.gateway_config.read().unwrap().max_retries };

        let resp_result = if served_from_cache {
            Ok(cached_response.take().unwrap())
        } else {
            let call = |d: &Dispatch, b: Value| -> futures::future::BoxFuture<'static, Result<Value, ProviderError>> {
                let adapter = d.adapter.clone();
                let endpoint = d.endpoint.clone();
                Box::pin(async move { adapter.chat_complete(&endpoint, b).await })
            };
            match self
                .call_with_retry(dispatch, max_retries, body.clone(), call)
                .await
            {
                Ok(resp) => Ok(resp),
                Err((error, retries)) => {
                    retry_count = retries;
                    Err(error)
                }
            }
        };

        match resp_result {
            Ok(mut resp) => {
                normalize_reasoning_inner(&mut resp);

                let completion_tokens = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0);
                let cache_hit = resp["usage"]["prompt_tokens_details"]["cached_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                let cache_write = resp["usage"]["prompt_tokens_details"]["cache_write_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                // OpenAI prompt_tokens includes cached tokens; subtract them.
                let prompt_tokens = resp["usage"]["prompt_tokens"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_sub(cache_hit + cache_write);

                let reasoning = resp
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("reasoning_content"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                lifecycle.add_tokens(prompt_tokens, completion_tokens, cache_hit, cache_write);
                lifecycle.set_attempts(retry_count + 1, Some(retry_count + 1));
                lifecycle.finalize_success();
                if let Some(reservation) = &reservation {
                    reservation.settle_usage(
                        prompt_tokens,
                        completion_tokens,
                        cache_hit,
                        cache_write,
                        true,
                        "completed",
                    );
                }
                self.usage.record_with_endpoint(
                    UsageRecord {
                        timestamp: Utc::now().to_rfc3339(),
                        request_id: ctx.request_id.clone(),
                        user_id: ctx.user_id.clone(),
                        user_name: ctx.user_name,
                        channel_id: ctx.channel_id.clone(),
                        model: ctx.model.clone(),
                        prompt_tokens,
                        completion_tokens,
                        total_tokens: prompt_tokens + cache_hit + completion_tokens,
                        cache_hit_input_tokens: cache_hit,
                        cache_write_tokens: cache_write,
                        latency_ms,
                        status_code: 200,
                        success: true,
                        request_body: req_body.clone(),
                        response_body: serde_json::to_string(&resp).ok(),
                        reasoning_body: reasoning,
                        api_key_name: Some(ctx.api_key_name.clone()),
                        api_format: "openai".to_string(),
                        stream: false,
                        prompt_price: Decimal::ZERO,
                        completion_price: Decimal::ZERO,
                        cache_read_price: Decimal::ZERO,
                        cache_write_price: Decimal::ZERO,
                        client_ip: Some(ctx.client_ip.clone()),
                        endpoint_id: dispatch.endpoint.id,
                        endpoint_url: Some(dispatch.endpoint.url.clone()),
                        original_model: ctx.orig_model.clone(),
                        team_id: ctx.team_id.clone(),
                        ttft_ms: None,
                        account_type: ctx.account_type.clone(),
                        billing_group_id: None,
                        billing_group_name: None,
                        billing_payment_mode: None,
                    },
                    dispatch.endpoint.id,
                );

                // Cache the response for non-streaming upstream requests.
                if !served_from_cache {
                    if let Some(ref ck) = cache_key {
                        if let Ok(body_str) = serde_json::to_string(&resp) {
                            let ttl = self.gateway_config.read().unwrap().cache_ttl_secs;
                            let _ = self.cache.set(&ctx.user_id, ck, &body_str, ttl).await;
                        }
                    }
                    dispatch.report_success();
                }
                let mut response = Json(resp).into_response();
                response.headers_mut().insert(
                    "x-cache",
                    if served_from_cache {
                        axum::http::HeaderValue::from_static("HIT")
                    } else {
                        axum::http::HeaderValue::from_static("MISS")
                    },
                );
                Ok(response)
            }
            Err(e) => {
                let exhausted =
                    matches!(e.kind(), ErrorKind::ConnectFailed) || is_retryable_error(&e);
                if let Some(reservation) = &reservation {
                    reservation.release(if exhausted {
                        "upstream retries exhausted"
                    } else {
                        "upstream non-retryable error"
                    });
                }
                // Non-retryable (4xx etc.) — don't record failure on the breaker;
                // retries-exhausted paths already recorded failure in call_with_retry.
                let err_body = serde_json::json!({"error": {"message": &e.0}}).to_string();
                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id.clone(),
                    user_id: ctx.user_id.clone(),
                    user_name: ctx.user_name.clone(),
                    channel_id: ctx.channel_id.clone(),
                    model: ctx.model.clone(),
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cache_hit_input_tokens: 0,
                    cache_write_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body.clone(),
                    response_body: Some(err_body),
                    reasoning_body: None,
                    api_key_name: Some(ctx.api_key_name.clone()),
                    api_format: "openai".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip.clone()),
                    endpoint_id: dispatch.endpoint.id,
                    endpoint_url: Some(dispatch.endpoint.url.clone()),
                    original_model: ctx.orig_model.clone(),
                    team_id: ctx.team_id.clone(),
                    ttft_ms: None,
                    account_type: ctx.account_type.clone(),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });
                if exhausted {
                    tracing::error!(
                        request_id = %ctx.request_id,
                        channel = %ctx.channel_id,
                        model = %ctx.model,
                        endpoint = %dispatch.endpoint.url,
                        error = %e.0,
                        retries = retry_count,
                        "Upstream request retries exhausted",
                    );
                } else {
                    tracing::error!(
                        request_id = %ctx.request_id,
                        endpoint = %dispatch.endpoint.url,
                        error = %e.0,
                        "Upstream request failed",
                    );
                }
                Err(GatewayError::Upstream(e.0))
            }
        }
    }

    // ── Anthropic messages format ────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn exec_messages_stream(
        &self,
        ctx: DispatchCtx,
        adapter: Arc<dyn ProviderAdapter>,
        endpoint: EndpointConfig,
        runtime: Arc<crate::service::endpoint_pool::ModelEndpointRuntime>,
        endpoint_idx: usize,
        body: Value,
        reservation: Option<ReservationFinalizer>,
        lifecycle: Arc<RequestLifecycle>,
    ) -> Result<Response, GatewayError> {
        let req_body = serde_json::to_string(&body).ok();
        self.flow_tracker
            .mark_upstream_started(&ctx.request_id, Utc::now().to_rfc3339());
        let stream_result = adapter.messages_stream(&endpoint, body).await;

        match stream_result {
            Ok(stream) => {
                // Real-time routing event is broadcast by UsageService.record() when
                // the UsageTrackingStream completes (avoids double-counting).
                let (first_byte_timeout, idle_timeout) = {
                    let gw = self.gateway_config.read().unwrap();
                    (
                        Duration::from_secs(gw.stream_first_byte_timeout_secs),
                        Duration::from_secs(gw.stream_idle_timeout_secs),
                    )
                };
                let stream = IdleTimeoutStream::new(stream, first_byte_timeout, idle_timeout);
                let stream = SseBuffer {
                    inner: stream,
                    buf: String::new(),
                    overflow_error: None,
                };
                let usage_stream = UsageTrackingStream {
                    inner: stream,
                    resp_buf: String::new(),
                    usage: self.usage.clone(),
                    request_id: ctx.request_id,
                    user_id: ctx.user_id,
                    user_name: ctx.user_name,
                    api_key_name: ctx.api_key_name,
                    channel_id: ctx.channel_id.clone(),
                    model: ctx.model,
                    start: ctx.start,
                    req_body,
                    api_format: "anthropic".to_string(),
                    recorded: false,
                    client_ip: ctx.client_ip,
                    endpoint_id: endpoint.id,
                    endpoint_url: Some(endpoint.url.clone()),
                    original_model: ctx.orig_model.clone(),
                    runtime: Some(runtime),
                    team_id: ctx.team_id,
                    upstream_started_at: Instant::now(),
                    ttft_ms: None,
                    endpoint_idx,
                    reservation,
                    lifecycle: Some(lifecycle),
                };

                let body_stream =
                    usage_stream.map(|data| Ok::<_, std::convert::Infallible>(Bytes::from(data)));

                Ok(Response::builder()
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .header("connection", "keep-alive")
                    .body(Body::from_stream(body_stream))
                    .map_err(|e| GatewayError::Internal(format!("Response build error: {}", e)))?)
            }
            Err(e) => {
                if let Some(reservation) = &reservation {
                    reservation.release("stream upstream request failed");
                }
                if let Some(state) = runtime.endpoints.get(endpoint_idx) {
                    state.breaker.record_failure();
                }
                tracing::error!(
                    request_id = %ctx.request_id,
                    channel = %ctx.channel_id,
                    model = %ctx.model,
                    endpoint = %endpoint.url,
                    error = %e.0,
                    "Messages streaming upstream request failed",
                );
                lifecycle.finalize_failed(
                    502,
                    RequestError::new("upstream", "upstream_error").with_message(e.0.clone()),
                );
                let err_body = serde_json::json!({"error": {"message": &e.0}}).to_string();
                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                let (p_tokens, c_tokens, cache_hit, cache_write) = parse_sse_usage("");
                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id,
                    user_id: ctx.user_id,
                    user_name: ctx.user_name,
                    channel_id: ctx.channel_id.clone(),
                    model: ctx.model,
                    prompt_tokens: p_tokens,
                    completion_tokens: c_tokens,
                    total_tokens: p_tokens + cache_hit + c_tokens,
                    cache_hit_input_tokens: cache_hit,
                    cache_write_tokens: cache_write,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: Some(err_body),
                    reasoning_body: None,
                    api_key_name: Some(ctx.api_key_name),
                    api_format: "anthropic".to_string(),
                    stream: true,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip),
                    endpoint_id: endpoint.id,
                    endpoint_url: Some(endpoint.url.clone()),
                    original_model: ctx.orig_model,
                    team_id: ctx.team_id,
                    ttft_ms: None,
                    account_type: ctx.account_type.clone(),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });
                Err(GatewayError::Upstream(e.0))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn exec_messages_non_stream(
        &self,
        ctx: DispatchCtx,
        dispatch: &mut Dispatch,
        body: Value,
        reservation: Option<ReservationFinalizer>,
        lifecycle: &RequestLifecycle,
    ) -> Result<Response, GatewayError> {
        let req_body = serde_json::to_string(&body).ok();
        self.flow_tracker
            .mark_upstream_started(&ctx.request_id, Utc::now().to_rfc3339());
        let max_retries = { self.gateway_config.read().unwrap().max_retries };
        let mut retry_count = 0u32;

        let call = |d: &Dispatch,
                    b: Value|
         -> futures::future::BoxFuture<'static, Result<Value, ProviderError>> {
            let adapter = d.adapter.clone();
            let endpoint = d.endpoint.clone();
            Box::pin(async move { adapter.messages(&endpoint, b).await })
        };
        let resp_result = match self
            .call_with_retry(dispatch, max_retries, body.clone(), call)
            .await
        {
            Ok(resp) => Ok(resp),
            Err((error, retries)) => {
                retry_count = retries;
                Err(error)
            }
        };

        match resp_result {
            Ok(resp) => {
                dispatch.report_success();
                let prompt_tokens = resp["usage"]["input_tokens"].as_u64().unwrap_or(0);
                let completion_tokens = resp["usage"]["output_tokens"].as_u64().unwrap_or(0);
                let cache_hit = resp["usage"]["cache_read_input_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                let cache_write = resp["usage"]["cache_creation_input_tokens"]
                    .as_u64()
                    .unwrap_or(0);

                let reasoning = resp
                    .get("content")
                    .and_then(|c| c.as_array())
                    .and_then(|blocks| {
                        blocks
                            .iter()
                            .find_map(|b| {
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

                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                lifecycle.add_tokens(prompt_tokens, completion_tokens, cache_hit, cache_write);
                lifecycle.set_attempts(retry_count + 1, Some(retry_count + 1));
                lifecycle.finalize_success();
                if let Some(reservation) = &reservation {
                    reservation.settle_usage(
                        prompt_tokens,
                        completion_tokens,
                        cache_hit,
                        cache_write,
                        true,
                        "completed",
                    );
                }
                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id,
                    user_id: ctx.user_id,
                    user_name: ctx.user_name,
                    channel_id: ctx.channel_id.clone(),
                    model: ctx.model,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens + cache_hit + completion_tokens,
                    cache_hit_input_tokens: cache_hit,
                    cache_write_tokens: cache_write,
                    latency_ms,
                    status_code: 200,
                    success: true,
                    request_body: req_body,
                    response_body: serde_json::to_string(&resp).ok(),
                    reasoning_body: reasoning,
                    api_key_name: Some(ctx.api_key_name),
                    api_format: "anthropic".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip.clone()),
                    endpoint_id: dispatch.endpoint.id,
                    endpoint_url: Some(dispatch.endpoint.url.clone()),
                    original_model: ctx.orig_model.clone(),
                    team_id: ctx.team_id.clone(),
                    ttft_ms: None,
                    account_type: ctx.account_type.clone(),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });

                Ok(Json(resp).into_response())
            }
            Err(e) => {
                let exhausted =
                    matches!(e.kind(), ErrorKind::ConnectFailed) || is_retryable_error(&e);
                if let Some(reservation) = &reservation {
                    reservation.release(if exhausted {
                        "upstream retries exhausted"
                    } else {
                        "upstream non-retryable error"
                    });
                }
                let err_body = serde_json::json!({"error": {"message": &e.0}}).to_string();
                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id.clone(),
                    user_id: ctx.user_id.clone(),
                    user_name: ctx.user_name.clone(),
                    channel_id: ctx.channel_id.clone(),
                    model: ctx.model.clone(),
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cache_hit_input_tokens: 0,
                    cache_write_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body.clone(),
                    response_body: Some(err_body),
                    reasoning_body: None,
                    api_key_name: Some(ctx.api_key_name.clone()),
                    api_format: "anthropic".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip.clone()),
                    endpoint_id: dispatch.endpoint.id,
                    endpoint_url: Some(dispatch.endpoint.url.clone()),
                    original_model: ctx.orig_model.clone(),
                    team_id: ctx.team_id.clone(),
                    ttft_ms: None,
                    account_type: ctx.account_type.clone(),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });
                if exhausted {
                    tracing::error!(
                        request_id = %ctx.request_id,
                        channel = %ctx.channel_id,
                        model = %ctx.model,
                        endpoint = %dispatch.endpoint.url,
                        error = %e.0,
                        retries = retry_count,
                        "Messages upstream request retries exhausted",
                    );
                } else {
                    tracing::error!(
                        request_id = %ctx.request_id,
                        endpoint = %dispatch.endpoint.url,
                        error = %e.0,
                        "Messages upstream request failed",
                    );
                }
                Err(GatewayError::Upstream(e.0))
            }
        }
    }

    // ── Relay format ─────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn exec_relay(
        &self,
        ctx: DispatchCtx,
        dispatch: &mut Dispatch,
        body: Value,
        upstream_path: &str,
        reservation: Option<ReservationFinalizer>,
        lifecycle: &RequestLifecycle,
    ) -> Result<Response, GatewayError> {
        let req_body = Some(serde_json::to_string(&body).unwrap_or_default());
        self.flow_tracker
            .mark_upstream_started(&ctx.request_id, Utc::now().to_rfc3339());
        let max_retries = { self.gateway_config.read().unwrap().max_retries };
        let mut retry_count = 0u32;

        let call = |d: &Dispatch,
                    b: Value|
         -> futures::future::BoxFuture<'static, Result<Value, ProviderError>> {
            let adapter = d.adapter.clone();
            let endpoint = d.endpoint.clone();
            let path = upstream_path.to_string();
            Box::pin(async move { adapter.relay(&endpoint, &path, b).await })
        };
        let resp_result = match self
            .call_with_retry(dispatch, max_retries, body.clone(), call)
            .await
        {
            Ok(resp) => Ok(resp),
            Err((error, retries)) => {
                retry_count = retries;
                Err(error)
            }
        };

        match resp_result {
            Ok(mut resp) => {
                dispatch.report_success();
                normalize_reasoning_inner(&mut resp);
                let completion_tokens = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0);
                let cache_hit = resp["usage"]["prompt_tokens_details"]["cached_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                let cache_write = resp["usage"]["prompt_tokens_details"]["cache_write_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                // OpenAI prompt_tokens includes cached tokens; subtract them.
                let prompt_tokens = resp["usage"]["prompt_tokens"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_sub(cache_hit + cache_write);

                let reasoning = resp
                    .get("choices")
                    .and_then(|c| c.get(0))
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("reasoning_content"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string());

                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                lifecycle.add_tokens(prompt_tokens, completion_tokens, cache_hit, cache_write);
                lifecycle.set_attempts(retry_count + 1, Some(retry_count + 1));
                lifecycle.finalize_success();
                if let Some(reservation) = &reservation {
                    reservation.settle_usage(
                        prompt_tokens,
                        completion_tokens,
                        cache_hit,
                        cache_write,
                        true,
                        "completed",
                    );
                }
                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id,
                    user_id: ctx.user_id,
                    user_name: ctx.user_name,
                    channel_id: dispatch.channel_id.clone(),
                    model: ctx.model,
                    prompt_tokens,
                    completion_tokens,
                    total_tokens: prompt_tokens + cache_hit + completion_tokens,
                    cache_hit_input_tokens: cache_hit,
                    cache_write_tokens: cache_write,
                    latency_ms,
                    status_code: 200,
                    success: true,
                    request_body: req_body,
                    response_body: serde_json::to_string(&resp).ok(),
                    reasoning_body: reasoning,
                    api_key_name: Some(ctx.api_key_name.clone()),
                    api_format: "relay".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip.clone()),
                    endpoint_id: dispatch.endpoint.id,
                    endpoint_url: Some(dispatch.endpoint.url.clone()),
                    original_model: ctx.orig_model.clone(),
                    team_id: ctx.team_id.clone(),
                    ttft_ms: None,
                    account_type: ctx.account_type.clone(),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });

                Ok(Json(resp).into_response())
            }
            Err(e) => {
                let exhausted =
                    matches!(e.kind(), ErrorKind::ConnectFailed) || is_retryable_error(&e);
                if let Some(reservation) = &reservation {
                    reservation.release(if exhausted {
                        "relay retries exhausted"
                    } else {
                        "upstream non-retryable error"
                    });
                }
                let err_body = serde_json::json!({"error": {"message": &e.0}}).to_string();
                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id.clone(),
                    user_id: ctx.user_id.clone(),
                    user_name: ctx.user_name.clone(),
                    channel_id: dispatch.channel_id.clone(),
                    model: ctx.model.clone(),
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cache_hit_input_tokens: 0,
                    cache_write_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: Some(err_body),
                    reasoning_body: None,
                    api_key_name: Some(ctx.api_key_name.clone()),
                    api_format: "relay".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip.clone()),
                    endpoint_id: dispatch.endpoint.id,
                    endpoint_url: Some(dispatch.endpoint.url.clone()),
                    original_model: ctx.orig_model.clone(),
                    team_id: ctx.team_id.clone(),
                    ttft_ms: None,
                    account_type: ctx.account_type.clone(),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });
                if exhausted {
                    tracing::error!(
                        request_id = %ctx.request_id,
                        channel = %ctx.channel_id,
                        model = %ctx.model,
                        endpoint = %dispatch.endpoint.url,
                        error = %e.0,
                        retries = retry_count,
                        "Relay upstream request retries exhausted",
                    );
                } else {
                    tracing::error!(
                        request_id = %ctx.request_id,
                        endpoint = %dispatch.endpoint.url,
                        error = %e.0,
                        "Relay upstream request failed",
                    );
                }
                Err(GatewayError::Upstream(e.0))
            }
        }
    }

    // ── Responses format ─────────────────────────────────────────────

    pub(crate) async fn exec_responses_non_stream(
        &self,
        ctx: DispatchCtx,
        dispatch: &mut Dispatch,
        body: Value,
        reservation: Option<ReservationFinalizer>,
        lifecycle: &RequestLifecycle,
    ) -> Result<Response, GatewayError> {
        let req_body = serde_json::to_string(&body).ok();
        self.flow_tracker
            .mark_upstream_started(&ctx.request_id, Utc::now().to_rfc3339());

        let max_retries = self.gateway_config.read().unwrap().max_retries;
        let mut retries = 0u32;
        let result = loop {
            let attempt_body = dispatch.body_for_attempt(&body);
            let result = dispatch
                .adapter
                .relay(&dispatch.endpoint, "/v1/responses", attempt_body)
                .await;
            match result {
                Err(error)
                    if error.kind() == ErrorKind::ConnectFailed || is_retryable_error(&error) =>
                {
                    if retries >= max_retries || !dispatch.retry_next() {
                        dispatch.report_failure();
                        break Err(error);
                    }
                    retries += 1;
                }
                result => break result,
            }
        };

        match result {
            Ok(resp) => {
                dispatch.report_success();
                let latency_ms = ctx.start.elapsed().as_millis() as u64;

                // Responses API usage: input_tokens, input_tokens_details.cached_tokens, output_tokens
                let raw_input_tokens = resp["usage"]["input_tokens"].as_u64().unwrap_or(0);
                let output_tokens = resp["usage"]["output_tokens"].as_u64().unwrap_or(0);
                let cache_hit = resp["usage"]["input_tokens_details"]["cached_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                let cache_write = resp["usage"]["input_tokens_details"]["cache_write_tokens"]
                    .as_u64()
                    .unwrap_or(0);
                // Keep prompt_tokens consistent with the OpenAI chat path: it is
                // the uncached input component, while cache tokens are reported separately.
                let input_tokens = raw_input_tokens.saturating_sub(cache_hit + cache_write);
                lifecycle.add_tokens(input_tokens, output_tokens, cache_hit, cache_write);
                lifecycle.set_attempts(retries + 1, Some(retries + 1));
                lifecycle.finalize_success();
                if let Some(reservation) = &reservation {
                    reservation.settle_usage(
                        input_tokens,
                        output_tokens,
                        cache_hit,
                        cache_write,
                        true,
                        "completed",
                    );
                }

                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id,
                    user_id: ctx.user_id,
                    user_name: ctx.user_name,
                    channel_id: dispatch.channel_id.clone(),
                    model: ctx.model,
                    prompt_tokens: input_tokens,
                    completion_tokens: output_tokens,
                    total_tokens: input_tokens + cache_hit + output_tokens,
                    cache_hit_input_tokens: cache_hit,
                    cache_write_tokens: cache_write,
                    latency_ms,
                    status_code: 200,
                    success: true,
                    request_body: req_body,
                    response_body: serde_json::to_string(&resp).ok(),
                    reasoning_body: None,
                    api_key_name: Some(ctx.api_key_name.clone()),
                    api_format: "openai".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip),
                    endpoint_id: dispatch.endpoint.id,
                    endpoint_url: Some(dispatch.endpoint.url.clone()),
                    original_model: ctx.orig_model,
                    team_id: ctx.team_id.clone(),
                    ttft_ms: None,
                    account_type: Some("user".to_string()),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });

                Ok(Json(resp).into_response())
            }
            Err(e) => {
                let reason = if e.kind() == ErrorKind::ConnectFailed {
                    "responses upstream connect failed"
                } else if is_retryable_error(&e) {
                    "responses upstream retryable failure"
                } else {
                    "responses upstream request failed"
                };
                if let Some(reservation) = &reservation {
                    reservation.release(reason);
                }
                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id,
                    user_id: ctx.user_id.clone(),
                    user_name: ctx.user_name.clone(),
                    channel_id: dispatch.channel_id.clone(),
                    model: ctx.model,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cache_hit_input_tokens: 0,
                    cache_write_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: Some(format!("{{\"error\":\"{}\"}}", e.0)),
                    reasoning_body: None,
                    api_key_name: Some(ctx.api_key_name.clone()),
                    api_format: "openai".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip),
                    endpoint_id: dispatch.endpoint.id,
                    endpoint_url: Some(dispatch.endpoint.url.clone()),
                    original_model: ctx.orig_model,
                    team_id: ctx.team_id.clone(),
                    ttft_ms: None,
                    account_type: Some("user".to_string()),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });
                Err(GatewayError::Upstream(e.0))
            }
        }
    }

    pub(crate) async fn exec_responses_stream(
        &self,
        ctx: DispatchCtx,
        dispatch: &mut Dispatch,
        mut body: Value,
        reservation: Option<ReservationFinalizer>,
        lifecycle: Arc<RequestLifecycle>,
    ) -> Result<Response, GatewayError> {
        // Inject stream_options so upstream returns usage in the final event
        match body.get_mut("stream_options") {
            Some(Value::Object(opts)) => {
                opts.insert("include_usage".into(), Value::Bool(true));
            }
            _ => {
                body["stream_options"] = serde_json::json!({"include_usage": true});
            }
        }

        let req_body = serde_json::to_string(&body).ok();
        self.flow_tracker
            .mark_upstream_started(&ctx.request_id, Utc::now().to_rfc3339());

        let max_retries = self.gateway_config.read().unwrap().max_retries;
        let mut retries = 0u32;
        let stream_result = loop {
            let attempt_body = dispatch.body_for_attempt(&body);
            let result = dispatch
                .adapter
                .responses_stream(&dispatch.endpoint, attempt_body)
                .await;
            match result {
                Err(error)
                    if error.kind() == ErrorKind::ConnectFailed || is_retryable_error(&error) =>
                {
                    if retries >= max_retries || !dispatch.retry_next() {
                        dispatch.report_failure();
                        break Err(error);
                    }
                    retries += 1;
                }
                result => break result,
            }
        };

        match stream_result {
            Ok(stream) => {
                dispatch.report_success();

                // Wrap the stream to capture response.completed usage
                let resp_buf = Arc::new(std::sync::Mutex::new(String::new()));
                let recorded = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let clean_eof = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let usage_state = self.usage.clone();
                let rid = ctx.request_id.clone();
                let uid = ctx.user_id.clone();
                let uname = ctx.user_name.clone();
                let akn = ctx.api_key_name.clone();
                let chid = ctx.channel_id.clone();
                let mdl = ctx.model.clone();
                let orig_mdl = ctx.orig_model.clone();
                let st = ctx.start;
                let rbody = req_body.clone();
                let cip = ctx.client_ip.clone();
                let eid = dispatch.endpoint.id;
                let eurl = dispatch.endpoint.url.clone();
                let tid = ctx.team_id.clone();
                let reservation_finalizer = reservation.clone();
                let lifecycle_stream = lifecycle.clone();
                let clean_eof2 = clean_eof.clone();
                let buf2 = resp_buf.clone();
                let rec2 = recorded.clone();
                let flow_tracker = self.flow_tracker.clone();
                let first_byte_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let first_byte_seen2 = first_byte_seen.clone();
                let first_byte_request_id = ctx.request_id.clone();

                let tracing_stream = stream.map(move |data| {
                    if !data.is_empty()
                        && !first_byte_seen2.swap(true, std::sync::atomic::Ordering::SeqCst)
                    {
                        flow_tracker
                            .mark_first_byte(&first_byte_request_id, Utc::now().to_rfc3339());
                    }
                    let mut b = buf2.lock().unwrap();
                    b.push_str(&data);
                    data
                });

                let on_done = move || {
                    if rec2.swap(true, std::sync::atomic::Ordering::SeqCst) {
                        return;
                    }
                    let latency_ms = st.elapsed().as_millis() as u64;
                    let buf = resp_buf.lock().unwrap().clone();
                    let (input_tokens, output_tokens, cache_hit, cache_write) =
                        parse_responses_sse_usage(&buf);
                    let completed = clean_eof2.load(std::sync::atomic::Ordering::Acquire);
                    // Finalize the request lifecycle at stream end: clean EOF →
                    // succeeded; premature end (client disconnect) → cancelled/499.
                    lifecycle_stream.add_tokens(
                        input_tokens,
                        output_tokens,
                        cache_hit,
                        cache_write,
                    );
                    lifecycle_stream.set_attempts(1, if completed { Some(1) } else { None });
                    if completed {
                        lifecycle_stream.finalize_success();
                    } else {
                        lifecycle_stream.mark_client_disconnected();
                        lifecycle_stream.finalize_cancelled(
                            499,
                            RequestError::new("response_stream", "client_disconnect"),
                        );
                    }
                    if let Some(reservation) = &reservation_finalizer {
                        if completed {
                            reservation.settle_usage(
                                input_tokens,
                                output_tokens,
                                cache_hit,
                                cache_write,
                                true,
                                "completed",
                            );
                        } else {
                            reservation.release_partial(
                                input_tokens,
                                output_tokens,
                                cache_hit,
                                cache_write,
                                "responses stream dropped",
                            );
                        }
                    }
                    usage_state.record(UsageRecord {
                        timestamp: Utc::now().to_rfc3339(),
                        request_id: rid,
                        user_id: uid,
                        user_name: uname,
                        channel_id: chid,
                        model: mdl,
                        prompt_tokens: input_tokens,
                        completion_tokens: output_tokens,
                        total_tokens: input_tokens + cache_hit + output_tokens,
                        cache_hit_input_tokens: cache_hit,
                        cache_write_tokens: cache_write,
                        latency_ms,
                        status_code: if completed { 200 } else { 499 },
                        success: completed,
                        request_body: rbody,
                        response_body: None,
                        reasoning_body: None,
                        api_key_name: Some(akn),
                        api_format: "openai".to_string(),
                        stream: true,
                        prompt_price: Decimal::ZERO,
                        completion_price: Decimal::ZERO,
                        cache_read_price: Decimal::ZERO,
                        cache_write_price: Decimal::ZERO,
                        client_ip: Some(cip),
                        endpoint_id: eid,
                        endpoint_url: Some(eurl),
                        original_model: orig_mdl,
                        team_id: tid,
                        ttft_ms: None,
                        account_type: Some("user".to_string()),
                        billing_group_id: None,
                        billing_group_name: None,
                        billing_payment_mode: None,
                    });
                };

                // Wrap in a struct that calls on_done on Drop
                struct TracingStream<S> {
                    inner: S,
                    clean_eof: Arc<std::sync::atomic::AtomicBool>,
                    on_done: Option<Box<dyn FnOnce() + Send>>,
                }
                impl<S: futures::Stream<Item = String> + Unpin> futures::Stream for TracingStream<S> {
                    type Item = String;
                    fn poll_next(
                        mut self: std::pin::Pin<&mut Self>,
                        cx: &mut std::task::Context<'_>,
                    ) -> std::task::Poll<Option<Self::Item>> {
                        let poll = std::pin::Pin::new(&mut self.inner).poll_next(cx);
                        if matches!(poll, std::task::Poll::Ready(None)) {
                            // Only a real upstream EOF is a successful response.
                            // Drop remains the partial/client-disconnect path.
                            self.clean_eof
                                .store(true, std::sync::atomic::Ordering::Release);
                        }
                        poll
                    }
                }
                impl<S> Drop for TracingStream<S> {
                    fn drop(&mut self) {
                        if let Some(f) = self.on_done.take() {
                            f();
                        }
                    }
                }

                let tracing_stream = TracingStream {
                    inner: tracing_stream,
                    clean_eof,
                    on_done: Some(Box::new(on_done)),
                };

                let body_stream =
                    tracing_stream.map(|data| Ok::<_, std::convert::Infallible>(Bytes::from(data)));

                Ok(Response::builder()
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .header("connection", "keep-alive")
                    .body(Body::from_stream(body_stream))
                    .map_err(|e| GatewayError::Internal(format!("Response build error: {}", e)))?)
            }
            Err(e) => {
                tracing::error!(
                    request_id = %ctx.request_id,
                    channel = %ctx.channel_id,
                    model = %ctx.model,
                    endpoint = %dispatch.endpoint.url,
                    error = %e.0,
                    "Responses streaming upstream request failed",
                );
                lifecycle.finalize_failed(
                    502,
                    RequestError::new("upstream", "upstream_error").with_message(e.0.clone()),
                );
                if let Some(reservation) = &reservation {
                    reservation.release("responses stream upstream failed");
                }
                let latency_ms = ctx.start.elapsed().as_millis() as u64;
                self.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: ctx.request_id,
                    user_id: ctx.user_id.clone(),
                    user_name: ctx.user_name.clone(),
                    channel_id: dispatch.channel_id.clone(),
                    model: ctx.model,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    cache_hit_input_tokens: 0,
                    cache_write_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: Some(format!("{{\"error\":\"{}\"}}", e.0)),
                    reasoning_body: None,
                    api_key_name: Some(ctx.api_key_name.clone()),
                    api_format: "openai".to_string(),
                    stream: true,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(ctx.client_ip),
                    endpoint_id: dispatch.endpoint.id,
                    endpoint_url: Some(dispatch.endpoint.url.clone()),
                    original_model: ctx.orig_model,
                    team_id: ctx.team_id.clone(),
                    ttft_ms: None,
                    account_type: Some("user".to_string()),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });
                Err(GatewayError::Upstream(e.0))
            }
        }
    }

    // ── Token-counting formats ───────────────────────────────────────

    pub(crate) async fn exec_count_tokens(
        &self,
        dispatch: &mut Dispatch,
        body: Value,
        request_id: &str,
        max_retries: u32,
    ) -> Result<Value, GatewayError> {
        self.flow_tracker
            .mark_upstream_started(request_id, Utc::now().to_rfc3339());
        let call = |d: &Dispatch,
                    b: Value|
         -> futures::future::BoxFuture<'static, Result<Value, ProviderError>> {
            let adapter = d.adapter.clone();
            let endpoint = d.endpoint.clone();
            Box::pin(async move { adapter.count_tokens(&endpoint, b).await })
        };
        match self
            .call_with_retry(dispatch, max_retries, body, call)
            .await
        {
            Ok(resp) => {
                dispatch.report_success();
                Ok(resp)
            }
            Err((error, _)) => {
                let exhausted =
                    matches!(error.kind(), ErrorKind::ConnectFailed) || is_retryable_error(&error);
                if exhausted {
                    tracing::error!(
                        request_id = %request_id,
                        endpoint = %dispatch.endpoint.url,
                        error = %error.0,
                        "Count tokens upstream request failed after retries"
                    );
                } else {
                    tracing::error!(
                        request_id = %request_id,
                        endpoint = %dispatch.endpoint.url,
                        error = %error.0,
                        "Count tokens upstream request failed"
                    );
                }
                Err(GatewayError::Upstream(
                    "Upstream count_tokens request failed".into(),
                ))
            }
        }
    }

    pub(crate) async fn exec_responses_input_tokens(
        &self,
        dispatch: &mut Dispatch,
        body: Value,
        request_id: &str,
        max_retries: u32,
    ) -> Result<Value, GatewayError> {
        self.flow_tracker
            .mark_upstream_started(request_id, Utc::now().to_rfc3339());
        let call = |d: &Dispatch,
                    b: Value|
         -> futures::future::BoxFuture<'static, Result<Value, ProviderError>> {
            let adapter = d.adapter.clone();
            let endpoint = d.endpoint.clone();
            Box::pin(async move { adapter.responses_input_tokens(&endpoint, b).await })
        };
        match self
            .call_with_retry(dispatch, max_retries, body, call)
            .await
        {
            Ok(resp) => {
                dispatch.report_success();
                Ok(resp)
            }
            Err((error, _)) => {
                let exhausted =
                    matches!(error.kind(), ErrorKind::ConnectFailed) || is_retryable_error(&error);
                if exhausted {
                    tracing::error!(
                        request_id = %request_id,
                        endpoint = %dispatch.endpoint.url,
                        error = %error.0,
                        "Responses input_tokens upstream request failed after retries"
                    );
                } else {
                    tracing::error!(
                        request_id = %request_id,
                        endpoint = %dispatch.endpoint.url,
                        error = %error.0,
                        "Responses input_tokens upstream request failed"
                    );
                }
                Err(GatewayError::Upstream(
                    "Upstream responses input_tokens request failed".into(),
                ))
            }
        }
    }
}

/// Extract token usage from Responses API SSE events.
/// Looks for response.completed event with usage data.
/// Format: {response: {usage: {input_tokens, input_tokens_details: {cached_tokens}, output_tokens}}}
pub(crate) fn parse_responses_sse_usage(data: &str) -> (u64, u64, u64, u64) {
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut cache_hit = 0u64;
    let mut cache_write = 0u64;
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with("data: ") {
            continue;
        }
        let json_str = trimmed.strip_prefix("data: ").unwrap_or(trimmed);
        if let Ok(val) = serde_json::from_str::<Value>(json_str) {
            // Look for response.completed event: {type: "response.completed", response: {usage: {...}}}
            if val.get("type").and_then(|t| t.as_str()) == Some("response.completed") {
                if let Some(resp) = val.get("response") {
                    if let Some(usage) = resp.get("usage") {
                        if let Some(p) = usage.get("input_tokens").and_then(|v| v.as_u64()) {
                            input_tokens = p;
                        }
                        if let Some(c) = usage.get("output_tokens").and_then(|v| v.as_u64()) {
                            output_tokens = c;
                        }
                        if let Some(details) = usage.get("input_tokens_details") {
                            if let Some(cached) =
                                details.get("cached_tokens").and_then(|v| v.as_u64())
                            {
                                cache_hit = cached;
                            }
                            if let Some(written) =
                                details.get("cache_write_tokens").and_then(|v| v.as_u64())
                            {
                                cache_write = written;
                            }
                        }
                    }
                }
            }
        }
    }
    (input_tokens, output_tokens, cache_hit, cache_write)
}

/// Whether a channel supports POST /v1/messages/count_tokens.
pub(crate) fn count_tokens_supported_for_channel(
    channel: Option<&crate::domain::channel::Channel>,
) -> bool {
    !matches!(channel, Some(ch) if ch.anthropic_compat)
}

/// Whether a channel supports POST /responses/input_tokens.
pub(crate) fn responses_input_tokens_supported_for_channel(
    channel: Option<&crate::domain::channel::Channel>,
) -> bool {
    matches!(
        channel.map(|ch| ch.provider.as_str()),
        None | Some("openai" | "azure" | "ollama")
    )
}

#[cfg(test)]
mod tests {
    use super::clamp_max_tokens;
    use serde_json::json;

    #[test]
    fn clamps_requested_max_tokens_above_binding_limit() {
        let mut body = json!({"max_tokens": 100_000});
        clamp_max_tokens(&mut body, Some(65_536));
        assert_eq!(body["max_tokens"], 65_536);
    }

    #[test]
    fn preserves_requested_max_tokens_within_binding_limit() {
        let mut body = json!({"max_tokens": 32_768});
        clamp_max_tokens(&mut body, Some(65_536));
        assert_eq!(body["max_tokens"], 32_768);
    }

    #[test]
    fn leaves_missing_max_tokens_untouched() {
        let mut body = json!({"model": "example"});
        clamp_max_tokens(&mut body, Some(65_536));
        assert_eq!(body, json!({"model": "example"}));
    }

    #[test]
    fn leaves_non_numeric_max_tokens_untouched() {
        let mut body = json!({"max_tokens": "100000"});
        clamp_max_tokens(&mut body, Some(65_536));
        assert_eq!(body["max_tokens"], "100000");
    }

    #[test]
    fn leaves_body_untouched_without_binding_limit() {
        let mut body = json!({"max_tokens": 100_000});
        clamp_max_tokens(&mut body, None);
        assert_eq!(body["max_tokens"], 100_000);
    }
}
