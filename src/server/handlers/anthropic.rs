// ── Messages streaming (Anthropic-native format) ──────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_messages_streaming(
    state: &AppState,
    adapter: Arc<dyn crate::provider::ProviderAdapter>,
    endpoint: EndpointConfig,
    balancer: Arc<LoadBalancer>,
    endpoint_idx: usize,
    body: Value,
    request_id: String,
    user_id: String,
    user_name: String,
    api_key_name: String,
    channel_id: String,
    model: String,
    orig_model: String,
    start: Instant,
    client_ip: String,
    team_id: Option<String>,
    reservation: Option<crate::service::token_reservation::ReservationFinalizer>,
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    state
        .flow_tracker
        .mark_upstream_started(&request_id, Utc::now().to_rfc3339());
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
            let stream = SseBuffer {
                inner: stream,
                buf: String::new(),
                overflow_error: None,
            };
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
                endpoint_url: Some(endpoint.url.clone()),
                original_model: orig_model.clone(),
                balancer: Some(balancer),
                team_id,
                upstream_started_at: Instant::now(),
                ttft_ms: None,
                endpoint_idx,
                reservation,
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
            balancer.as_health_aware().record_failure(endpoint_idx);
            let err_body = serde_json::json!({"error": {"message": &e.0}}).to_string();
            let latency_ms = start.elapsed().as_millis() as u64;
            let (p_tokens, c_tokens, cache_hit, cache_write) = parse_sse_usage("");
            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id,
                user_name,
                channel_id,
                model,
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
                api_key_name: Some(api_key_name),
                api_format: "anthropic".to_string(),
                stream: true,
                prompt_price: Decimal::ZERO,
                completion_price: Decimal::ZERO,
                cache_read_price: Decimal::ZERO,
                cache_write_price: Decimal::ZERO,
                client_ip: Some(client_ip),
                endpoint_id: endpoint.id,
                endpoint_url: Some(endpoint.url.clone()),
                original_model: orig_model.clone(),
                team_id: team_id.clone(),
                ttft_ms: None,
                account_type: team_id
                    .as_ref()
                    .map(|_| "team")
                    .or(Some("user"))
                    .map(String::from),
                billing_group_id: None,
                billing_group_name: None,
                billing_payment_mode: None,
            });
            Err(GatewayError::Upstream(e.0))
        }
    }
}


// ── Messages non-streaming (Anthropic-native format) ──────────────

