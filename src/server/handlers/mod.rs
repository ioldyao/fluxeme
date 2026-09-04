use std::sync::Arc;
use std::time::Instant;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::{Json, Response};
use serde_json::Value;
use std::net::SocketAddr;
use uuid::Uuid;

use crate::server::AppState;

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
