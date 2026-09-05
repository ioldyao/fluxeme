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

    let user = authenticate_inference(state, headers, addr, &request_id, path)?;
    let lifecycle = new_lifecycle(
        state,
        request_id.clone(),
        path,
        "relay",
        false,
        headers,
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
            lifecycle,
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
