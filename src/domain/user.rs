use serde::{Deserialize, Serialize};

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend_limit: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_models: Option<Vec<String>>,
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
