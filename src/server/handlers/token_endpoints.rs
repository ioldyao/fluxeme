fn count_tokens_supported_for_channel(channel: Option<&crate::domain::channel::Channel>) -> bool {
    !matches!(channel, Some(ch) if ch.anthropic_compat)
}

fn responses_input_tokens_supported_for_channel(
    channel: Option<&crate::domain::channel::Channel>,
) -> bool {
    matches!(
        channel.map(|ch| ch.provider.as_str()),
        None | Some("openai" | "azure" | "ollama")
    )
}

async fn handle_messages_count_tokens(
    route: &mut RouteTarget,
    body: Value,
    request_id: &str,
    max_retries: u32,
) -> Result<Value, GatewayError> {
    let mut retry_count = 0u32;

    let err_msg: String = loop {
        let result = route
            .adapter
            .count_tokens(&route.endpoint, body.clone())
            .await;

        match result {
            Ok(resp) => {
                route.report_success();
                return Ok(resp);
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
                if retry_count >= max_retries {
                    break e.0;
                }
                retry_count += 1;
                if !route.retry_next() {
                    break e.0;
                }
            }
            Err(e) => {
                tracing::error!(request_id = %request_id, endpoint = %route.endpoint.url, error = %e.0, "Count tokens upstream request failed");
                return Err(GatewayError::Upstream(
                    "Upstream count_tokens request failed".into(),
                ));
            }
        }
    };

    tracing::error!(request_id = %request_id, endpoint = %route.endpoint.url, error = %err_msg, "Count tokens upstream request failed after retries");
    Err(GatewayError::Upstream(
        "Upstream count_tokens request failed".into(),
    ))
}

async fn handle_responses_input_tokens(
    route: &mut RouteTarget,
    body: Value,
    request_id: &str,
    max_retries: u32,
) -> Result<Value, GatewayError> {
    let mut retry_count = 0u32;

    let err_msg: String = loop {
        let result = route
            .adapter
            .responses_input_tokens(&route.endpoint, body.clone())
            .await;

        match result {
            Ok(resp) => {
                route.report_success();
                return Ok(resp);
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
                if retry_count >= max_retries {
                    break e.0;
                }
                retry_count += 1;
                if !route.retry_next() {
                    break e.0;
                }
            }
            Err(e) => {
                tracing::error!(request_id = %request_id, endpoint = %route.endpoint.url, error = %e.0, "Responses input_tokens upstream request failed");
                return Err(GatewayError::Upstream(
                    "Upstream responses input_tokens request failed".into(),
                ));
            }
        }
    };

    tracing::error!(request_id = %request_id, endpoint = %route.endpoint.url, error = %err_msg, "Responses input_tokens upstream request failed after retries");
    Err(GatewayError::Upstream(
        "Upstream responses input_tokens request failed".into(),
    ))
}

// ── Handlers ──────────────────────────────────────────────────────

// Importers/callers: registered in src/server/mod.rs as the public HTTP route
// handler for POST /responses/input_tokens. Affected API: OpenAI-native token
// counting endpoint relayed upstream to POST /v1/responses/input_tokens. Data
// schema: OpenAI Responses request body and response shape
// {"object":"response.input_tokens","input_tokens": number}. User
// instruction: "openai的提供商层，也添加这个openai的端点`POST/responses/input_tokens`".
pub async fn responses_input_tokens(
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
        "responses_input_tokens",
        request_id = %request_id,
        user_id = %user.user_id,
        model = %model,
    );
    let _guard = request_span.enter();

    tracing::info!(request_id, user = %user.user_id, model = %model, "Incoming responses input_tokens request");

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
            .check_tpm(&user.user_id, tpm, estimate_tokens_responses(&body))
            .await?;
    }

    let (channel_id, resolved_model, upstream_model) = state
        .routing
        .route_public(&user.user_id, &model, user.team_id.as_deref())
        .await?;
    if let Some(ref id) = upstream_model {
        body["model"] = Value::String(id.clone());
    }

    let mut route = resolve_route(&state, &channel_id)?;

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
    if !responses_input_tokens_supported_for_channel(channel.as_ref()) {
        return Err(GatewayError::BadRequest(
            "POST /responses/input_tokens is only supported for OpenAI-compatible channels".into(),
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
                tracing::warn!(request_id, rule = %rule_name, "Responses input_tokens request blocked by content filter");
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
                        "Responses input_tokens request body masked by content filter"
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
        handle_responses_input_tokens(&mut route, body, &request_id, gw_cfg.max_retries).await
    })
    .await;
    state.flow_tracker.mark_completed(&rid);

    match result {
        Ok(inner) => Ok(Json(inner?).into_response()),
        Err(_) => {
            tracing::error!(
                rid,
                handler_timeout_s = handler_timeout.as_secs(),
                "Responses input_tokens handler timed out"
            );
            Err(GatewayError::Upstream("Request timed out".into()))
        }
    }
}

