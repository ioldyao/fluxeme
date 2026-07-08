use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub model_pattern: String,
    #[serde(default)]
    pub pricing: Pricing,
    #[serde(default)]
    pub channels: Vec<ModelChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pricing {
    #[serde(default)]
    pub prompt_price: f64,
    #[serde(default)]
    pub completion_price: f64,
}

impl Default for Pricing {
    fn default() -> Self {
        Self {
            prompt_price: 0.0,
            completion_price: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChannel {
    #[serde(skip)]
    #[allow(dead_code)]
    pub model_id: String,
    pub channel_id: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_priority() -> i32 {
    1
}
