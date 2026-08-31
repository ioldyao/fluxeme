// ── Relay ─────────────────────────────────────────────────────────

#[allow(clippy::option_map_unit_fn)]
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

    // Relay is always non-streaming — strip streaming fields so
    // upstreams don't return SSE which relay() cannot parse, and
    // don't reject stream_options without stream (400 Bad Request).
    body.as_object_mut().map(|o| {
        o.remove("stream");
        o.remove("stream_options");
    });

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
    let mut route = resolve_route_for_model(state, &resolved_model, &channel_id)?;

    // Broadcast route-decision event so the admin UI shows
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

    // ── Serialize body once (after all mutations), used for content filter + req_body ──
    let mut body_str = serde_json::to_string(&body).unwrap_or_default();

    // ── Content filter check (request body) ──
    if state.content_filter.is_enabled() {
        match state
            .content_filter
            .check_request(&body_str, Some(&channel_id))
        {
            crate::service::moderation::FilterOutcome::Blocked(rule_name) => {
                state.flow_tracker.mark_completed(&request_id);
                tracing::warn!(request_id, rule = %rule_name, "Relay request blocked by content filter");
                return Err(GatewayError::BadRequest(format!(
                    "Request blocked by content filter rule: {}",
                    rule_name
                )));
            }
            crate::service::moderation::FilterOutcome::Masked(masked) => {
                if let Ok(v) = serde_json::from_str(&masked) {
                    body = v;
                    body_str = masked;
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
            .map_err(|e| {
                state.flow_tracker.mark_completed(&request_id);
                GatewayError::PaymentRequired(e.0)
            })?,
        )
    } else {
        None
    };
    let reservation_finalizer = reservation.map(|handle| {
        crate::service::token_reservation::ReservationFinalizer::new(state.db.clone(), handle)
    });
    let req_body = Some(body_str);
    state
        .flow_tracker
        .mark_upstream_started(&request_id, Utc::now().to_rfc3339());
    let mut retry_count = 0u32;

    let err_msg: String = loop {
        let result = route
            .adapter
            .relay(&route.endpoint, upstream_path, body.clone())
            .await;

        match result {
            Ok(mut resp) => {
                route.report_success();
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
                if let Some(reservation) = &reservation_finalizer {
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
                    user_id: user.user_id,
                    user_name: user.user_name,
                    channel_id: route.channel_id,
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
                    api_key_name: Some(user.api_key_name.clone()),
                    api_format: "relay".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(client_ip.clone()),
                    endpoint_id: route.endpoint.id,
                    endpoint_url: Some(route.endpoint.url.clone()),
                    original_model: orig_model.clone(),
                    team_id: user.team_id.clone(),
                    ttft_ms: None,
                    account_type: user
                        .team_id
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
                route.report_failure();
                if !route.retry_next() {
                    break e.0;
                }
                continue;
            }
            Err(e) if is_retryable_error(&e) => {
                route.report_failure();
                if retry_count >= gw_cfg.max_retries {
                    if let Some(reservation) = &reservation_finalizer {
                        reservation.release("relay retries exhausted");
                    }
                    break e.0;
                }
                retry_count += 1;
                if !route.retry_next() {
                    break e.0;
                }
            }
            Err(e) => {
                if let Some(reservation) = &reservation_finalizer {
                    reservation.release("upstream non-retryable error");
                }
                let err_body = serde_json::json!({"error": {"message": &e.0}}).to_string();
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
                    cache_write_tokens: 0,
                    latency_ms,
                    status_code: 502,
                    success: false,
                    request_body: req_body,
                    response_body: Some(err_body),
                    reasoning_body: None,
                    api_key_name: Some(user.api_key_name.clone()),
                    api_format: "relay".to_string(),
                    stream: false,
                    prompt_price: Decimal::ZERO,
                    completion_price: Decimal::ZERO,
                    cache_read_price: Decimal::ZERO,
                    cache_write_price: Decimal::ZERO,
                    client_ip: Some(client_ip.clone()),
                    endpoint_id: route.endpoint.id,
                    endpoint_url: Some(route.endpoint.url.clone()),
                    original_model: orig_model.clone(),
                    team_id: user.team_id.clone(),
                    ttft_ms: None,
                    account_type: user
                        .team_id
                        .as_ref()
                        .map(|_| "team")
                        .or(Some("user"))
                        .map(String::from),
                    billing_group_id: None,
                    billing_group_name: None,
                    billing_payment_mode: None,
                });
                return Err(GatewayError::from(e));
            }
        }
    };

    let latency_ms = start.elapsed().as_millis() as u64;
    let err_body = serde_json::json!({"error": {"message": &err_msg}}).to_string();
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
        cache_write_tokens: 0,
        latency_ms,
        status_code: 502,
        success: false,
        request_body: req_body,
        response_body: Some(err_body),
        reasoning_body: None,
        api_key_name: Some(user.api_key_name),
        api_format: "relay".to_string(),
        stream: false,
        prompt_price: Decimal::ZERO,
        completion_price: Decimal::ZERO,
        cache_read_price: Decimal::ZERO,
        cache_write_price: Decimal::ZERO,
        client_ip: Some(client_ip),
        endpoint_id: route.endpoint.id,
        endpoint_url: Some(route.endpoint.url.clone()),
        original_model: orig_model.clone(),
        team_id: user.team_id.clone(),
        ttft_ms: None,
        account_type: user
            .team_id
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

pub async fn completions(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let client_ip = extract_client_ip(&headers, addr);
    relay_to_upstream(
        &state,
        &headers,
        body.0,
        "/v1/completions",
        Uuid::new_v4().to_string(),
        Instant::now(),
        client_ip,
    )
    .await
}


