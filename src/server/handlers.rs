use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use bytes::Bytes;
use chrono::Utc;
use futures::stream::StreamExt;
use serde_json::Value;
use uuid::Uuid;

use crate::balancer::LoadBalancer;
use crate::config::types::EndpointConfig;
use crate::domain::usage::UsageRecord;
use crate::server::AppState;

// ── Error type ────────────────────────────────────────────────────

#[derive(Debug)]
pub enum GatewayError {
    Auth(String),
    RateLimit(String),
    Route(String),
    BadRequest(String),
    Upstream(String),
    Internal(String),
}

impl GatewayError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Auth(_) => StatusCode::UNAUTHORIZED,
            Self::RateLimit(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Route(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Auth(m)
            | Self::RateLimit(m)
            | Self::Route(m)
            | Self::BadRequest(m)
            | Self::Upstream(m)
            | Self::Internal(m) => m,
        }
    }
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({
            "error": {
                "message": self.message(),
                "type": "gateway_error",
            }
        });
        (self.status(), Json(body)).into_response()
    }
}

impl From<crate::service::auth::AuthError> for GatewayError {
    fn from(e: crate::service::auth::AuthError) -> Self {
        Self::Auth(e.0)
    }
}

impl From<crate::service::routing::RouteError> for GatewayError {
    fn from(e: crate::service::routing::RouteError) -> Self {
        Self::Route(e.0)
    }
}

impl From<crate::ratelimit::RateLimitError> for GatewayError {
    fn from(e: crate::ratelimit::RateLimitError) -> Self {
        Self::RateLimit(e.0)
    }
}

impl From<crate::provider::ProviderError> for GatewayError {
    fn from(e: crate::provider::ProviderError) -> Self {
        Self::Upstream(e.0)
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn trim_model(body: &mut Value) -> Result<String, GatewayError> {
    let model_val = body["model"].clone();
    let s = model_val
        .as_str()
        .ok_or_else(|| GatewayError::BadRequest("Missing 'model' field".into()))?
        .trim()
        .to_string();
    if s.is_empty() {
        return Err(GatewayError::BadRequest("'model' field is empty".into()));
    }
    body["model"] = Value::String(s.clone());
    Ok(s)
}

struct RouteTarget {
    channel_id: String,
    endpoint: EndpointConfig,
    adapter: Arc<dyn crate::provider::ProviderAdapter>,
}

fn resolve_route(state: &AppState, channel_id: &str) -> Result<RouteTarget, GatewayError> {
    let (provider_name, endpoints) = state
        .routing
        .resolve_channel(channel_id)
        .ok_or_else(|| GatewayError::Internal(format!("Channel '{}' not found or disabled", channel_id)))?;

    let adapter = state
        .providers
        .get(&provider_name.as_str())
        .ok_or_else(|| GatewayError::Internal(format!("Unknown provider: {}", provider_name)))?;

    let endpoint = LoadBalancer::new(&endpoints)
        .select(&endpoints)
        .ok_or_else(|| GatewayError::Internal("No available endpoints".into()))?
        .clone();

    Ok(RouteTarget {
        channel_id: channel_id.to_string(),
        endpoint,
        adapter,
    })
}

// ── Streaming ─────────────────────────────────────────────────────

async fn handle_streaming(
    state: &AppState,
    adapter: Arc<dyn crate::provider::ProviderAdapter>,
    endpoint: EndpointConfig,
    body: Value,
    request_id: String,
    user_id: String,
    user_name: String,
    channel_id: String,
    model: String,
    start: Instant,
) -> Result<Response, GatewayError> {
    let stream_result = adapter.chat_complete_stream(&endpoint, body).await;

    match stream_result {
        Ok(stream) => {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let tx_for_usage = tx.clone();
            let usage = state.usage.clone();

            tokio::spawn(async move {
                let mut stream = Box::pin(stream);
                while let Some(data) = stream.next().await {
                    let _ = tx_for_usage.send(data);
                }
                drop(tx_for_usage);

                let latency_ms = start.elapsed().as_millis() as u64;
                usage.record(UsageRecord {
                    timestamp: Utc::now().to_rfc3339(),
                    request_id,
                    user_id,
                    user_name,
                    channel_id,
                    model,
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    total_tokens: 0,
                    latency_ms,
                    status_code: 200,
                    success: true,
                });
            });

            let rx_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
            let body_stream = rx_stream.map(|data| {
                Ok::<_, std::convert::Infallible>(Bytes::from(data))
            });

            Ok(Response::builder()
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("connection", "keep-alive")
                .header("access-control-allow-origin", "*")
                .body(Body::from_stream(body_stream))
                .unwrap())
        }
        Err(e) => {
            let latency_ms = start.elapsed().as_millis() as u64;
            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id,
                user_name,
                channel_id,
                model,
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                latency_ms,
                status_code: 502,
                success: false,
            });
            Err(GatewayError::Upstream(e.0))
        }
    }
}

// ── Non-streaming ─────────────────────────────────────────────────

