//! # fluxeme-contract
//!
//! SkillHub ↔ Skill Runtime 之间的**跨域最小契约**。
//!
//! ## 规则（禁止违反）
//!
//! - 只放跨 bounded context 的东西：`SkillId` / `SkillVersionId` / `SkillSlug` /
//!   `SkillManifest` / 领域事件 / Port 接口。
//! - **禁止**放入：各端 HTTP DTO、DB Row、泛化 Repository、common 垃圾桶。
//! - `skillhub` 与 `skill-backing` 只能依赖本 crate，**互相禁止 import**
//!   （编译期红线）。
//! - 契约版本 [`CONTRACT_VERSION`]：跨域 schema 的破坏性变更必须 bump，
//!   并显式标注。

/// 契约版本（semver）。跨域 schema 的破坏性变更必须 bump。
///
/// 0.2.0（破坏性）：EndpointDecl 增加 upstream/timeout_ms；RuntimeSkillManifest
/// 改为携带原始 SKILL.md + fluxeme.yaml（解释权移交 Runtime）；新增
/// ApiKeyAuthorizer / RuntimeMeter Port；SkillRuntimeCatalog 增加 resolve_by_id。
pub const CONTRACT_VERSION: &str = "0.2.1";

use std::fmt;

use serde::{Deserialize, Serialize};

// ── 标识符（newtype 包装，避免裸 String 散落） ──────────────────────────

/// Skill 的不透明标识符（内部 UUID 字符串）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillId(pub String);

/// Skill 版本的不透明标识符。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillVersionId(pub String);

/// Skill 的稳定、人类可读 slug（公开 URL 用，如 `hpc3-slurm`）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillSlug(pub String);

impl fmt::Display for SkillSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ── SkillManifest ──────────────────────────────────────────────────────
// 领域命名是 Agent Skill（厂商无关），不是 Claude Skill。本类型为技能的
// 机器可读清单：SkillHub 原样保存，解释权在 Skill Runtime。

/// 技能包机器可读清单（解析自 SKILL.md frontmatter + `backing-api` 扩展块）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: SkillSlug,
    pub version: String,
    /// `backing-api` 扩展块：技能声明的运行时端点。SkillHub 不解释，
    /// 原样跨域传递。
    #[serde(default)]
    pub endpoints: Vec<EndpointDecl>,
}

/// 运行时端点声明（manifest 的 backing-api 部分）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDecl {
    pub name: String,
    pub method: String,
    /// 对外公开路径（如 `/jobs`）。
    pub path: String,
    /// 上游 URL（部署时经 SSRF 上游策略校验后才允许注册）。
    pub upstream: String,
    /// 上游超时（毫秒），缺省用运行时默认。
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

// ── 领域事件 ────────────────────────────────────────────────────────────
// 事件归属：SkillPublished 是控制面事实（SkillHub 发）；
// SkillRuntime* 是运行面事实（Skill Runtime 发）。Hub 不宣布自己
// 无法保证的部署结果。

/// SkillHub 拥有的事件（控制面事实）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkillHubEvent {
    SkillPublished {
        skill: SkillId,
        version: SkillVersionId,
    },
    SkillDisabled {
        skill: SkillId,
    },
}

/// Skill Runtime 拥有的事件（运行面事实）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeEvent {
    SkillRuntimeReady {
        skill: SkillId,
        version: SkillVersionId,
    },
    SkillRuntimeFailed {
        skill: SkillId,
        version: SkillVersionId,
        reason: String,
    },
    SkillRuntimeDisabled {
        skill: SkillId,
    },
}

// ── Port 接口 ───────────────────────────────────────────────────────────
// Port 命名是"能力"（runtime catalog），不是泛化 Repository —— Runtime
// 不该知道 SkillHub 的仓储模型。

/// Port：Skill Runtime 只通过此接口读取"发布态技能怎么运行"。
/// 由 SkillHub 实现；Skill Runtime 依赖此 trait（不反向 import Hub）。
#[async_trait::async_trait]
pub trait SkillRuntimeCatalog: Send + Sync {
    /// 按 slug + 版本解析运行所需信息（HTTP 请求链用）。
    async fn resolve(
        &self,
        slug: &SkillSlug,
        version: &str,
    ) -> Result<RuntimeSkillManifest, ContractError>;

