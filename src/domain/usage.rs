use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub timestamp: String,
    pub request_id: String,
    pub user_id: String,
    pub user_name: String,
    pub channel_id: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub latency_ms: u64,
    pub status_code: u16,
    pub success: bool,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
}
