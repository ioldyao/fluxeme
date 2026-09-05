// ── Scheduler: unified request dispatch pipeline ──
//
// The scheduler owns the full request path after authentication and rate
// limiting: route decision (model → channel), endpoint selection (channel →
// endpoint), upstream execution with retry/fallback, circuit-breaker
// feedback, request caching, content filtering, token reservation, and usage
// recording. Handlers are thin shells that parse the request and call
// `SchedulerService::dispatch`.
//
// This is a pure refactor of the previously handler-scattered dispatch logic
// (server/handlers/{openai,anthropic,relay,responses,token_endpoints}.rs):
// behavior is intentionally unchanged.

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use axum::response::IntoResponse;
use chrono::Utc;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cache::RedisCache;
use crate::config::types::GatewayRuntimeConfig;
use crate::db::Database;
use crate::domain::user::AuthResult;
use crate::observability::event::RouteDecided;
use crate::observability::event_bus::EventBus;
use crate::observability::flow_tracker::FlowTracker;
use crate::observability::lifecycle::RequestLifecycle;
use crate::provider::ProviderRegistry;
use crate::service::moderation::FilterOutcome;
use crate::service::routing::RoutingService;
use crate::service::token_reservation::ReservationFinalizer;
use crate::service::usage::UsageService;

pub(crate) mod dispatch;
pub(crate) mod helpers;
pub(crate) mod stream;

pub use helpers::GatewayError;

/// Which upstream API format a request uses. Drives adapter method selection,
/// body normalization, caching, and usage `api_format`.
#[derive(Debug, Clone)]
pub enum DispatchFormat {
    /// POST /v1/chat/completions
    OpenaiChat,
    /// POST /v1/messages (native Anthropic format)
    AnthropicMessages,
    /// POST /v1/completions, /v1/embeddings, /v1/messages/batches, /tokenize, /detokenize
    Relay { path: String },
    /// POST /v1/responses
    Responses,
    /// POST /v1/messages/count_tokens
    CountTokens,
    /// POST /responses/input_tokens
    ResponsesInputTokens,
}

/// Everything the scheduler needs to run one request through the pipeline.
pub struct DispatchRequest {
    /// Authenticated caller context (user_id, team, billing, scopes, …).
    pub auth: AuthResult,
    /// Trimmed logical model name as requested by the client.
    pub model: String,
    /// Request body (already trimmed of whitespace-model).
    pub body: Value,
    pub stream: bool,
    pub request_id: String,
    pub start: Instant,
    pub client_ip: String,
    pub format: DispatchFormat,
    /// Exactly-once request lifecycle created by the authenticated handler.
    pub lifecycle: Arc<RequestLifecycle>,
}

pub struct SchedulerService {
    routing: Arc<RoutingService>,
    providers: Arc<ProviderRegistry>,
    db: Arc<Database>,
    cache: Arc<RedisCache>,
    usage: UsageService,
    flow_tracker: FlowTracker,
    event_bus: EventBus,
    content_filter: Arc<crate::service::moderation::ContentFilterService>,
    gateway_config: Arc<RwLock<GatewayRuntimeConfig>>,
}

