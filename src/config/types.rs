use serde::{Deserialize, Serialize};

fn default_db_path() -> String {
    "gateway.db".to_string()
}

fn default_retention_days() -> u64 {
    90
}

fn default_weight() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SsoConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provider_name: String,
    #[serde(default)]
    pub issuer_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub redirect_url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub admin: AdminConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub jwt_secret: Option<String>,
    #[serde(default)]
    pub sso: SsoConfig,
    #[serde(default)]
    pub cors: CorsConfig,
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
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CorsConfig {
    #[serde(default = "default_allowed_origins")]
    pub allowed_origins: Vec<String>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: default_allowed_origins(),
        }
    }
}

fn default_allowed_origins() -> Vec<String> {
    vec![
        "http://localhost:5173".to_string(),
        "http://localhost:8080".to_string(),
    ]
}

/// Resolved endpoint with credentials, passed to provider adapters.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EndpointConfig {
    #[serde(default)]
    pub id: Option<i64>,
    pub url: String,
    pub api_key: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
    pub timeout_secs: Option<u64>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub enable_gzip: bool,
}

fn default_enabled() -> bool {
    true
}
