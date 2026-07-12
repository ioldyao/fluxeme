use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone)]
pub struct UsageFilter {
    pub user_id: Option<String>,
    pub model: Option<String>,
    pub api_key_name: Option<String>,
    pub api_format: Option<String>,
}

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
    pub reasoning_body: Option<String>,
    pub api_key_name: Option<String>,
    pub api_format: String,
    pub stream: bool,
    pub cache_hit_input_tokens: u64,
}
