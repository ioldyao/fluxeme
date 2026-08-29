pub async fn embeddings(
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
        "/v1/embeddings",
        Uuid::new_v4().to_string(),
        Instant::now(),
        client_ip,
    )
    .await
}

pub async fn batches(
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
        "/v1/messages/batches",
        Uuid::new_v4().to_string(),
        Instant::now(),
        client_ip,
    )
    .await
}

pub async fn tokenize(
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
        "/tokenize",
        Uuid::new_v4().to_string(),
        Instant::now(),
        client_ip,
    )
    .await
}

pub async fn detokenize(
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
        "/detokenize",
        Uuid::new_v4().to_string(),
        Instant::now(),
        client_ip,
    )
    .await
}

// ── Other ─────────────────────────────────────────────────────────

pub async fn health() -> Json<Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, GatewayError> {
    let user = state.auth.authenticate(&headers)?;

    let mut models: Vec<Value> = if user
        .scopes
        .as_ref()
        .is_some_and(|scopes| !scopes.iter().any(|scope| scope == "model"))
    {
        Vec::new()
    } else {
        state
            .routing
            .list_display_models_for(user.allowed_models.as_deref())
            .into_iter()
            .collect()
    };

    let limit: usize = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20)
        .min(1000);

    let after_id = params.get("after_id");
    let before_id = params.get("before_id");

    if let Some(after) = after_id {
        if let Some(pos) = models.iter().position(|m| m["id"].as_str() == Some(after)) {
            models = models.split_off(pos + 1);
        }
    }
    if let Some(before) = before_id {
        if let Some(pos) = models.iter().position(|m| m["id"].as_str() == Some(before)) {
            models.truncate(pos);
        }
    }

    let has_more = models.len() > limit;
    models.truncate(limit);

    let first_id = models
        .first()
        .and_then(|m| m["id"].as_str().map(|s| s.to_string()));
    let last_id = models
        .last()
        .and_then(|m| m["id"].as_str().map(|s| s.to_string()));

    Ok(Json(serde_json::json!({
        "data": models,
        "first_id": first_id,
        "has_more": has_more,
        "last_id": last_id,
    })))
}


