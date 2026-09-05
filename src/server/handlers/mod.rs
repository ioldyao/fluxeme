use std::sync::Arc;
use std::time::Instant;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::{Json, Response};
use serde_json::Value;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::observability::gateway_events::{GatewayAccessEvent, GatewayEventRecorder};
use crate::observability::lifecycle::RequestLifecycle;
use crate::server::AppState;
use crate::service::auth::{credential_fingerprint, AuthError};

/// Authenticate an inference request and record failures without creating a
/// request/usage/billing lifecycle event.
fn authenticate_inference(
    state: &AppState,
    headers: &HeaderMap,
    addr: SocketAddr,
    request_id: &str,
    path: &str,
) -> Result<crate::domain::user::AuthResult, GatewayError> {
    let started_at = Instant::now();
    match state.auth.authenticate(headers) {
        Ok(user) => Ok(user),
        Err(error) => {
            let credential_fingerprint = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .or_else(|| headers.get("x-api-key").and_then(|v| v.to_str().ok()))
                .map(credential_fingerprint);
            record_auth_failure(
                &state.gateway_events,
                extract_client_ip(headers, addr),
                request_id,
                path,
                &error,
                credential_fingerprint,
                started_at.elapsed().as_millis() as u64,
            );
            Err(error.into())
        }
    }
}

fn record_auth_failure(
    recorder: &GatewayEventRecorder,
    client_ip: String,
    request_id: &str,
    path: &str,
    error: &AuthError,
    credential_fingerprint: Option<String>,
    latency_ms: u64,
) {
    recorder.record_access(GatewayAccessEvent {
        timestamp: chrono::Utc::now().to_rfc3339(),
        request_id: request_id.to_string(),
        user_id: None,
        api_key_id: None,
        credential_fingerprint,
        route_id: "authentication".to_string(),
        method: "POST".to_string(),
        path: path.to_string(),
        client_ip: Some(client_ip),
        auth_result: "failure".to_string(),
        error_kind: Some(error.kind().as_str().to_string()),
        status_code: 401,
        success: false,
        latency_ms,
        bytes_in: 0,
        bytes_out: 0,
    });
}

/// Create the authenticated request lifecycle before model trimming, so even
/// malformed model fields and rate-limit failures produce one request event.
fn new_lifecycle(
    state: &AppState,
    request_id: String,
    path: &str,
    format: &str,
    stream: bool,
    headers: &HeaderMap,
    addr: SocketAddr,
    user: &crate::domain::user::AuthResult,
    body: &Value,
) -> Arc<RequestLifecycle> {
    let requested_model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    Arc::new(RequestLifecycle::new(
        &state.gateway_events,
        crate::scheduler::helpers::lifecycle_meta(request_id, path, format, stream, headers, addr),
        crate::scheduler::helpers::lifecycle_identity(user, &requested_model),
    ))
}

// The dispatch pipeline now lives in crate::scheduler; handlers are thin
// shells that authenticate, rate-limit, and call SchedulerService::dispatch.
use crate::scheduler::{
    helpers::{extract_client_ip, trim_model},
    DispatchFormat, DispatchRequest, GatewayError,
};

include!("openai.rs");
include!("anthropic.rs");
include!("token_endpoints.rs");
include!("relay.rs");
include!("responses.rs");
include!("meta.rs");
include!("tokens.rs");
include!("tests.rs");
