use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRule {
    pub name: String,
    #[serde(default)]
    pub user_id: String,
    pub model_pattern: String,
    pub channel_id: String,
}
