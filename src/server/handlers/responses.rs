// Thin HTTP entry point for POST /v1/responses. The dispatch pipeline lives
// in crate::scheduler::SchedulerService.

pub async fn responses(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();

    let user = authenticate_inference(
        &state,
        &headers,
        addr,
        &request_id,
        "/v1/responses",
    )?;
    let is_streaming = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let lifecycle = new_lifecycle(
        &state,
        request_id.clone(),
        "/v1/responses",
        "openai",
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
            format: DispatchFormat::Responses,
            lifecycle,
        })
        .await
}
