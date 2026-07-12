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
    #[serde(default)]
    pub published: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<i64>,
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pricing {
    #[serde(default)]
    pub prompt_price: f64,
    #[serde(default)]
    pub completion_price: f64,
    #[serde(default)]
    pub cache_read_price: f64,
    #[serde(default)]
    pub cache_write_price: f64,
    #[serde(default)]
    pub image_input_price: f64,
    #[serde(default)]
    pub audio_input_price: f64,
    #[serde(default)]
    pub audio_output_price: f64,
}

impl Default for Pricing {
    fn default() -> Self {
        Self {
            prompt_price: 0.0,
            completion_price: 0.0,
            cache_read_price: 0.0,
            cache_write_price: 0.0,
            image_input_price: 0.0,
            audio_input_price: 0.0,
            audio_output_price: 0.0,
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
    /// Populated on read by joining with channels.provider.
    #[serde(default)]
    pub provider: String,
}

fn default_priority() -> i32 {
    1
}