impl SchedulerService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        routing: Arc<RoutingService>,
        providers: Arc<ProviderRegistry>,
        db: Arc<Database>,
        cache: Arc<RedisCache>,
        usage: UsageService,
        flow_tracker: FlowTracker,
        event_bus: EventBus,
        content_filter: Arc<crate::service::moderation::ContentFilterService>,
        gateway_config: Arc<RwLock<GatewayRuntimeConfig>>,
    ) -> Self {
        Self {
            routing,
            providers,
            db,
            cache,
            usage,
            flow_tracker,
            event_bus,
            content_filter,
            gateway_config,
        }
    }

    /// Run the full request pipeline and return the gateway response.
    pub async fn dispatch(
        &self,
        req: DispatchRequest,
    ) -> Result<axum::response::Response, GatewayError> {
        let DispatchRequest {
            auth,
            model,
            mut body,
            stream,
            request_id,
            start,
            client_ip,
            format,
            lifecycle,
        } = req;
        let gw_cfg = self.gateway_config.read().unwrap().clone();
        let handler_timeout = Duration::from_secs(gw_cfg.handler_timeout_secs);
        let token_format = matches!(
            format,
            DispatchFormat::CountTokens | DispatchFormat::ResponsesInputTokens
        );

        // ── 1. Route decision ──
        let route = match self
            .routing
            .route_public(&auth.user_id, &model, auth.team_id.as_deref())
            .await
        {
            Ok(route) => route,
            Err(e) => {
                let classified =
                    crate::scheduler::helpers::ClassifiedError::from(GatewayError::from(e));
                lifecycle.finalize_classified(&classified);
                return Err(classified.into_gateway());
            }
        };
        let resolved_model = route.resolved_model;
        let upstream_model = route.upstream_model;
        let channel_scope = route.channel_scope;
        helpers::authorize_effective_model(&auth, &resolved_model).map_err(|e| {
            let classified = helpers::ClassifiedError::from(e);
            lifecycle.finalize_classified(&classified);
            classified.into_gateway()
        })?;
        let orig_model = if model != resolved_model {
            model.clone()
        } else {
            String::new()
        };

        // ── 2. Format-specific body normalization ──
        match &format {
            DispatchFormat::AnthropicMessages | DispatchFormat::CountTokens => {
                helpers::normalize_messages_body(&mut body);
            }
            DispatchFormat::Relay { .. } => {
                if let Some(obj) = body.as_object_mut() {
                    obj.remove("stream");
                    obj.remove("stream_options");
                }
            }
            _ => {}
        }

        // ── 3. Endpoint selection (flattened model endpoint pool) ──
        let mut dispatch = match dispatch::resolve_dispatch(
            self,
            &resolved_model,
            channel_scope.as_deref(),
            upstream_model.as_deref(),
        ) {
            Ok(dispatch) => dispatch,
            Err(e) => {
                let classified = helpers::ClassifiedError::from(e);
                lifecycle.finalize_classified(&classified);
                return Err(classified.into_gateway());
            }
        };

        // Route facts for the request lifecycle (resolved model, channel,
        // endpoint, provider). Set once the endpoint is selected; the request
        // event carries these even when a later step fails.
        lifecycle.set_route(
            resolved_model.clone(),
            Some(dispatch.channel_id.clone()),
            dispatch.endpoint.id,
            Some(dispatch.endpoint.url.clone()),
            dispatch.upstream_model.clone(),
            dispatch
                .runtime
                .endpoints
                .get(dispatch.endpoint_idx)
                .map(|state| state.provider.clone()),
        );

        // The upstream alias travels with the selected endpoint (a model may
        // bind channels that expose different upstream names).
        if let Some(ref id) = dispatch.upstream_model {
            body["model"] = Value::String(id.clone());
        }

        // Apply the selected endpoint's upstream output cap before filtering,
        // caching, reservation, and the upstream request. Keep the original
        // numeric request value so a retry to another endpoint re-applies
        // that endpoint's own cap.
        dispatch.requested_max_tokens = body.get("max_tokens").and_then(Value::as_u64);
        dispatch::clamp_max_tokens(&mut body, dispatch.endpoint.max_tokens);

        // Anthropic-compat OpenAI channels accept /v1/messages and convert.
        if matches!(format, DispatchFormat::AnthropicMessages) {
            if let Some(ref ch) = self.routing.get_channel(&dispatch.channel_id) {
                if ch.anthropic_compat && ch.provider == "openai" {
                    dispatch.adapter = Arc::new(
                        crate::provider::anthropic_compat::AnthropicCompatAdapter::new(
                            dispatch.adapter.clone(),
                        ),
                    );
                }
            }
        }

        // ── 4. Broadcast route decision ──
        let accepted_at = Utc::now().to_rfc3339();
        self.event_bus.route_decided(RouteDecided {
            event_type: "route_decided".to_string(),
            timestamp: accepted_at.clone(),
            request_id: request_id.clone(),
            model: resolved_model.clone(),
            channel_id: dispatch.channel_id.clone(),
            endpoint_id: dispatch.endpoint.id,
            user_id: auth.user_id.clone(),
        });
        self.flow_tracker.mark_accepted(
            request_id.clone(),
            resolved_model.clone(),
            dispatch.channel_id.clone(),
            dispatch.endpoint.id,
            accepted_at,
        );

        // ── 5. Format-specific channel capability checks ──
        match &format {
            DispatchFormat::CountTokens => {
                let channel = self.routing.get_channel(&dispatch.channel_id);
                if !dispatch::count_tokens_supported_for_channel(channel.as_ref()) {
                    return Err(helpers::finalize_and_fail(
                        &lifecycle,
                        GatewayError::BadRequest(
                            "POST /v1/messages/count_tokens is not supported for anthropic_compat OpenAI channels yet"
                                .into(),
                        ),
                    ));
                }
            }
            DispatchFormat::ResponsesInputTokens => {
                let channel = self.routing.get_channel(&dispatch.channel_id);
                if !dispatch::responses_input_tokens_supported_for_channel(channel.as_ref()) {
                    return Err(helpers::finalize_and_fail(
                        &lifecycle,
                        GatewayError::BadRequest(
                            "POST /responses/input_tokens is only supported for OpenAI-compatible channels"
                                .into(),
                        ),
                    ));
                }
            }
            _ => {}
        }

        // ── 6. Content filter (request body) ──
        let mut body_str = serde_json::to_string(&body).unwrap_or_default();
        if self.content_filter.is_enabled() {
            match self
                .content_filter
                .check_request(&body_str, Some(&dispatch.channel_id))
            {
                FilterOutcome::Blocked(rule_name) => {
                    self.flow_tracker.mark_completed(&request_id);
                    tracing::warn!(request_id, rule = %rule_name, "Request blocked by content filter");
                    let classified = helpers::ClassifiedError::new(
                        GatewayError::BadRequest(format!(
                            "Request blocked by content filter rule: {}",
                            rule_name
                        )),
                        400,
                        "guardrail",
                        "content_blocked",
                    );
                    lifecycle.finalize_classified(&classified);
                    return Err(classified.into_gateway());
                }
                FilterOutcome::Masked(masked) => {
                    if let Ok(v) = serde_json::from_str(&masked) {
                        body = v;
                        body_str = masked;
                    }
                }
                FilterOutcome::Pass => {}
            }
        }

        // ── 7. Request cache check (OpenAI chat non-streaming only) ──
        let (cache_key, cached_response) = if !stream
            && matches!(format, DispatchFormat::OpenaiChat)
        {
            let raw_key = format!("{}:{}", model, body_str);
            let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
            let cached_response = match self.cache.get(&auth.user_id, &hash).await {
                Ok(Some(cached)) => match serde_json::from_str::<Value>(&cached) {
                    Ok(value)
                        if value["usage"]["prompt_tokens"].is_u64()
                            && value["usage"]["completion_tokens"].is_u64() =>
                    {
                        tracing::info!(request_id, "Cache HIT for model {}", model);
                        Some(value)
                    }
                    Ok(_) => {
                        // A cached response without usage cannot be billed safely;
                        // fall through to upstream rather than serving it for free.
                        tracing::warn!(request_id, "Ignoring cache entry without complete usage");
                        None
                    }
                    Err(e) => {
                        tracing::warn!(request_id, "Invalid cached response: {}", e);
                        None
                    }
                },
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(request_id, "Cache GET error: {}", e);
                    None
                }
            };
            (Some(hash), cached_response)
        } else {
            (None, None)
        };

        // ── 8. stream_options include_usage (streaming chat/responses) ──
        if stream
            && matches!(
                format,
                DispatchFormat::OpenaiChat | DispatchFormat::Responses
            )
        {
            match body.get_mut("stream_options") {
                Some(Value::Object(opts)) => {
                    opts.insert("include_usage".into(), Value::Bool(true));
                }
                _ => {
                    body["stream_options"] = serde_json::json!({"include_usage": true});
                }
            }
        }

        // ── 9. Token reservation (billing formats) ──
        let bills = matches!(
            format,
            DispatchFormat::OpenaiChat
                | DispatchFormat::AnthropicMessages
                | DispatchFormat::Relay { .. }
                | DispatchFormat::Responses
        );
        let reservation = if gw_cfg.billing_enabled && bills {
            let expires_at = match &format {
                DispatchFormat::Relay { .. } | DispatchFormat::Responses => {
                    (Utc::now() + chrono::Duration::minutes(2)).to_rfc3339()
                }
                _ => (Utc::now()
                    + chrono::Duration::seconds(handler_timeout.as_secs() as i64 + 60))
                .to_rfc3339(),
            };
            let anthropic_reserve = matches!(format, DispatchFormat::AnthropicMessages);
            Some(
                crate::service::token_reservation::reserve(
                    self.db.clone(),
                    &request_id,
                    &auth.user_id,
                    &auth.user_name,
                    &auth.api_key_name,
                    auth.team_id.as_deref(),
                    &auth.billing_group_id,
                    "",
                    auth.billing_payment_mode,
                    &resolved_model,
                    &body,
                    anthropic_reserve,
                    &expires_at,
                )
                .await
                .map_err(|e| {
                    self.flow_tracker.mark_completed(&request_id);
                    let classified =
                        helpers::ClassifiedError::from(GatewayError::PaymentRequired(e.0.clone()));
                    lifecycle.finalize_classified(&classified);
                    classified.into_gateway()
                })?,
            )
        } else {
            None
        };
        let timeout_reservation = reservation.clone();
        let reservation_finalizer =
            reservation.map(|handle| ReservationFinalizer::new(self.db.clone(), handle));

        // ── 10. Execute with timeout ──
        let ctx = dispatch::DispatchCtx {
            request_id: request_id.clone(),
            user_id: auth.user_id.clone(),
            user_name: auth.user_name.clone(),
            api_key_name: auth.api_key_name.clone(),
            channel_id: dispatch.channel_id.clone(),
            model: resolved_model.clone(),
            orig_model: orig_model.clone(),
            start,
            client_ip: client_ip.clone(),
            team_id: auth.team_id.clone(),
            account_type: auth
                .team_id
                .as_ref()
                .map(|_| "team")
                .or(Some("user"))
                .map(String::from),
        };
        let rid = request_id.clone();
        let max_retries = gw_cfg.max_retries;
        let lifecycle_cloned = lifecycle.clone();
        let result = tokio::time::timeout(handler_timeout, async move {
            match format {
                DispatchFormat::OpenaiChat if stream => {
                    self.exec_openai_stream(
                        ctx,
                        dispatch.adapter,
                        dispatch.endpoint,
                        dispatch.runtime.clone(),
                        dispatch.endpoint_idx,
                        body,
                        reservation_finalizer,
                        lifecycle_cloned.clone(),
                    )
                    .await
                }
                DispatchFormat::OpenaiChat => {
                    self.exec_openai_non_stream(
                        ctx,
                        &mut dispatch,
                        body,
                        cache_key,
                        cached_response,
                        reservation_finalizer,
                        &lifecycle_cloned,
                    )
                    .await
                }
                DispatchFormat::AnthropicMessages if stream => {
                    self.exec_messages_stream(
                        ctx,
                        dispatch.adapter,
                        dispatch.endpoint,
                        dispatch.runtime.clone(),
                        dispatch.endpoint_idx,
                        body,
                        reservation_finalizer,
                        lifecycle_cloned.clone(),
                    )
                    .await
                }
                DispatchFormat::AnthropicMessages => {
                    self.exec_messages_non_stream(
                        ctx,
                        &mut dispatch,
                        body,
                        reservation_finalizer,
                        &lifecycle_cloned,
                    )
                    .await
                }
                DispatchFormat::Relay { path } => {
                    self.exec_relay(
                        ctx,
                        &mut dispatch,
                        body,
                        &path,
                        reservation_finalizer,
                        &lifecycle_cloned,
                    )
                    .await
                }
                DispatchFormat::Responses if stream => {
                    self.exec_responses_stream(
                        ctx,
                        &mut dispatch,
                        body,
                        reservation_finalizer,
                        lifecycle_cloned.clone(),
                    )
                    .await
                }
                DispatchFormat::Responses => {
                    self.exec_responses_non_stream(
                        ctx,
                        &mut dispatch,
                        body,
                        reservation_finalizer,
                        &lifecycle_cloned,
                    )
                    .await
                }
                DispatchFormat::CountTokens => {
                    let value = self
                        .exec_count_tokens(
                            &mut dispatch,
                            body,
                            &ctx.request_id,
                            max_retries,
                            &lifecycle_cloned,
                        )
                        .await?;
                    lifecycle_cloned.finalize_success();
                    Ok(axum::response::Json(value).into_response())
                }
                DispatchFormat::ResponsesInputTokens => {
                    let value = self
                        .exec_responses_input_tokens(
                            &mut dispatch,
                            body,
                            &ctx.request_id,
                            max_retries,
                            &lifecycle_cloned,
                        )
                        .await?;
                    lifecycle_cloned.finalize_success();
                    Ok(axum::response::Json(value).into_response())
                }
            }
        })
        .await;

        match result {
            Ok(Ok(inner)) => {
                // Token-counting endpoints don't record usage, so the flow
                // lifecycle is closed explicitly here.
                if token_format {
                    self.flow_tracker.mark_completed(&rid);
                }
                Ok(inner)
            }
            Ok(Err(e)) => {
                // Upstream / detected failure returned by an executor. Stream
                // executors may already have finalized this exact outcome; the
                // lifecycle's exactly-once guard makes a second finalize a
                // no-op, so the centralized classification below is always safe.
                let classified = helpers::ClassifiedError::from(e);
                lifecycle.finalize_classified(&classified);
                Err(classified.into_gateway())
            }
            Err(_) => {
                if let Some(handle) = timeout_reservation {
                    ReservationFinalizer::new(self.db.clone(), handle).release("handler timeout");
                }
                self.flow_tracker.mark_completed(&rid);
                tracing::error!(
                    rid,
                    handler_timeout_s = handler_timeout.as_secs(),
                    "Gateway handler timed out"
                );
                // Timeout is a gateway-level failure (504), distinct from an
                // upstream error (502).
                let classified = helpers::ClassifiedError::new(
                    GatewayError::Upstream("Request timed out".into()),
                    504,
                    "gateway",
                    "overall_timeout",
                );
                lifecycle.finalize_classified(&classified);
                Err(classified.into_gateway())
            }
        }
    }
}
