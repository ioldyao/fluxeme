pub async fn responses(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let client_ip = extract_client_ip(&headers, addr);
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();
    let user = state.auth.authenticate(&headers)?;
    let model = trim_model(&mut body)?;

    if let Some(ref allowed) = user.allowed_models {
        if !allowed.contains(&model) {
            return Err(GatewayError::Auth(format!(
                "Model '{}' not allowed for this API key",
                model
            )));
        }
    }

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm).await?;
        state
            .rate_limiter
            .check_tpm(&user.user_id, tpm, estimate_tokens(&body))
            .await?;
    }

    let gw_cfg = state.gateway_config.read().unwrap().clone();

    let (channel_id, resolved_model, upstream_model) = state
        .routing
        .route(&user.user_id, &model, user.team_id.as_deref())
        .await?;
    let orig_model = if model != resolved_model {
        model.clone()
    } else {
        String::new()
    };
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }
    let mut route = resolve_route(&state, &channel_id)?;

    let is_streaming = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Broadcast route-decision event
    let accepted_at = Utc::now().to_rfc3339();
    state.event_bus.route_decided(RouteDecided {
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

    let body_str = serde_json::to_string(&body).unwrap_or_default();

    // ── Content filter check ──
    if state.content_filter.is_enabled() {
        match state
            .content_filter
            .check_request(&body_str, Some(&channel_id))
        {
            crate::service::moderation::FilterOutcome::Blocked(rule_name) => {
                state.flow_tracker.mark_completed(&request_id);
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

    let reservation = if gw_cfg.billing_enabled {
        let expires_at = (Utc::now() + chrono::Duration::minutes(2)).to_rfc3339();
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
            .map_err(|e| GatewayError::PaymentRequired(e.0))?,
        )
    } else {
        None
    };
    let reservation = reservation.map(|handle| {
        crate::service::token_reservation::ReservationFinalizer::new(state.db.clone(), handle)
    });

    if is_streaming {
        handle_responses_streaming(
            &state,
            &mut route,
            body,
            request_id,
            user.user_id,
            user.user_name,
            user.api_key_name,
            user.team_id,
            resolved_model,
            orig_model,
            start,
            client_ip,
            reservation.clone(),
        )
        .await
    } else {
        handle_responses_non_streaming(
            &state,
            &mut route,
            body,
            request_id,
            user.user_id,
            user.user_name,
            user.api_key_name,
            user.team_id,
            resolved_model,
            orig_model,
            start,
            client_ip,
            reservation,
        )
        .await
    }
}

/// Non-streaming POST /v1/responses — extract usage from the response body
/// Responses API format: {usage: {input_tokens, input_tokens_details: {cached_tokens}, output_tokens}}
async fn handle_responses_non_streaming(
    state: &AppState,
    route: &mut RouteTarget,
    body: Value,
    request_id: String,
    user_id: String,
    user_name: String,
    api_key_name: String,
    team_id: Option<String>,
    model: String,
    orig_model: String,
    start: Instant,
    client_ip: String,
    reservation: Option<crate::service::token_reservation::ReservationFinalizer>,
) -> Result<Response, GatewayError> {
    let req_body = serde_json::to_string(&body).ok();
    state
        .flow_tracker
        .mark_upstream_started(&request_id, Utc::now().to_rfc3339());

    let result = route
        .adapter
        .relay(&route.endpoint, "/v1/responses", body.clone())
        .await;

    match result {
        Ok(resp) => {
            route.report_success();
            let latency_ms = start.elapsed().as_millis() as u64;

            // Responses API usage: input_tokens, input_tokens_details.cached_tokens, output_tokens
            let input_tokens = resp["usage"]["input_tokens"].as_u64().unwrap_or(0);
            let output_tokens = resp["usage"]["output_tokens"].as_u64().unwrap_or(0);
            let cache_hit = resp["usage"]["input_tokens_details"]["cached_tokens"]
                .as_u64()
                .unwrap_or(0);
            let cache_write = resp["usage"]["input_tokens_details"]["cache_write_tokens"]
                .as_u64()
                .unwrap_or(0);
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

            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id: user_id,
                user_name: user_name,
                channel_id: route.channel_id.clone(),
                model,
                prompt_tokens: input_tokens,
                completion_tokens: output_tokens,
                total_tokens: input_tokens + output_tokens,
                cache_hit_input_tokens: cache_hit,
                cache_write_tokens: cache_write,
                latency_ms,
                status_code: 200,
                success: true,
                request_body: req_body,
                response_body: serde_json::to_string(&resp).ok(),
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
                original_model: orig_model,
                team_id: team_id.clone(),
                ttft_ms: None,
                account_type: Some("user".to_string()),
                billing_group_id: None,
                billing_group_name: None,
                billing_payment_mode: None,
            });

            Ok(Json(resp).into_response())
        }
        Err(e) if e.kind() == ErrorKind::ConnectFailed => {
            route.report_failure();
            if let Some(reservation) = &reservation {
                reservation.release("responses upstream connect failed");
            }
            let latency_ms = start.elapsed().as_millis() as u64;
            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id: user_id.clone(),
                user_name: user_name.clone(),
                channel_id: route.channel_id.clone(),
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
                response_body: Some(format!("{{\"error\":\"{}\"}}", e.0)),
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
                original_model: orig_model,
                team_id: team_id.clone(),
                ttft_ms: None,
                account_type: Some("user".to_string()),
                billing_group_id: None,
                billing_group_name: None,
                billing_payment_mode: None,
            });
            Err(GatewayError::Upstream(e.0))
        }
        Err(e) if is_retryable_error(&e) => {
            route.report_failure();
            if let Some(reservation) = &reservation {
                reservation.release("responses upstream retryable failure");
            }
            let latency_ms = start.elapsed().as_millis() as u64;
            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id: user_id.clone(),
                user_name: user_name.clone(),
                channel_id: route.channel_id.clone(),
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
                response_body: Some(format!("{{\"error\":\"{}\"}}", e.0)),
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
                original_model: orig_model,
                team_id: team_id.clone(),
                ttft_ms: None,
                account_type: Some("user".to_string()),
                billing_group_id: None,
                billing_group_name: None,
                billing_payment_mode: None,
            });
            Err(GatewayError::Upstream(e.0))
        }
        Err(e) => {
            if let Some(reservation) = &reservation {
                reservation.release("responses upstream request failed");
            }
            let latency_ms = start.elapsed().as_millis() as u64;
            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id: user_id.clone(),
                user_name: user_name.clone(),
                channel_id: route.channel_id.clone(),
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
                response_body: Some(format!("{{\"error\":\"{}\"}}", e.0)),
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
                original_model: orig_model,
                team_id: team_id.clone(),
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

/// Streaming POST /v1/responses — parse usage from the SSE response.completed event
async fn handle_responses_streaming(
    state: &AppState,
    route: &mut RouteTarget,
    mut body: Value,
    request_id: String,
    user_id: String,
    user_name: String,
    api_key_name: String,
    team_id: Option<String>,
    model: String,
    orig_model: String,
    start: Instant,
    client_ip: String,
    reservation: Option<crate::service::token_reservation::ReservationFinalizer>,
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
    state
        .flow_tracker
        .mark_upstream_started(&request_id, Utc::now().to_rfc3339());

    let stream_result = route
        .adapter
        .responses_stream(&route.endpoint, body.clone())
        .await;

    match stream_result {
        Ok(stream) => {
            route.report_success();

            // Wrap the stream to capture response.completed usage
            let resp_buf = Arc::new(std::sync::Mutex::new(String::new()));
            let recorded = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let clean_eof = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let usage_state = state.usage.clone();
            let rid = request_id.clone();
            let uid = user_id.clone();
            let uname = user_name.clone();
            let akn = api_key_name.clone();
            let chid = route.channel_id.clone();
            let mdl = model.clone();
            let orig_mdl = orig_model.clone();
            let st = start;
            let rbody = req_body.clone();
            let cip = client_ip.clone();
            let eid = route.endpoint.id;
            let eurl = route.endpoint.url.clone();
            let tid = team_id.clone();
            let reservation_finalizer = reservation.clone();
            let clean_eof2 = clean_eof.clone();
            let buf2 = resp_buf.clone();
            let rec2 = recorded.clone();

            let tracing_stream = stream.map(move |data| {
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
                    total_tokens: input_tokens + output_tokens,
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
            route.report_failure();
            if let Some(reservation) = &reservation {
                reservation.release("responses stream upstream failed");
            }
            let latency_ms = start.elapsed().as_millis() as u64;
            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id: user_id.clone(),
                user_name: user_name.clone(),
                channel_id: route.channel_id.clone(),
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
                response_body: Some(format!("{{\"error\":\"{}\"}}", e.0)),
                reasoning_body: None,
                api_key_name: Some(api_key_name.clone()),
                api_format: "openai".to_string(),
                stream: true,
                prompt_price: Decimal::ZERO,
                completion_price: Decimal::ZERO,
                cache_read_price: Decimal::ZERO,
                cache_write_price: Decimal::ZERO,
                client_ip: Some(client_ip),
                endpoint_id: route.endpoint.id,
                endpoint_url: Some(route.endpoint.url.clone()),
                original_model: orig_model,
                team_id: team_id.clone(),
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

/// Extract token usage from Responses API SSE events.
/// Looks for response.completed event with usage data.
/// Format: {response: {usage: {input_tokens, input_tokens_details: {cached_tokens}, output_tokens}}}
fn parse_responses_sse_usage(data: &str) -> (u64, u64, u64, u64) {
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


