//! SkillHub 域模型（控制面）。
//!
//! 命名采用 **Agent Skill**（厂商无关），数据模型不绑定任何具体 agent 工具。
//! 行结构直接派生 Serialize，组合根可将它们作为 HTTP JSON 返回（阶段 1 的内部 API）。

use serde::{Deserialize, Serialize};

/// 包状态机（SkillHub 拥有）：`draft → reviewing → approved → published`。
///
/// 注意：`published` 只代表"控制面已发布"，**不代表运行时可用**。
/// 运行可用性是 Skill Runtime 的 `RuntimeState`（阶段 2），安装/调用门禁 =
/// `published AND runtime_ready`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageStatus {
    Draft,
    Reviewing,
    Approved,
    Published,
    Disabled,
}

impl PackageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PackageStatus::Draft => "draft",
            PackageStatus::Reviewing => "reviewing",
            PackageStatus::Approved => "approved",
            PackageStatus::Published => "published",
            PackageStatus::Disabled => "disabled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(Self::Draft),
            "reviewing" => Some(Self::Reviewing),
            "approved" => Some(Self::Approved),
            "published" => Some(Self::Published),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }
}

/// 可见性：`public` 所有人可见 / `internal` 内部团队可见 / `private` 私有。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Internal,
    Private,
}

/// 用户端访问上下文。团队 ACL 接入前，作者和管理员是 internal/private 的
/// 最小安全可见范围；public 仍可按目录策略访问。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillAccessContext {
    pub user_id: String,
    pub is_admin: bool,
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Internal => "internal",
            Visibility::Private => "private",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "public" => Some(Self::Public),
            "internal" => Some(Self::Internal),
            "private" => Some(Self::Private),
            _ => None,
        }
    }
}

/// `agent_skills` 行。
#[derive(Debug, Clone, Serialize)]
pub struct SkillRow {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub author_id: String,
    /// 当前已上传版本（0.0.0 = 尚未上传任何包）。
    pub version: String,
    pub artifact_path: Option<String>,
    pub artifact_size: i64,
    /// SKILL.md 原样文本（预览/渲染用）。
    pub source_markdown: Option<String>,
    pub visibility: String,
    pub status: String,
    pub published_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// 累计下载次数（download 成功后自增，管理端展示）。
    pub download_count: i64,
    /// 当前对外发布的版本 ID；与旧 `version` 字段兼容并逐步取代它。
    pub published_version_id: Option<String>,
}

/// `agent_skill_versions` 行（版本历史，独立于 skill 当前版本）。
#[derive(Debug, Clone, Serialize)]
pub struct SkillVersionRow {
    pub id: String,
    pub skill_id: String,
    pub version: String,
    pub changelog: Option<String>,
    pub artifact_path: Option<String>,
    pub artifact_size: i64,
    pub source_markdown: Option<String>,
    /// fluxeme.yaml 原文（backing-api 运行时声明）。SkillHub 原样保存，
    /// 解释权在 Skill Runtime。
    pub manifest_yaml: Option<String>,
    pub status: String,
    pub created_by: String,
    pub created_at: String,
}

/// outbox 传输任务（`agent_skill_runtime_tasks`）。
///
/// 这是**传输/基础设施**，不是域数据：SkillHub（publisher）在状态变更的
/// 同一事务内写入，Skill Runtime（consumer）认领时更新 claim 字段。
/// 两个子系统都按此结构读写，互不 import。
#[derive(Debug, Clone)]
pub struct RuntimeTaskRow {
    pub id: String,
    pub skill_id: String,
    pub version_id: String,
    /// skill_published / skill_disabled / skill_version_deployed
    pub event_type: String,
    /// JSON 字符串（携带 slug/version 等身份信息）
    pub payload: String,
    /// pending / processing / done / failed
    pub status: String,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub created_at: String,
    pub processed_at: Option<String>,
}

// ── 输入 ──────────────────────────────────────────────────────────────

/// 新建技能（管理端）。
#[derive(Debug, Clone)]
pub struct CreateSkill {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub visibility: Visibility,
    pub author_id: String,
}

/// 更新技能（管理端）。
#[derive(Debug, Clone, Default)]
pub struct UpdateSkill {
    pub name: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub tags: Option<Vec<String>>,
    pub visibility: Option<Visibility>,
}
