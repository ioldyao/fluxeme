use serde::{Deserialize, Serialize};

/// Event published when a request completes (supersedes the old ws::RequestEvent).
/// Sent after the upstream response finishes — carries token counts and latency.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestCompleted {
    #[serde(rename = "type")]
    pub event_type: String,
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
/// call starts. The explicit event type distinguishes it from completion events;
/// latency is not used as a discriminator because a completed request may be 0ms.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteDecided {
    #[serde(rename = "type")]
    pub event_type: String,
    pub timestamp: String,
    pub request_id: String,
    pub model: String,
    pub channel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<i64>,
    pub user_id: String,
}
