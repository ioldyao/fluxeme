// ── Messages entry points (Anthropic-native format) ──────────────
//
// Thin HTTP entry points; the full dispatch pipeline lives in
// crate::scheduler::SchedulerService.

// Importers/callers: registered in src/server/mod.rs as the public HTTP route
// handler for POST /v1/messages and mirrored by the new POST
// /v1/messages/count_tokens handler. Affected API: Anthropic-native endpoints.
// Data schema: Anthropic request body and message/count_tokens responses.
// User instruction: "要，添加`/v1/messages/count_tokens`的端点支持".
pub async fn messages_count_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();

    let user = state.auth.authenticate(&headers)?;
    let lifecycle = new_lifecycle(
        &state,
        request_id.clone(),
        "/v1/messages/count_tokens",
        "anthropic",
        false,
        &headers,
        addr,
        &user,
        &body,
    );
    let model = match trim_model(&mut body) {
        Ok(model) => model,
        Err(e) => {
            let classified = crate::scheduler::helpers::ClassifiedError::from(e);
            lifecycle.finalize_classified(&classified);
            return Err(classified.into_gateway());
        }
    };

    let request_span = tracing::info_span!(
        "messages_count_tokens",
        request_id = %request_id,
        user_id = %user.user_id,
        model = %model,
    );
    let _guard = request_span.enter();

    tracing::info!(request_id, user = %user.user_id, model = %model, "Incoming messages count_tokens request");

    if let Some((rpm, tpm)) = user.rate_limits {
        if let Err(e) = state.rate_limiter.check_rpm(&user.user_id, rpm).await {
            let classified = crate::scheduler::helpers::ClassifiedError::from(e);
            lifecycle.finalize_classified(&classified);
            return Err(classified.into_gateway());
        }
        if let Err(e) = state
            .rate_limiter
            .check_tpm(&user.user_id, tpm, estimate_tokens_anthropic(&body))
            .await
        {
            let classified = crate::scheduler::helpers::ClassifiedError::from(e);
            lifecycle.finalize_classified(&classified);
            return Err(classified.into_gateway());
        }
    }

    let client_ip = extract_client_ip(&headers, addr);

    state
        .scheduler
        .dispatch(DispatchRequest {
            auth: user,
            model,
            body,
            stream: false,
            request_id,
            start: Instant::now(),
            client_ip,
            format: DispatchFormat::CountTokens,
            lifecycle,
        })
        .await
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

    let user = state.auth.authenticate(&headers)?;
    let is_streaming = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let lifecycle = new_lifecycle(
        &state,
        request_id.clone(),
        "/v1/messages",
        "anthropic",
        is_streaming,
        &headers,
        addr,
        &user,
        &body,
    );
    let model = match trim_model(&mut body) {
        Ok(model) => model,
        Err(e) => {
            let classified = crate::scheduler::helpers::ClassifiedError::from(e);
            lifecycle.finalize_classified(&classified);
            return Err(classified.into_gateway());
        }
    };

    let request_span = tracing::info_span!(
        "messages",
        request_id = %request_id,
        user_id = %user.user_id,
        model = %model,
    );
    let _guard = request_span.enter();

    tracing::info!(request_id, user = %user.user_id, model = %model, "Incoming messages request");

    if let Some((rpm, tpm)) = user.rate_limits {
        if let Err(e) = state.rate_limiter.check_rpm(&user.user_id, rpm).await {
            let classified = crate::scheduler::helpers::ClassifiedError::from(e);
            lifecycle.finalize_classified(&classified);
            return Err(classified.into_gateway());
        }
        if let Err(e) = state
            .rate_limiter
            .check_tpm(&user.user_id, tpm, estimate_tokens_anthropic(&body))
            .await
        {
            let classified = crate::scheduler::helpers::ClassifiedError::from(e);
            lifecycle.finalize_classified(&classified);
            return Err(classified.into_gateway());
        }
    }

    let client_ip = extract_client_ip(&headers, addr);

    state
        .scheduler
        .dispatch(DispatchRequest {
            auth: user,
            model,
            body,
            stream: is_streaming,
            request_id,
            start,
            client_ip,
            format: DispatchFormat::AnthropicMessages,
            lifecycle,
        })
        .await
}
