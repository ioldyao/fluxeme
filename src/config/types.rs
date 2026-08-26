use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub jwt_secret: Option<String>,
    #[serde(default)]
    pub encryption_key: Option<String>,
    #[serde(default)]
    pub previous_encryption_key: Option<String>,
    #[serde(default)]
    pub sso: SsoConfig,
    #[serde(default)]
    pub cors: CorsConfig,
    #[serde(default)]
    pub redis: RedisConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_pg_url")]
    pub pg_url: String,
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,
    #[serde(default)]
    pub clickhouse: ClickHouseConfig,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            pg_url: default_pg_url(),
            retention_days: default_retention_days(),
            clickhouse: ClickHouseConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClickHouseConfig {
    #[serde(default = "default_ch_host")]
    pub host: String,
    #[serde(default = "default_ch_port")]
    pub port: u16,
    #[serde(default = "default_ch_user")]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_ch_db")]
    pub db: String,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            host: default_ch_host(),
            port: default_ch_port(),
            user: default_ch_user(),
            password: String::new(),
            db: default_ch_db(),
        }
    }
}

fn default_ch_host() -> String {
    "localhost".to_string()
}
fn default_ch_port() -> u16 {
    8123
}
fn default_ch_user() -> String {
    "default".to_string()
}
fn default_ch_db() -> String {
    "aigateway".to_string()
}

fn default_pg_url() -> String {
    String::new()
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

// ── Redis Config ────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RedisConfig {
    #[serde(default = "default_redis_enabled")]
    pub enabled: bool,
    #[serde(default = "default_redis_url")]
    pub url: String,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            enabled: default_redis_enabled(),
            url: default_redis_url(),
        }
    }
}

fn default_redis_enabled() -> bool {
    true
}

fn default_redis_url() -> String {
    "redis://127.0.0.1:16379".to_string()
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
    /// When true, use `url` exactly without appending a provider path.
    #[serde(default)]
    pub full_url: bool,
}

fn default_enabled() -> bool {
    true
}

// ── Gateway Runtime Config ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRuntimeConfig {
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,
    #[serde(default = "default_unary_base_timeout")]
    pub unary_base_timeout_secs: u64,
    #[serde(default = "default_body_size_extra")]
    pub body_size_extra_secs_per_100kb: u64,
    #[serde(default = "default_stream_first_byte_timeout")]
    pub stream_first_byte_timeout_secs: u64,
    #[serde(default = "default_stream_idle_timeout")]
    pub stream_idle_timeout_secs: u64,
    #[serde(default = "default_stream_total_timeout")]
    pub stream_total_timeout_secs: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_handler_timeout")]
    pub handler_timeout_secs: u64,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,
    #[serde(default)]
    pub billing_enabled: bool,
}

fn default_connect_timeout() -> u64 {
    10
}
fn default_unary_base_timeout() -> u64 {
    60
}
fn default_body_size_extra() -> u64 {
    5
}
fn default_stream_first_byte_timeout() -> u64 {
    60
}
fn default_stream_idle_timeout() -> u64 {
    30
}
fn default_stream_total_timeout() -> u64 {
    600
}
fn default_max_retries() -> u32 {
    2
}
fn default_handler_timeout() -> u64 {
    240
}
fn default_cache_ttl() -> u64 {
    300
}

impl Default for GatewayRuntimeConfig {
    fn default() -> Self {
        Self {
            connect_timeout_secs: default_connect_timeout(),
            unary_base_timeout_secs: default_unary_base_timeout(),
            body_size_extra_secs_per_100kb: default_body_size_extra(),
            stream_first_byte_timeout_secs: default_stream_first_byte_timeout(),
            stream_idle_timeout_secs: default_stream_idle_timeout(),
            stream_total_timeout_secs: default_stream_total_timeout(),
            max_retries: default_max_retries(),
            handler_timeout_secs: default_handler_timeout(),
            cache_ttl_secs: default_cache_ttl(),
            billing_enabled: false,
        }
    }
}
