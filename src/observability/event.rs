use serde::Serialize;

/// Event published when a request completes (supersedes the old ws::RequestEvent).
/// Sent after the upstream response finishes — carries token counts and latency.
#[derive(Clone, Debug, Serialize)]
pub struct RequestCompleted {
    pub timestamp: String,
    pub request_id: String,
    pub model: String,
    pub channel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<i64>,
    pub latency_ms: u64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
}

/// Event published immediately after route resolution, before the upstream
/// call starts.  `latency_ms` is always 0 — the frontend uses this to
/// distinguish "in-flight" from "completed" events.
#[derive(Clone, Debug, Serialize)]
pub struct RouteDecided {
    pub timestamp: String,
    pub request_id: String,
    pub model: String,
    pub channel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<i64>,
    pub user_id: String,
}