    /// 按调用主体解析，必须同时执行技能 visibility/ACL 门禁。
    async fn resolve_for(
        &self,
        slug: &SkillSlug,
        version: &str,
        _principal: &RuntimePrincipal,
    ) -> Result<RuntimeSkillManifest, ContractError> {
        self.resolve(slug, version).await
    }

    /// 按内部 id 解析（部署流程用：outbox 任务里只有 skill_id/version_id）。
    async fn resolve_by_id(
        &self,
        skill: &SkillId,
        version: &SkillVersionId,
    ) -> Result<RuntimeSkillManifest, ContractError>;
}

/// `resolve` 的结果：SkillHub 原样保存，解释权在 Skill Runtime。
/// `manifest_yaml` 为 fluxeme.yaml 原文（backing-api 运行时声明），
/// 由 Runtime 解析成 `SkillManifest` 并执行 SSRF/部署策略。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSkillManifest {
    pub skill: SkillId,
    pub slug: SkillSlug,
    pub version: String,
    /// 控制面当前发布版本的稳定 ID，runtime 查询 endpoint 时必须绑定此 ID。
    #[serde(default)]
    pub version_id: Option<SkillVersionId>,
    /// SKILL.md 原文（Agent Skill 标准正文）。
    pub source_markdown: Option<String>,
    /// fluxeme.yaml 原文（backing-api 声明）。SkillHub 不解释。
    pub manifest_yaml: Option<String>,
    pub artifact_path: Option<String>,
}

/// Port：技能包存储抽象。LocalArtifactStore 起步，S3/MinIO 可无痛替换，
/// 避免多实例 SkillHub 时本地磁盘成为瓶颈。
#[async_trait::async_trait]
pub trait SkillArtifactStore: Send + Sync {
    async fn put(&self, key: &str, bytes: Vec<u8>) -> Result<(), ContractError>;
    async fn get(&self, key: &str) -> Result<Vec<u8>, ContractError>;
    async fn delete(&self, key: &str) -> Result<(), ContractError>;
}

/// API Key 鉴权主体（请求链 ① 的结果）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePrincipal {
    pub user_id: String,
    pub api_key_id: String,
    /// Whether the authenticated owner has administrative access to SkillHub.
    #[serde(default)]
    pub is_admin: bool,
}

/// Port：Skill Runtime 的请求鉴权（请求链 ③）。
/// 由根实现：现有 API Key 查找 + `api_key_scopes` 校验
/// （resource_type ∈ {skill, model, mcp}；action ∈ {invoke, connect}）。
/// Backing 不接触根侧 AuthService/DB。
#[async_trait::async_trait]
pub trait ApiKeyAuthorizer: Send + Sync {
    /// 校验 bearer key（`sk_...`）对该资源是否有指定 action 权限。
    /// 未授权返回 `ContractError::NotFound` 以隐藏资源存在性。
    async fn authorize(
        &self,
        bearer: &str,
        resource_type: &str,
        resource_id: &str,
        action: &str,
    ) -> Result<RuntimePrincipal, ContractError>;
}

/// 一次 Skill Runtime 调用（数据面计量，可观测归属 ClickHouse）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeUsageRecord {
    pub skill: SkillId,
    pub slug: SkillSlug,
    pub version: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub latency_ms: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub user_id: String,
    pub api_key_id: String,
}

/// Port：Skill Runtime 调用的计量/计费钩子（请求链 ⑥）。
/// 由根实现：写 ClickHouse 观测 + PostgreSQL `billing_events`
/// （钱包/账单等财务事实永远以 PG 为准）。
#[async_trait::async_trait]
pub trait RuntimeMeter: Send + Sync {
    async fn record(&self, record: RuntimeUsageRecord) -> Result<(), ContractError>;
}

// ── 跨域错误码（契约级） ────────────────────────────────────────────────
// 各子系统负责把契约错误映射到自己的 HTTP 错误形态。

/// 契约级错误。
#[derive(Debug, Clone)]
pub enum ContractError {
    NotFound(String),
    Invalid(String),
    Conflict(String),
    Storage(String),
    Internal(String),
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContractError::NotFound(m) => write!(f, "not found: {m}"),
            ContractError::Invalid(m) => write!(f, "invalid: {m}"),
            ContractError::Conflict(m) => write!(f, "conflict: {m}"),
            ContractError::Storage(m) => write!(f, "storage: {m}"),
            ContractError::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl std::error::Error for ContractError {}
