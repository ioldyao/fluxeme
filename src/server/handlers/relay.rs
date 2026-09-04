// ── Relay entry points ───────────────────────────────────────────
//
// Thin HTTP entry points for relayed non-streaming upstream paths
// (/v1/completions, /v1/embeddings, /v1/messages/batches, /tokenize,
// /detokenize). The dispatch pipeline lives in crate::scheduler.

async fn relay_dispatch(
    state: &AppState,
    headers: &HeaderMap,
    addr: SocketAddr,
    body: Value,
    path: &str,
) -> Result<Response, GatewayError> {
    let mut body = body;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();

    let user = state.auth.authenticate(headers)?;
    let model = trim_model(&mut body)?;

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm).await?;
        state
            .rate_limiter
            .check_tpm(&user.user_id, tpm, estimate_tokens(&body))
            .await?;
    }

    let client_ip = extract_client_ip(headers, addr);

    state
        .scheduler
        .dispatch(DispatchRequest {
            auth: user,
            model,
            body,
            stream: false,
            request_id,
            start,
            client_ip,
            format: DispatchFormat::Relay {
                path: path.to_string(),
            },
        })
        .await
}

pub async fn completions(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_dispatch(&state, &headers, addr, body.0, "/v1/completions").await
}