async fn handle_non_streaming(
    state: &AppState,
    adapter: Arc<dyn crate::provider::ProviderAdapter>,
    endpoint: EndpointConfig,
    body: Value,
    request_id: String,
    user_id: String,
    user_name: String,
    channel_id: String,
    model: String,
    start: Instant,
) -> Result<Response, GatewayError> {
    let result = adapter.chat_complete(&endpoint, body).await;
    let latency_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => {
            let prompt_tokens = resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
            let completion_tokens = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0);

            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id,
                user_name,
                channel_id,
                model,
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                latency_ms,
                status_code: 200,
                success: true,
            });

            Ok(Json(resp).into_response())
        }
        Err(e) => {
            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id,
                user_name,
                channel_id,
                model,
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                latency_ms,
                status_code: 502,
                success: false,
            });
            Err(GatewayError::Upstream(e.0))
        }
    }
}

// ── Handlers ──────────────────────────────────────────────────────

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();

    let user = state.auth.authenticate(&headers)?;
    let model = trim_model(&mut body)?;

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm)?;
        state.rate_limiter.check_tpm(&user.user_id, tpm, estimate_tokens(&body))?;
    }

    let channel_id = state.routing.route(&user.user_id, &model)?;
    let route = resolve_route(&state, &channel_id)?;
    let is_streaming = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    if is_streaming {
        handle_streaming(
            &state, route.adapter, route.endpoint, body,
            request_id, user.user_id, user.user_name, route.channel_id, model, start,
        )
        .await
    } else {
        handle_non_streaming(
            &state, route.adapter, route.endpoint, body,
            request_id, user.user_id, user.user_name, route.channel_id, model, start,
        )
        .await
    }
}

pub async fn messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    let mut body = body.0;
    let request_id = Uuid::new_v4().to_string();
    let start = Instant::now();

    let user = state.auth.authenticate(&headers)?;
    let model = trim_model(&mut body)?;

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm)?;
        state.rate_limiter.check_tpm(&user.user_id, tpm, estimate_tokens_anthropic(&body))?;
    }

    let channel_id = state.routing.route(&user.user_id, &model)?;
    let route = resolve_route(&state, &channel_id)?;
    let is_streaming = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    if is_streaming {
        handle_streaming(
            &state, route.adapter, route.endpoint, body,
            request_id, user.user_id, user.user_name, route.channel_id, model, start,
        )
        .await
    } else {
        handle_non_streaming(
            &state, route.adapter, route.endpoint, body,
            request_id, user.user_id, user.user_name, route.channel_id, model, start,
        )
        .await
    }
}

// ── Relay ─────────────────────────────────────────────────────────

async fn relay_to_upstream(
    state: &AppState,
    headers: &HeaderMap,
    mut body: Value,
    upstream_path: &str,
    request_id: String,
    start: Instant,
) -> Result<Response, GatewayError> {
    let user = state.auth.authenticate(headers)?;
    let model = trim_model(&mut body)?;

    if let Some((rpm, tpm)) = user.rate_limits {
        state.rate_limiter.check_rpm(&user.user_id, rpm)?;
        state.rate_limiter.check_tpm(&user.user_id, tpm, estimate_tokens(&body))?;
    }

    let channel_id = state.routing.route(&user.user_id, &model)?;
    let route = resolve_route(&state, &channel_id)?;
    let latency_ms = start.elapsed().as_millis() as u64;

    match route.adapter.relay(&route.endpoint, upstream_path, body).await {
        Ok(resp) => {
            let prompt_tokens = resp["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
            let completion_tokens = resp["usage"]["completion_tokens"].as_u64().unwrap_or(0);

            state.usage.record(UsageRecord {
                timestamp: Utc::now().to_rfc3339(),
                request_id,
                user_id: user.user_id,
                user_name: user.user_name,
                channel_id: route.channel_id,
                model,
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                latency_ms,
                status_code: 200,
                success: true,
            });

            Ok(Json(resp).into_response())
        }
        Err(e) => {
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
                latency_ms,
                status_code: 502,
                success: false,
            });

            Err(GatewayError::from(e))
        }
    }
}

pub async fn completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/v1/completions",
        Uuid::new_v4().to_string(), Instant::now()).await
}

pub async fn embeddings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/v1/embeddings",
        Uuid::new_v4().to_string(), Instant::now()).await
}

pub async fn tokenize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/tokenize",
        Uuid::new_v4().to_string(), Instant::now()).await
}

pub async fn detokenize(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Json<Value>,
) -> Result<Response, GatewayError> {
    relay_to_upstream(&state, &headers, body.0, "/detokenize",
        Uuid::new_v4().to_string(), Instant::now()).await
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
) -> Json<Value> {
    let models = state.routing.list_display_models();
    Json(serde_json::json!({
        "object": "list",
        "data": models,
    }))
}

// ── Token estimators ──────────────────────────────────────────────

fn estimate_tokens(body: &Value) -> u64 {
    let text = body["messages"]
        .as_array()
        .map(|msgs| {
            msgs.iter()
                .filter_map(|m| m["content"].as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    (text.len() / 4) as u64
}

fn estimate_tokens_anthropic(body: &Value) -> u64 {
    let text = body["messages"]
        .as_array()
        .map(|msgs| {
            msgs.iter()
                .filter_map(|m| match &m["content"] {
                    Value::String(s) => Some(s.clone()),
                    Value::Array(arr) => Some(
                        arr.iter()
                            .filter_map(|c| c["text"].as_str())
                            .collect::<Vec<_>>()
                            .join(" "),
                    ),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    (text.len() / 4) as u64
}
