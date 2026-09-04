// ── Token-counting endpoint entries ──────────────────────────────
//
// Thin HTTP entry points for POST /responses/input_tokens. The dispatch
// pipeline lives in crate::scheduler::SchedulerService.

// Importers/callers: registered in src/server/mod.rs as the public HTTP route
// handler for POST /responses/input_tokens. Affected API: OpenAI-native token
// counting endpoint relayed upstream to POST /v1/responses/input_tokens. Data
// schema: OpenAI Responses request body and response shape
// {"object":"response.input_tokens","input_tokens": number}. User
// instruction: "openai的提供商层，也添加这个openai的端点`POST/responses/input_tokens`".
pub async fn responses_input_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();

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

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm).await?;
        state
            .rate_limiter
            .check_tpm(&user.user_id, tpm, estimate_tokens_responses(&body))
            .await?;
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
            format: DispatchFormat::ResponsesInputTokens,
        })
        .await
}
