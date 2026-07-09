use serde::{Deserialize, Serialize};

fn default_db_path() -> String {
    "gateway.db".to_string()
}

fn default_weight() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub admin: AdminConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub jwt_secret: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdminConfig {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_db_path")]
    pub path: String,
}

/// Resolved endpoint with credentials, passed to provider adapters.
/// Separated from domain::Endpoint (which has DB fields like id, channel_id).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EndpointConfig {
    pub url: String,
    pub api_key: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
    pub timeout_secs: Option<u64>,
}
