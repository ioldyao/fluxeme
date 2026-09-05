// Thin HTTP entry point. The full dispatch pipeline (route → endpoint
// selection → upstream call → retry → breaker feedback → cache → usage)
// lives in crate::scheduler::SchedulerService.

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

    let user = authenticate_inference(
        &state,
        &headers,
        addr,
        &request_id,
        "/v1/chat/completions",
    )?;
    // Create the lifecycle right after authentication so every LLM-data-plane
    // failure below (validation, rate limit, routing, upstream) yields exactly
    // one gateway request event.
    let lifecycle = new_lifecycle(
        &state,
        request_id.clone(),
        "/v1/chat/completions",
        "openai",
        body
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
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
        "chat_completions",
        request_id = %request_id,
        user_id = %user.user_id,
        model = %model,
    );
    let _guard = request_span.enter();

    tracing::info!(request_id, user = %user.user_id, model = %model, content_length = %content_len, "Incoming request");

    if let Some((rpm, tpm)) = user.rate_limits {
        if let Err(e) = state.rate_limiter.check_rpm(&user.user_id, rpm).await {
            let classified = crate::scheduler::helpers::ClassifiedError::from(e);
            lifecycle.finalize_classified(&classified);
            return Err(classified.into_gateway());
        }
        if let Err(e) = state
            .rate_limiter
            .check_tpm(&user.user_id, tpm, estimate_tokens(&body))
            .await
        {
            let classified = crate::scheduler::helpers::ClassifiedError::from(e);
            lifecycle.finalize_classified(&classified);
            return Err(classified.into_gateway());
        }
    }

    let is_streaming = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
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
            format: DispatchFormat::OpenaiChat,
            lifecycle,
        })
        .await
}
