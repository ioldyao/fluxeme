use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

pub const USER_STATUS_ACTIVE: &str = "active";
pub const USER_STATUS_SUSPENDED: &str = "suspended";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limits: Option<RateLimit>,
    #[serde(skip)]
    pub password_hash: Option<String>,
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub token_version: i64,
    #[serde(default)]
    pub role: String,
    #[serde(default = "default_concurrency")]
    pub concurrency_limit: u32,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default = "default_user_status")]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended_at: Option<String>,
}

fn default_concurrency() -> u32 {
    2000
}

fn default_currency() -> String {
    "usd".to_string()
}

fn default_user_status() -> String {
    USER_STATUS_ACTIVE.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    pub rpm: Option<u64>,
    pub tpm: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub key: String,
    pub user_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, with = "rust_decimal::serde::float_option")]
    pub spend_limit: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_models: Option<Vec<String>>,
    /// Team scope for this key. `None` = personal key (existing behavior).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    /// 访问范围 = 资源类型（model / skill / mcp）。来自 api_key_scopes 表。
    /// `None` = 未加载（创建/内部路径不设置；列表路径填充）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

fn default_enabled() -> bool {
    true
}

/// Resolved auth result for request processing.
#[derive(Debug, Clone)]
pub struct AuthResult {
    pub user_id: String,
    pub user_name: String,
    pub rate_limits: Option<(u64, u64)>,
    pub allowed_models: Option<Vec<String>>,
    pub api_key_name: String,
    #[allow(dead_code)]
    pub concurrency_limit: u32,
    /// Active team context resolved from the request. `None` for personal accounts.
    pub team_id: Option<String>,
    /// Role within the active team. `None` for personal accounts.
    pub team_role: Option<crate::domain::team::TeamRole>,
}

/// Session info for admin panel login
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub user_id: String,
    pub user_name: String,
    pub role: String, // "admin" or "user"
    #[serde(default)]
    pub token_version: i64,
}
