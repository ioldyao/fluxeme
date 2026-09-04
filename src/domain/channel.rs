use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub provider: String,
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
    #[allow(dead_code)]
    #[serde(skip)]
    pub channel_id: String,
    pub url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// When true, use `url` exactly without appending a provider path.
    #[serde(default)]
    pub full_url: bool,
}

fn default_enabled() -> bool {
    true
}
