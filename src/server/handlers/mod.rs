use std::sync::Arc;
use std::time::Instant;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::{Json, Response};
use serde_json::Value;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::observability::lifecycle::RequestLifecycle;
use crate::server::AppState;

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
