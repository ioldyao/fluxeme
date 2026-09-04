use serde::{Deserialize, Serialize};

/// Scheduler policy for one endpoint within one model's endpoint set.
///
/// The scheduler is endpoint-centric: a model binds channels (groups), the
/// channels' endpoints are flattened into one candidate set, and scheduling
/// parameters live on the endpoint. `channel_id` is relational metadata (used
/// for FK integrity, admin grouping, and same-channel recovery scope), not a
/// scheduling level — the model never schedules "channel then endpoint".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerEndpointPolicy {
    pub model_id: String,
    pub channel_id: String,
    pub endpoint_id: i64,
    #[serde(default = "default_weight")]
    pub weight: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

fn default_weight() -> u32 {
    1
}
