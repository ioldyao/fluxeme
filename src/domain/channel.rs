use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub provider: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// When true, this OpenAI channel also accepts Anthropic-format /v1/messages
    /// requests alongside native OpenAI /v1/chat/completions requests.
    #[serde(default)]
    pub anthropic_compat: bool,
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: Option<i64>,
    #[serde(skip)]
    pub channel_id: String,
    pub url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
    pub timeout_secs: Option<u64>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_priority() -> i32 {
    1
}

fn default_enabled() -> bool {
    true
}

fn default_weight() -> u32 {
    1
}
