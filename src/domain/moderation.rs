use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentFilterRule {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_pattern_type")]
    pub pattern_type: String,
    pub pattern: String,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_priority")]
    pub priority: i32,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_pattern_type() -> String {
    "keyword".to_string()
}

fn default_action() -> String {
    "block".to_string()
}

fn default_scope() -> String {
    "both".to_string()
}

fn default_enabled() -> bool {
    true
}

fn default_priority() -> i32 {
    1
}
