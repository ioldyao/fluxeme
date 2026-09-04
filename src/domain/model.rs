use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct MarketplaceModel {
    pub name: String,
    pub pricing: Pricing,
    pub context_length: Option<i64>,
    pub category: String,
    pub formats: MarketplaceFormats,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct MarketplaceFormats {
    pub openai: bool,
    pub anthropic: bool,
}

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
    #[serde(default, with = "rust_decimal::serde::float")]
    pub prompt_price: Decimal,
    #[serde(default, with = "rust_decimal::serde::float")]
    pub completion_price: Decimal,
    #[serde(default, with = "rust_decimal::serde::float")]
    pub cache_read_price: Decimal,
    #[serde(default, with = "rust_decimal::serde::float")]
    pub cache_write_price: Decimal,
    #[serde(default, with = "rust_decimal::serde::float")]
    pub image_input_price: Decimal,
    #[serde(default, with = "rust_decimal::serde::float")]
    pub audio_input_price: Decimal,
    #[serde(default, with = "rust_decimal::serde::float")]
    pub audio_output_price: Decimal,
}

impl Default for Pricing {
    fn default() -> Self {
        Self {
            prompt_price: Decimal::ZERO,
            completion_price: Decimal::ZERO,
            cache_read_price: Decimal::ZERO,
            cache_write_price: Decimal::ZERO,
            image_input_price: Decimal::ZERO,
            audio_input_price: Decimal::ZERO,
            audio_output_price: Decimal::ZERO,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChannel {
    #[serde(skip)]
    #[allow(dead_code)]
    pub model_id: String,
    /// Surrogate DB primary key of this binding row (model_channels.id).
    /// Kept for override FK integrity; not serialized to the client.
    #[serde(skip)]
    #[allow(dead_code)]
    pub binding_id: Option<i64>,
    pub channel_id: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Populated on read by joining with channels.provider.
    #[serde(default)]
    pub provider: String,
    /// Per-channel upstream model name override.
    /// When set, the upstream receives this name instead of the user-facing
    /// model name. Different channels may expose the same logical model
    /// under different internal names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
    /// Per-channel cap on the upstream `max_tokens` this model may request.
    /// When set, the scheduler clamps the request's `max_tokens` down to this
    /// value before hitting the upstream (some upstreams reject larger values
    /// with a 500). `None` leaves the request untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Per-endpoint weight overrides for this binding. Each entry only
    /// overrides one endpoint; endpoints not listed inherit the channel
    /// default weight. Empty = fully inherit channel defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoint_weight_overrides: Vec<EndpointWeightOverride>,
}

/// One endpoint-level weight override for a model-channel binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointWeightOverride {
    pub endpoint_id: i64,
    pub weight: u32,
}

fn default_priority() -> i32 {
    1
}