#[allow(clippy::too_many_arguments)]
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
    orig_model: String,
    start: Instant,
    client_ip: String,
    team_id: Option<String>,
    reservation: Option<crate::service::token_reservation::ReservationFinalizer>,
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    state
        .flow_tracker
        .mark_upstream_started(&request_id, Utc::now().to_rfc3339());
    let max_retries = {
        let gw = state.gateway_config.read().unwrap();
        gw.max_retries
    };
    let mut retry_count = 0u32;

    let err_msg: String = loop {
        let result = route.adapter.messages(&route.endpoint, body.clone()).await;

        match result {
            Ok(resp) => {
                route.report_success();
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

                let latency_ms = start.elapsed().as_millis() as u64;
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
                state.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id,
                    user_id,
                    user_name,
                    channel_id,
                    model,
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
                    api_key_name: Some(api_key_name),
                    api_format: "anthropic".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(client_ip.clone()),
                    endpoint_id: route.endpoint.id,
                    endpoint_url: Some(route.endpoint.url.clone()),
                    original_model: orig_model.clone(),
                    team_id: team_id.clone(),
                    ttft_ms: None,
                    account_type: team_id
                        .as_ref()
                        .map(|_| "team")
                        .or(Some("user"))
                        .map(String::from),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });

                return Ok(Json(resp).into_response());
            }
            Err(e) if e.kind() == ErrorKind::ConnectFailed => {
                if !route.retry_next() {
                    route.report_failure();
                    break e.0;
                }
                continue;
            }
            Err(e) if is_retryable_error(&e) => {
                if retry_count >= max_retries {
                    route.report_failure();
                    break e.0;
                }
                retry_count += 1;
                if !route.retry_next() {
                    route.report_failure();
                    break e.0;
                }
            }
            Err(e) => {
                if let Some(reservation) = &reservation {
                    reservation.release("upstream non-retryable error");
                }
                let err_body = serde_json::json!({"error": {"message": &e.0}}).to_string();
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
                    cache_write_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body.clone(),
                    response_body: Some(err_body),
                    reasoning_body: None,
                    api_key_name: Some(api_key_name.clone()),
                    api_format: "anthropic".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(client_ip.clone()),
                    endpoint_id: route.endpoint.id,
                    endpoint_url: Some(route.endpoint.url.clone()),
                    original_model: orig_model.clone(),
                    team_id: team_id.clone(),
                    ttft_ms: None,
                    account_type: team_id
                        .as_ref()
                        .map(|_| "team")
                        .or(Some("user"))
                        .map(String::from),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });
                tracing::error!(request_id = %request_id, endpoint = %route.endpoint.url, error = %e.0, "Messages upstream request failed");
                return Err(GatewayError::Upstream(e.0));
            }
        }
    };

    let latency_ms = start.elapsed().as_millis() as u64;
    let err_body = serde_json::json!({"error": {"message": &err_msg}}).to_string();
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
        cache_write_tokens: 0,
        latency_ms,
        status_code: 502,
        success: false,
        request_body: req_body,
        response_body: Some(err_body),
        reasoning_body: None,
        api_key_name: Some(api_key_name.clone()),
        api_format: "anthropic".to_string(),
        stream: false,
        prompt_price: Decimal::ZERO,
        completion_price: Decimal::ZERO,
        cache_read_price: Decimal::ZERO,
        cache_write_price: Decimal::ZERO,
        client_ip: Some(client_ip),
        endpoint_id: route.endpoint.id,
        endpoint_url: Some(route.endpoint.url.clone()),
        original_model: orig_model.clone(),
        team_id: team_id.clone(),
        ttft_ms: None,
        account_type: team_id
            .as_ref()
            .map(|_| "team")
            .or(Some("user"))
            .map(String::from),
        billing_group_id: None,
        billing_group_name: None,
        billing_payment_mode: None,
    });
    Err(GatewayError::Upstream(err_msg))
}


