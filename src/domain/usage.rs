use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone)]
pub struct UsageFilter {
    pub user_id: Option<String>,
    pub model: Option<String>,
    pub api_key_name: Option<String>,
    pub api_format: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
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
    pub cache_write_tokens: u64,
    #[serde(with = "rust_decimal::serde::float")]
    pub prompt_price: Decimal,
    #[serde(with = "rust_decimal::serde::float")]
    pub completion_price: Decimal,
    #[serde(with = "rust_decimal::serde::float")]
    pub cache_read_price: Decimal,
    pub client_ip: Option<String>,
    #[serde(default)]
    pub endpoint_id: Option<i64>,
    /// Endpoint URL at request time. Stable across endpoint re-creation
    /// (unlike endpoint_id, which changes when an endpoint row is deleted
    /// and re-added) — used by the flow-control timeline to match a request
    /// to its endpoint. Captured here because old endpoint rows may be gone
    /// by the time the observability consumer runs.
    #[serde(default)]
    pub endpoint_url: Option<String>,
    /// Original model name before routing rule rewrites.
    /// Empty string if no rewrite occurred.
    #[serde(default)]
    pub original_model: String,
    /// Team scope for this usage record. `None` = personal (charged to the
    /// user's wallet); `Some` = team account (charged to the team wallet).
    #[serde(default)]
    pub team_id: Option<String>,
    /// Which account type was charged: "user" or "team". Mirrors the
    /// discriminator column on wallet_transactions / billing_events.
    #[serde(default)]
    pub account_type: Option<String>,
}
