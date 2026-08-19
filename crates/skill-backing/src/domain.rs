//! Skill Runtime / Backing 域模型（数据面）。
//!
//! 运行状态机：`pending → deploying → ready / failed → disabled`。
//! 运行状态属于 Runtime（不是 SkillHub），只反映在 Runtime 自有的表上。

use serde::Serialize;

/// 技能级运行状态（由 endpoint 聚合推导，非独立列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeState {
    Pending,
    Ready,
    Failed,
    Disabled,
}

impl RuntimeState {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuntimeState::Pending => "pending",
            RuntimeState::Ready => "ready",
            RuntimeState::Failed => "failed",
            RuntimeState::Disabled => "disabled",
        }
    }
}

/// `agent_skill_endpoints` 行（绑 `skill_version_id`，发布新版本不覆盖旧版本）。
/// slug/version 为 Runtime 侧反规范化副本（状态聚合直接可读，无需跨域查询）。
#[derive(Debug, Clone, Serialize)]
pub struct EndpointRow {
    pub id: String,
    pub skill_id: String,
    pub skill_version_id: String,
    pub slug: String,
    pub version: String,
    pub endpoint_name: String,
    pub method: String,
    pub public_path: String,
    pub upstream_url: String,
    pub upstream_path: Option<String>,
    pub timeout_ms: i64,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// outbox 任务（消费侧视图）。
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub id: String,
    pub skill_id: String,
    pub version_id: String,
    pub event_type: String,
    pub payload: String,
    pub status: String,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub created_at: String,
    pub processed_at: Option<String>,
}

/// `agent_skill_runtime_events` 行（控制面事件，不存每次 HTTP 调用）。
#[derive(Debug, Clone)]
pub struct RuntimeEventRow {
    pub id: String,
    pub skill_id: String,
    pub version_id: Option<String>,
    pub event_type: String,
    pub detail: Option<String>,
    pub created_at: String,
}

/// 技能级运行状态汇总（UI 展示"运行状态"列）。
#[derive(Debug, Clone, Serialize)]
pub struct SkillRuntimeStatus {
    pub skill_id: String,
    pub slug: String,
    pub version: String,
    pub state: String,
}
