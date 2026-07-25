use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// "system" (admin-managed) or "user" (self-service)
    #[serde(default)]
    pub scope: String,
    /// For system rules: target user_id or "*" for all.
    /// For user rules: the rule owner's user_id.
    #[serde(default)]
    pub user_id: String,
    /// Incoming model name/pattern to match.
    /// User rules: exact match only.
    /// System rules: supports glob "*" pattern.
    #[serde(default)]
    pub source_model: String,
    /// Model name rewrite — when set, the request model is rewritten to this
    /// before further routing. Empty string means no rewrite.
    #[serde(default)]
    pub target_model: String,
    /// Channel override (system rules only). Empty = let model console decide.
    #[serde(default)]
    pub channel_id: String,
    /// Upstream model name override (system rules only). Empty = use default.
    #[serde(default)]
    pub upstream_model: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn default_enabled() -> bool {
    true
}