// Importers/callers: registered in src/server/mod.rs as the public HTTP route
// handler for POST /v1/messages and mirrored by the new POST
// /v1/messages/count_tokens handler. Affected API: Anthropic-native endpoints.
// Data schema: Anthropic request body and message/count_tokens responses.
// User instruction: "要，添加`/v1/messages/count_tokens`的端点支持".
pub async fn messages_count_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(_addr): ConnectInfo<SocketAddr>,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();

    let gw_cfg = state.gateway_config.read().unwrap().clone();

    let user = state.auth.authenticate(&headers)?;
    let model = trim_model(&mut body)?;

    let request_span = tracing::info_span!(
        "messages_count_tokens",
        request_id = %request_id,
        user_id = %user.user_id,
        model = %model,
    );
    let _guard = request_span.enter();

    tracing::info!(request_id, user = %user.user_id, model = %model, "Incoming messages count_tokens request");

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm).await?;
        state
            .rate_limiter
            .check_tpm(&user.user_id, tpm, estimate_tokens_anthropic(&body))
            .await?;
    }

    let (channel_id, resolved_model, upstream_model) = state
        .routing
        .route_public(&user.user_id, &model, user.team_id.as_deref())
        .await?;
    authorize_effective_model(&user, &resolved_model)?;
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }

    normalize_messages_body(&mut body);
    let mut route = resolve_route_for_model(&state, &resolved_model, &channel_id, upstream_model.as_deref())?;

    state.event_bus.route_decided(RouteDecided {
        event_type: "route_decided".to_string(),
        timestamp: Utc::now().to_rfc3339(),
        request_id: request_id.clone(),
        model: resolved_model.clone(),
        channel_id: channel_id.clone(),
        endpoint_id: route.endpoint.id,
        user_id: user.user_id.clone(),
    });

    let channel = state.routing.get_channel(&channel_id);
    if !count_tokens_supported_for_channel(channel.as_ref()) {
        return Err(GatewayError::BadRequest(
            "POST /v1/messages/count_tokens is not supported for anthropic_compat OpenAI channels yet"
                .into(),
        ));
    }

    let accepted_at = Utc::now().to_rfc3339();
    state.flow_tracker.mark_accepted(
        request_id.clone(),
        resolved_model.clone(),
        channel_id.clone(),
        route.endpoint.id,
        accepted_at,
    );

    let body_str = serde_json::to_string(&body).unwrap_or_default();
    if state.content_filter.is_enabled() {
        match state
            .content_filter
            .check_request(&body_str, Some(&channel_id))
        {
            crate::service::moderation::FilterOutcome::Blocked(rule_name) => {
                state.flow_tracker.mark_completed(&request_id);
                tracing::warn!(request_id, rule = %rule_name, "Messages count_tokens request blocked by content filter");
                return Err(GatewayError::BadRequest(format!(
                    "Request blocked by content filter rule: {}",
                    rule_name
                )));
            }
            crate::service::moderation::FilterOutcome::Masked(masked) => {
                if let Ok(v) = serde_json::from_str(&masked) {
                    body = v;
                    tracing::info!(
                        request_id,
                        "Messages count_tokens request body masked by content filter"
                    );
                }
            }
            crate::service::moderation::FilterOutcome::Pass => {}
        }
    }

    let handler_timeout = Duration::from_secs(gw_cfg.handler_timeout_secs);
    let rid = request_id.clone();
    state
        .flow_tracker
        .mark_upstream_started(&request_id, Utc::now().to_rfc3339());
    let result = tokio::time::timeout(handler_timeout, async move {
        handle_messages_count_tokens(&mut route, body, &request_id, gw_cfg.max_retries).await
    })
    .await;
    state.flow_tracker.mark_completed(&rid);

    match result {
        Ok(inner) => Ok(Json(inner?).into_response()),
        Err(_) => {
            tracing::error!(
                rid,
                handler_timeout_s = handler_timeout.as_secs(),
                "Messages count_tokens handler timed out"
            );
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

    // Read gateway config once
    let gw_cfg = state.gateway_config.read().unwrap().clone();

    let user = state.auth.authenticate(&headers)?;
    let model = trim_model(&mut body)?;

    let request_span = tracing::info_span!(
        "messages",
        request_id = %request_id,
        user_id = %user.user_id,
        model = %model,
    );
    let _guard = request_span.enter();

    tracing::info!(request_id, user = %user.user_id, model = %model, "Incoming messages request");

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm).await?;
        state
            .rate_limiter
            .check_tpm(&user.user_id, tpm, estimate_tokens_anthropic(&body))
            .await?;
    }

    let (channel_id, resolved_model, upstream_model) = state
        .routing
        .route_public(&user.user_id, &model, user.team_id.as_deref())
        .await?;
    authorize_effective_model(&user, &resolved_model)?;
    let orig_model = if model != resolved_model {
        model.clone()
    } else {
        String::new()
    };
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }
    // Normalize Claude-Code-style inline system messages to the Anthropic
    // top-level "system" field.  SGLang's /v1/messages rejects role=system
    // in the messages array (only "user"/"assistant" are allowed).
    normalize_messages_body(&mut body);
    let mut route = resolve_route_for_model(&state, &resolved_model, &channel_id, upstream_model.as_deref())?;

    // Broadcast route-decision event immediately so the admin UI shows
    // the request as "in-flight" before the upstream call completes.
    let accepted_at = Utc::now().to_rfc3339();
    state.event_bus.route_decided(RouteDecided {
        event_type: "route_decided".to_string(),
        timestamp: accepted_at.clone(),
        request_id: request_id.clone(),
        model: resolved_model.clone(),
        channel_id: channel_id.clone(),
        endpoint_id: route.endpoint.id,
        user_id: user.user_id.clone(),
    });
    state.flow_tracker.mark_accepted(
        request_id.clone(),
        resolved_model.clone(),
        channel_id.clone(),
        route.endpoint.id,
        accepted_at,
    );

    // If the resolved channel has anthropic_compat enabled (OpenAI provider
    // accepting Anthropic-format requests), wrap the adapter so that
    // messages()/messages_stream() transparently convert between formats.
    if let Some(ref ch) = state.routing.get_channel(&channel_id) {
        if ch.anthropic_compat && ch.provider == "openai" {
            route.adapter = Arc::new(
                crate::provider::anthropic_compat::AnthropicCompatAdapter::new(
                    route.adapter.clone(),
                ),
            );
        }
    }

    let is_streaming = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let client_ip = extract_client_ip(&headers, addr);

    tracing::info!(request_id, channel = %channel_id, endpoint = %route.endpoint.url, "Messages routing resolved");

    // ── Serialize body once after all mutations ──
    let body_str = serde_json::to_string(&body).unwrap_or_default();

    // ── Content filter check (request body) ──
    if state.content_filter.is_enabled() {
        match state
            .content_filter
            .check_request(&body_str, Some(&channel_id))
        {
            crate::service::moderation::FilterOutcome::Blocked(rule_name) => {
                state.flow_tracker.mark_completed(&request_id);
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

    let handler_timeout = Duration::from_secs(gw_cfg.handler_timeout_secs);
    let reservation = if gw_cfg.billing_enabled {
        let expires_at = (Utc::now()
            + chrono::Duration::seconds(handler_timeout.as_secs() as i64 + 60))
        .to_rfc3339();
        Some(
            crate::service::token_reservation::reserve(
                state.db.clone(),
                &request_id,
                &user.user_id,
                &user.user_name,
                &user.api_key_name,
                user.team_id.as_deref(),
                &user.billing_group_id,
                "",
                user.billing_payment_mode,
                &resolved_model,
                &body,
                true,
                &expires_at,
            )
            .await
            .map_err(|e| {
                state.flow_tracker.mark_completed(&request_id);
                GatewayError::PaymentRequired(e.0)
            })?,
        )
    } else {
        None
    };
    let timeout_reservation = reservation.clone();
    let state_clone = state.clone();
    let rid = request_id.clone();
    let client_ip_clone = client_ip.clone();

    let result = tokio::time::timeout(handler_timeout, async move {
        if is_streaming {
            handle_messages_streaming(
                &state_clone,
                route.adapter,
                route.endpoint,
                route.balancer.clone(),
                route.endpoint_idx,
                body,
                request_id,
                user.user_id,
                user.user_name,
                user.api_key_name,
                route.channel_id.clone(),
                resolved_model,
                orig_model,
                start,
                client_ip,
                user.team_id.clone(),
                reservation.clone().map(|handle| {
                    crate::service::token_reservation::ReservationFinalizer::new(
                        state_clone.db.clone(),
                        handle,
                    )
                }),
            )
            .await
        } else {
            let ch_id = route.channel_id.clone();
            handle_messages_non_streaming(
                &state_clone,
                &mut route,
                body,
                request_id,
                user.user_id,
                user.user_name,
                user.api_key_name,
                ch_id,
                resolved_model,
                orig_model,
                start,
                client_ip_clone,
                user.team_id.clone(),
                reservation.map(|handle| {
                    crate::service::token_reservation::ReservationFinalizer::new(
                        state_clone.db.clone(),
                        handle,
                    )
                }),
            )
            .await
        }
    })
    .await;

    match result {
        Ok(inner) => inner,
        Err(_) => {
            if let Some(handle) = timeout_reservation {
                crate::service::token_reservation::ReservationFinalizer::new(
                    state.db.clone(),
                    handle,
                )
                .release("handler timeout");
            }
            state.flow_tracker.mark_completed(&rid);
            tracing::error!(
                rid,
                handler_timeout_s = handler_timeout.as_secs(),
                "Messages handler timed out"
            );
            Err(GatewayError::Upstream("Request timed out".into()))
        }
    }
}


