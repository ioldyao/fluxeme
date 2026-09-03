
#[allow(clippy::too_many_arguments)]
async fn handle_streaming(
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
            let stream = SseBuffer {
                inner: stream,
                buf: String::new(),
                overflow_error: None,
            }
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
                team_id,
                upstream_started_at: Instant::now(),
                ttft_ms: None,
                endpoint_url: Some(endpoint.url.clone()),
                original_model: orig_model.clone(),
                balancer: Some(balancer),
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
            tracing::error!(
                request_id = %request_id,
                channel = %channel_id,
                model = %model,
                endpoint = %endpoint.url,
                error = %e.0,
                "Streaming upstream request failed",
            );
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
                api_format: "openai".to_string(),
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



#[allow(clippy::too_many_arguments)]
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
    orig_model: String,
    start: Instant,
    cache_key: Option<String>,
    cached_response: Option<Value>,
    client_ip: String,
    team_id: Option<String>,
    reservation: Option<crate::service::token_reservation::ReservationFinalizer>,
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    let mut cached_response = cached_response;
    if cached_response.is_none() {
        state
            .flow_tracker
            .mark_upstream_started(&request_id, Utc::now().to_rfc3339());
    }
    let max_retries = {
        let gw = state.gateway_config.read().unwrap();
        gw.max_retries
    };
    let mut retry_count = 0u32;
    let served_from_cache = cached_response.is_some();

    let err_msg: String = loop {
        let result = if let Some(response) = cached_response.take() {
            Ok(response)
        } else {
            route
                .adapter
                .chat_complete(&route.endpoint, body.clone())
                .await
        };

        match result {
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
                state.usage.record_with_endpoint(
                    UsageRecord {
                        timestamp: Utc::now().to_rfc3339(),
                        request_id,
                        user_id: user_id.clone(),
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
                        request_body: req_body.clone(),
                        response_body: serde_json::to_string(&resp).ok(),
                        reasoning_body: reasoning,
                        api_key_name: Some(api_key_name),
                        api_format: "openai".to_string(),
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
                    },
                    route.endpoint.id,
                );

                // Cache the response for non-streaming upstream requests.
                if !served_from_cache {
                    if let Some(ref ck) = cache_key {
                        if let Ok(body_str) = serde_json::to_string(&resp) {
                            let ttl = state.gateway_config.read().unwrap().cache_ttl_secs;
                            let _ = state.cache.set(&user_id, ck, &body_str, ttl).await;
                        }
                    }
                    route.report_success();
                }
                let mut response = Json(resp).into_response();
                response.headers_mut().insert(
                    "x-cache",
                    if served_from_cache {
                        HeaderValue::from_static("HIT")
                    } else {
                        HeaderValue::from_static("MISS")
                    },
                );
                return Ok(response);
            }
            Err(e) if e.kind() == ErrorKind::ConnectFailed => {
                // Connect failure: try next endpoint without consuming
                // retry budget. Only feed the breaker when the request
                // ultimately fails (no more endpoints to try).
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
                // Non-retryable (4xx etc.) — don't record failure on the breaker,
                // return immediately without retrying.
                let err_body = serde_json::json!({"error": {"message": &e.0}}).to_string();
                let latency_ms = start.elapsed().as_millis() as u64;
                state.usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id: request_id.clone(),
                    user_id: user_id.clone(),
                    user_name: user_name.clone(),
                    channel_id: channel_id.clone(),
                    model: model.clone(),
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
                    api_format: "openai".to_string(),
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
                tracing::error!(request_id = %request_id, endpoint = %route.endpoint.url, error = %e.0, "Upstream request failed");
                return Err(GatewayError::Upstream(e.0));
            }
        }
    };

    if let Some(reservation) = &reservation {
        reservation.release("upstream retries exhausted");
    }
    tracing::error!(
        request_id = %request_id,
        channel = %channel_id,
        model = %model,
        endpoint = %route.endpoint.url,
        error = %err_msg,
        retries = retry_count,
        "Upstream request retries exhausted",
    );
    // All retry attempts exhausted without success
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
        api_format: "openai".to_string(),
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



pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();

    let content_len = headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    // Read gateway config once — avoids 3-5 lock acquisitions per request
    let gw_cfg = state.gateway_config.read().unwrap().clone();

    let user = state.auth.authenticate(&headers)?;
    let model = trim_model(&mut body)?;

    let request_span = tracing::info_span!(
        "chat_completions",
        request_id = %request_id,
        user_id = %user.user_id,
        model = %model,
    );
    let _guard = request_span.enter();

    tracing::info!(request_id, user = %user.user_id, model = %model, content_length = %content_len, "Incoming request");

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm).await?;
        state
            .rate_limiter
            .check_tpm(&user.user_id, tpm, estimate_tokens(&body))
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
    let mut route = resolve_route_for_model(&state, &resolved_model, &channel_id, upstream_model.as_deref())?;

    let is_streaming = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let client_ip = extract_client_ip(&headers, addr);

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

    tracing::info!(request_id, channel = %channel_id, endpoint = %route.endpoint.url, "Routing resolved");

    // ── Serialize body once after all mutations (trim_model, upstream model override) ──
    let mut body_str = serde_json::to_string(&body).unwrap_or_default();

    // ── Content filter check (request body) ──
    if state.content_filter.is_enabled() {
        match state
            .content_filter
            .check_request(&body_str, Some(&channel_id))
        {
            crate::service::moderation::FilterOutcome::Blocked(rule_name) => {
                state.flow_tracker.mark_completed(&request_id);
                tracing::warn!(request_id, rule = %rule_name, "Request blocked by content filter");
                return Err(GatewayError::BadRequest(format!(
                    "Request blocked by content filter rule: {}",
                    rule_name
                )));
            }
            crate::service::moderation::FilterOutcome::Masked(masked) => {
                if let Ok(v) = serde_json::from_str(&masked) {
                    body = v;
                    body_str = masked;
                    tracing::info!(request_id, "Request body masked by content filter");
                }
            }
            crate::service::moderation::FilterOutcome::Pass => {}
        }
    }

    // ── Cache check (non-streaming only) ──
    // Keep a cache hit aside until after the reservation is created. Serving it
    // here would bypass both reservation settlement and the usage/billing fact.
    let (cache_key, cached_response) = if !is_streaming {
        let raw_key = format!("{}:{}", model, body_str);
        let hash = hex::encode(Sha256::digest(raw_key.as_bytes()));
        let cached_response = match state.cache.get(&user.user_id, &hash).await {
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

    // Ask the upstream for the final usage chunk in streaming responses.
    // OpenAI-compatible servers only send usage when
    // stream_options.include_usage is true — the gateway needs it for
    // token + cache-hit accounting. Non-streaming responses always carry
    // usage, so this is streaming-only.
    if is_streaming {
        match body.get_mut("stream_options") {
            Some(serde_json::Value::Object(opts)) => {
                opts.insert("include_usage".into(), serde_json::Value::Bool(true));
            }
            _ => {
                body["stream_options"] = serde_json::json!({"include_usage": true});
            }
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
                false,
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
            handle_streaming(
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
            handle_non_streaming(
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
                cache_key,
                cached_response,
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
                "Chat completions handler timed out"
            );
            Err(GatewayError::Upstream("Request timed out".into()))
        }
    }
}


