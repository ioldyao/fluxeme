//! # fluxeme-skillhub
//!
//! SkillHub 控制面子系统：**目录 / 版本 / 安装 / 包存储 / 审批**。
//!
//! 独立性：
//! - 本 crate 只依赖 `fluxeme-contract`（+ 基础设施），**不依赖** `skill-backing`。
//! - 自持 PostgreSQL 连接池与自己的 schema 迁移（业务数据归属 PG）。
//! - 不感知 HTTP：组合根（根二进制）负责把本模块接到 axum handler。
//!
//! 领域命名采用 **Agent Skill**（厂商无关），数据模型不绑定任何具体 agent 工具。

pub mod artifact;
pub mod domain;
pub mod error;
pub mod repo;

use std::io::Read;
use std::path::PathBuf;

use async_trait::async_trait;
use fluxeme_contract::{
    ContractError, RuntimeSkillManifest, SkillArtifactStore, SkillId, SkillRuntimeCatalog,
    SkillSlug, SkillVersionId,
};

use crate::artifact::LocalArtifactStore;
use crate::domain::{
    CreateSkill, PackageStatus, RuntimeTaskRow, SkillAccessContext, SkillRow, SkillVersionRow,
    UpdateSkill, Visibility,
};
pub use crate::error::SkillHubError;
use crate::repo::SkillRepository;

/// 单个技能包上限（50 MB）。
pub const MAX_ARTIFACT_BYTES: usize = 50 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 256;
const MAX_ZIP_UNCOMPRESSED_BYTES: u64 = 100 * 1024 * 1024;
const MAX_SKILL_MARKDOWN_BYTES: usize = 512 * 1024;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;

/// 下载/安装 的结果载荷。
pub struct DownloadPayload {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub size: i64,
}

/// SkillHub 控制面入口。组合根创建并注入 AppState。
pub struct SkillHubModule {
    repo: SkillRepository,
    store: Box<dyn SkillArtifactStore>,
    #[allow(dead_code)]
    artifact_root: PathBuf,
}

impl SkillHubModule {
    /// `pool` 为 PostgreSQL 连接池（业务数据归属 PG）；`artifact_root` 为
    /// 技能包落盘根目录（如 `data/skills`）。
    pub fn new(pool: sqlx_postgres::PgPool, artifact_root: PathBuf) -> Self {
        let repo = SkillRepository::new(pool);
        let store = Box::new(LocalArtifactStore::new(artifact_root.clone()));
        Self {
            repo,
            store,
            artifact_root,
        }
    }

    /// 建表（自洽子系统的 schema 归自己管）。组合根在启动时调用。
    pub async fn migrate(&self) -> Result<(), SkillHubError> {
        self.repo.migrate().await
    }

    // ── 管理端：目录 ───────────────────────────────────────────────────

    pub async fn create_skill(&self, input: CreateSkill) -> Result<SkillRow, SkillHubError> {
        let slug = validate_slug(&input.slug)?;
        if input.name.trim().is_empty() {
            return Err(SkillHubError::Invalid("name is required".into()));
        }
        if self.repo.get_skill_by_slug(&slug).await?.is_some() {
            return Err(SkillHubError::Conflict(format!(
                "slug '{slug}' already exists"
            )));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let row = SkillRow {
            id: uuid::Uuid::new_v4().to_string(),
            slug,
            name: input.name.trim().to_string(),
            description: input.description.trim().to_string(),
            category: if input.category.trim().is_empty() {
                "general".to_string()
            } else {
                input.category.trim().to_string()
            },
            tags: input.tags,
            author_id: input.author_id,
            version: "0.0.0".to_string(),
            artifact_path: None,
            artifact_size: 0,
            source_markdown: None,
            visibility: input.visibility.as_str().to_string(),
            status: PackageStatus::Draft.as_str().to_string(),
            published_at: None,
            created_at: now.clone(),
            updated_at: now,
            download_count: 0,
            published_version_id: None,
        };
        self.repo.insert_skill(&row).await?;
        Ok(row)
    }

    pub async fn list_skills(
        &self,
        status: Option<&str>,
        visibility: Option<&str>,
    ) -> Result<Vec<SkillRow>, SkillHubError> {
        self.repo.list_skills(status, visibility).await
    }

    pub async fn get_skill(&self, id: &str) -> Result<Option<SkillRow>, SkillHubError> {
        self.repo.get_skill_by_id(id).await
    }

    pub async fn update_skill(
        &self,
        id: &str,
        input: UpdateSkill,
    ) -> Result<SkillRow, SkillHubError> {
        let mut existing = self
            .repo
            .get_skill_by_id(id)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))?;
        if let Some(v) = input.name {
            if v.trim().is_empty() {
                return Err(SkillHubError::Invalid("name cannot be empty".into()));
            }
            existing.name = v.trim().to_string();
        }
        if let Some(v) = input.description {
            existing.description = v.trim().to_string();
        }
        if let Some(v) = input.category {
            existing.category = if v.trim().is_empty() {
                "general".into()
            } else {
                v.trim().into()
            };
        }
        if let Some(v) = input.tags {
            existing.tags = v;
        }
        if let Some(v) = input.visibility {
            existing.visibility = v.as_str().to_string();
        }
        existing.updated_at = chrono::Utc::now().to_rfc3339();
        self.repo.update_skill(&existing).await?;
        Ok(existing)
    }

    pub async fn delete_skill(&self, id: &str) -> Result<(), SkillHubError> {
        let skill = self
            .repo
            .get_skill_by_id(id)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))?;
        // 版本/安装记录由 DB FK ON DELETE CASCADE 级联；顺带清理包文件。
        if let Some(path) = skill.artifact_path {
            let _ = self.store.delete(&path).await;
        }
        if let Ok(versions) = self.repo.list_versions(id).await {
            for v in versions {
                if let Some(p) = v.artifact_path {
                    let _ = self.store.delete(&p).await;
                }
            }
        }
        self.repo.delete_skill(id).await
    }

    /// 包状态机流转。`published` 只代表控制面发布；运行时可用性由
    /// Skill Runtime 决定（阶段 2），此处不承诺。
    pub async fn set_status(
        &self,
        id: &str,
        status: PackageStatus,
        version_id: Option<&str>,
    ) -> Result<SkillRow, SkillHubError> {
        let skill = self
            .repo
            .get_skill_by_id(id)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))?;
        if skill.status == status.as_str() && version_id.is_none() {
            return Ok(skill);
        }
        if !valid_status_transition(&skill.status, status) {
            return Err(SkillHubError::Invalid(format!(
                "invalid skill status transition: {} -> {}",
                skill.status,
                status.as_str()
            )));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let selected_version = match version_id {
            Some(id) => self.repo.get_version_by_id(id).await?,
            None => self.repo.latest_version(&skill.id).await?,
        };
        if let Some(candidate) = &selected_version {
            if candidate.skill_id != skill.id {
                return Err(SkillHubError::Invalid(
                    "version does not belong to skill".into(),
                ));
            }
        }
        let published_version_id = if status == PackageStatus::Published {
            let candidate = selected_version.as_ref().ok_or_else(|| {
                SkillHubError::Invalid("publishing requires an uploaded version".into())
            })?;
            if candidate.artifact_path.is_none()
                || !matches!(candidate.status.as_str(), "approved" | "published")
            {
                return Err(SkillHubError::Invalid(
                    "version must be approved and have an artifact before publishing".into(),
                ));
            }
            Some(candidate.id.clone())
        } else {
            None
        };
        let task_version_id = published_version_id
            .clone()
            .or_else(|| skill.published_version_id.clone());
        let task = match status {
            PackageStatus::Published => Some(self.build_task(
                &skill,
                task_version_id.as_deref().ok_or_else(|| {
                    SkillHubError::Invalid("skill has no published version".into())
                })?,
                "skill_published",
            )),
            PackageStatus::Disabled if skill.status == PackageStatus::Published.as_str() => {
                Some(self.build_task(
                    &skill,
                    task_version_id.as_deref().ok_or_else(|| {
                        SkillHubError::Invalid("skill has no published version".into())
                    })?,
                    "skill_disabled",
                ))
            }
            _ => None,
        };
        self.repo
            .set_status_with_task(
                id,
                status.as_str(),
                if status == PackageStatus::Published {
                    Some(now.as_str())
                } else {
                    None
                },
                published_version_id.as_deref(),
                if matches!(status, PackageStatus::Reviewing | PackageStatus::Approved) {
                    selected_version.as_ref().map(|v| v.id.as_str())
                } else {
                    None
                },
                task.as_ref(),
            )
            .await?;
        self.repo
            .get_skill_by_id(id)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))
    }

    pub async fn list_versions(
        &self,
        skill_id: &str,
    ) -> Result<Vec<SkillVersionRow>, SkillHubError> {
        self.repo.list_versions(skill_id).await
    }

    /// 上传技能包 zip：校验 zip（必须含根目录 SKILL.md）→ 落盘 →
    /// 写入草稿版本。上传候选版本不会改变当前发布版本，发布必须由显式
    /// 的版本状态流转完成。
    pub async fn upload_artifact(
        &self,
        skill_id: &str,
        version: &str,
        changelog: Option<&str>,
        created_by: &str,
        original_filename: &str,
        bytes: Vec<u8>,
    ) -> Result<SkillVersionRow, SkillHubError> {
        validate_version(version)?;
        if bytes.is_empty() || bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(SkillHubError::Invalid(format!(
                "artifact size must be 1..={} bytes",
                MAX_ARTIFACT_BYTES
            )));
        }
        let _skill = self
            .repo
            .get_skill_by_id(skill_id)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))?;
        if self.repo.get_version(skill_id, version).await?.is_some() {
            return Err(SkillHubError::Conflict(format!(
                "version '{version}' already exists"
            )));
        }
        let source_markdown = extract_skill_md(&bytes)?;
        let manifest_yaml = extract_manifest_yaml(&bytes)?;
        // 存储保留原始文件名：`{skill_id}/{version}/{原始文件名}`。
        // 版本作目录隔离（同名文件不同版本不互相覆盖），文件名保留用户原始命名；
        // 原始名非法时回退 `{version}.zip`。
        let filename = sanitize_filename(original_filename);
        let filename = if filename.is_empty() {
            format!("{version}.zip")
        } else {
            filename
        };
        let key = format!("{skill_id}/{version}/{filename}");
        self.store
            .put(&key, bytes.clone())
            .await
            .map_err(storage_err)?;

        let now = chrono::Utc::now().to_rfc3339();
        let vrow = SkillVersionRow {
            id: uuid::Uuid::new_v4().to_string(),
            skill_id: skill_id.to_string(),
            version: version.to_string(),
            changelog: changelog.map(|s| s.to_string()),
            artifact_path: Some(key.clone()),
            artifact_size: bytes.len() as i64,
            source_markdown: Some(source_markdown.clone()),
            manifest_yaml,
            status: PackageStatus::Draft.as_str().to_string(),
            created_by: created_by.to_string(),
            created_at: now,
        };
        // 版本上传只创建候选草稿。即使技能当前已发布，也必须先经过
        // reviewing/approved，再由显式发布动作切换对外版本。
        self.repo.upload_artifact_with_task(&vrow, None).await?;
        Ok(vrow)
    }

    /// 构造 outbox 传输任务（payload 携带 slug/version 身份信息）。
    fn build_task(&self, skill: &SkillRow, version_id: &str, event_type: &str) -> RuntimeTaskRow {
        RuntimeTaskRow {
            id: uuid::Uuid::new_v4().to_string(),
            skill_id: skill.id.clone(),
            version_id: version_id.to_string(),
            event_type: event_type.to_string(),
            payload: serde_json::json!({
                "slug": skill.slug,
                "version": skill.version,
            })
            .to_string(),
            status: "pending".to_string(),
            attempts: 0,
            last_error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            processed_at: None,
        }
    }

    // ── 用户端：目录 / 安装 / 下载 ─────────────────────────────────────

    /// 发布态目录（门禁：status=published 且 visibility=public）。
    /// internal/private 访问需要显式的主体上下文，不能由登录状态默认放行。
    pub async fn list_published_skills(&self) -> Result<Vec<SkillRow>, SkillHubError> {
        self.list_published_skills_for(None).await
    }

    pub async fn list_published_skills_for(
        &self,
        access: Option<&SkillAccessContext>,
    ) -> Result<Vec<SkillRow>, SkillHubError> {
        let skills = self.repo.list_skills(Some("published"), None).await?;
        Ok(skills
            .into_iter()
            .filter(|skill| can_view_skill(skill, access))
            .collect())
    }

    /// 发布态详情（根据 visibility/ACL 过滤）。
    pub async fn get_published_skill(&self, slug: &str) -> Result<Option<SkillRow>, SkillHubError> {
        self.get_published_skill_for(slug, None).await
    }

    pub async fn get_published_skill_for(
        &self,
        slug: &str,
        access: Option<&SkillAccessContext>,
    ) -> Result<Option<SkillRow>, SkillHubError> {
        let skill = self.repo.get_skill_by_slug(slug).await?;
        Ok(skill.filter(|s| can_view_skill(s, access)))
    }

    /// 发布态技能的版本列表（用户端详情页用，仅已发布技能可见）。
    pub async fn list_published_versions(
        &self,
        slug: &str,
    ) -> Result<Vec<SkillVersionRow>, SkillHubError> {
        self.list_published_versions_for(slug, None).await
    }

    pub async fn list_published_versions_for(
        &self,
        slug: &str,
        access: Option<&SkillAccessContext>,
    ) -> Result<Vec<SkillVersionRow>, SkillHubError> {
        let skill = self
            .repo
            .get_skill_by_slug(slug)
            .await?
            .filter(|s| can_view_skill(s, access))
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))?;
        let versions = self.repo.list_versions(&skill.id).await?;
        Ok(versions
            .into_iter()
            .filter(|v| {
                v.status == PackageStatus::Published.as_str()
                    && skill.published_version_id.as_deref() == Some(v.id.as_str())
            })
            .collect())
    }

    /// 下载技能包。门禁：`published AND 包已上传`。
    /// 阶段 1 以"包存在"代理 runtime_ready（阶段 2 由 Skill Runtime 接管）。
    pub async fn download(
        &self,
        slug: &str,
        version: Option<&str>,
    ) -> Result<DownloadPayload, SkillHubError> {
        self.download_for(slug, version, None).await
    }

    pub async fn download_for(
        &self,
        slug: &str,
        version: Option<&str>,
        access: Option<&SkillAccessContext>,
    ) -> Result<DownloadPayload, SkillHubError> {
        let skill = self
            .get_published_skill_for(slug, access)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))?;
        let version_id = skill
            .published_version_id
            .as_deref()
            .ok_or_else(|| SkillHubError::NotFound("published version".into()))?;
        let vrow = self
            .repo
            .get_version_by_id(version_id)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("version".into()))?;
        if version.is_some_and(|requested| requested != vrow.version) {
            return Err(SkillHubError::NotFound("version".into()));
        }
        let version = vrow.version.clone();
        if vrow.status != PackageStatus::Published.as_str() {
            return Err(SkillHubError::NotFound("version".into()));
        }
        let path = vrow
            .artifact_path
            .ok_or_else(|| SkillHubError::NotFound("artifact".into()))?;
        let bytes = self.store.get(&path).await.map_err(storage_err)?;
        // 只在文件成功读取后计数，失败下载不污染统计。
        let _ = self.repo.increment_download_count(&skill.id).await?;
        Ok(DownloadPayload {
            filename: format!("{slug}-{version}.zip"),
            content_type: "application/zip".to_string(),
            size: bytes.len() as i64,
            bytes,
        })
    }
}

// ── SkillRuntimeCatalog Port 实现 ───────────────────────────────────────
// SkillHub 实现契约 Port，Skill Runtime 只依赖 contract 类型消费。
// resolve 返回 SkillHub 原样保存的原始材料（SKILL.md / fluxeme.yaml），
// 由 Runtime 自行解释并执行 SSRF/部署策略。

#[async_trait]
impl SkillRuntimeCatalog for SkillHubModule {
    async fn resolve(
        &self,
        slug: &SkillSlug,
        version: &str,
    ) -> Result<RuntimeSkillManifest, ContractError> {
        let skill = self
            .repo
            .get_skill_by_slug(&slug.0)
            .await
            .map_err(dberr_to_contract)?
            .filter(|s| {
                s.status == PackageStatus::Published.as_str()
                    && s.visibility == Visibility::Public.as_str()
            })
            .ok_or_else(|| ContractError::NotFound(format!("skill {slug}")))?;
        let version_id = if version.is_empty() {
            skill
                .published_version_id
                .as_deref()
                .ok_or_else(|| ContractError::NotFound("published version".into()))?
        } else {
            return Err(ContractError::Invalid(
                "runtime resolution requires the current published version".into(),
            ));
        };
        let vrow = self
            .repo
            .get_version_by_id(version_id)
            .await
            .map_err(dberr_to_contract)?
            .ok_or_else(|| ContractError::NotFound("published version".into()))?;
        if vrow.skill_id != skill.id || vrow.status != PackageStatus::Published.as_str() {
            return Err(ContractError::NotFound("published version".into()));
        }
        Ok(RuntimeSkillManifest {
            skill: SkillId(skill.id.clone()),
            slug: slug.clone(),
            version: vrow.version,
            version_id: Some(SkillVersionId(vrow.id.clone())),
            source_markdown: vrow.source_markdown,
            manifest_yaml: vrow.manifest_yaml,
            artifact_path: vrow.artifact_path,
        })
    }

    async fn resolve_for(
        &self,
        slug: &SkillSlug,
        version: &str,
        principal: &fluxeme_contract::RuntimePrincipal,
    ) -> Result<RuntimeSkillManifest, ContractError> {
        let skill = self
            .repo
            .get_skill_by_slug(&slug.0)
            .await
            .map_err(dberr_to_contract)?
            .filter(|s| {
                s.status == PackageStatus::Published.as_str()
                    && (s.visibility == Visibility::Public.as_str()
                        || s.author_id == principal.user_id
                        || principal.is_admin)
            })
            .ok_or_else(|| ContractError::NotFound(format!("skill {slug}")))?;
        let vrow = if version.is_empty() {
            let version_id = skill
                .published_version_id
                .as_deref()
                .ok_or_else(|| ContractError::NotFound("published version".into()))?;
            self.repo
                .get_version_by_id(version_id)
                .await
                .map_err(dberr_to_contract)?
        } else {
            self.repo
                .get_version(&skill.id, version)
                .await
                .map_err(dberr_to_contract)?
        };
        let vrow = vrow
            .filter(|v| {
                v.status == PackageStatus::Published.as_str()
                    && skill.published_version_id.as_deref() == Some(v.id.as_str())
            })
            .ok_or_else(|| ContractError::NotFound("published version".into()))?;
        Ok(RuntimeSkillManifest {
            skill: SkillId(skill.id),
            slug: slug.clone(),
            version: vrow.version,
            version_id: Some(SkillVersionId(vrow.id.clone())),
            source_markdown: vrow.source_markdown,
            manifest_yaml: vrow.manifest_yaml,
            artifact_path: vrow.artifact_path,
        })
    }

    async fn resolve_by_id(
        &self,
        skill: &SkillId,
        version: &SkillVersionId,
    ) -> Result<RuntimeSkillManifest, ContractError> {
        let vrow = self
            .repo
            .get_version_by_id(&version.0)
            .await
            .map_err(dberr_to_contract)?
            .ok_or_else(|| ContractError::NotFound("version".into()))?;
        if vrow.skill_id != skill.0 {
            return Err(ContractError::NotFound("skill".into()));
        }
        let sk = self
            .repo
            .get_skill_by_id(&skill.0)
            .await
            .map_err(dberr_to_contract)?
            .filter(|s| s.status == PackageStatus::Published.as_str())
            .ok_or_else(|| ContractError::NotFound("skill".into()))?;
        if sk.published_version_id.as_deref() != Some(&vrow.id)
            || vrow.status != PackageStatus::Published.as_str()
        {
            return Err(ContractError::NotFound("published version".into()));
        }
        Ok(RuntimeSkillManifest {
            skill: skill.clone(),
            slug: SkillSlug(sk.slug),
            version: vrow.version,
            version_id: Some(SkillVersionId(vrow.id.clone())),
            source_markdown: vrow.source_markdown,
            manifest_yaml: vrow.manifest_yaml,
            artifact_path: vrow.artifact_path,
        })
    }
}

fn dberr_to_contract(e: SkillHubError) -> ContractError {
    ContractError::Internal(e.to_string())
}

fn can_view_skill(skill: &SkillRow, access: Option<&SkillAccessContext>) -> bool {
    if skill.status != PackageStatus::Published.as_str() {
        return false;
    }
    match Visibility::parse(&skill.visibility) {
        Some(Visibility::Public) => true,
        Some(Visibility::Internal | Visibility::Private) => {
            access.is_some_and(|ctx| ctx.is_admin || ctx.user_id == skill.author_id)
        }
        None => false,
    }
}

fn valid_status_transition(current: &str, next: PackageStatus) -> bool {
    matches!(
        (current, next),
        ("draft", PackageStatus::Reviewing)
            | ("reviewing", PackageStatus::Approved)
            | ("approved", PackageStatus::Published)
            | ("published", PackageStatus::Disabled)
            | ("disabled", PackageStatus::Published)
            | ("published", PackageStatus::Published)
    )
}

// ── 校验与解析 ─────────────────────────────────────────────────────────

/// slug：小写字母/数字/连字符，首字符为字母或数字，长度 ≤64。
fn validate_slug(s: &str) -> Result<String, SkillHubError> {
    if s.is_empty() || s.len() > 64 {
        return Err(SkillHubError::Invalid("slug must be 1..=64 chars".into()));
    }
    let first = s.as_bytes()[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err(SkillHubError::Invalid(
            "slug must start with a lowercase letter or digit".into(),
        ));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(SkillHubError::Invalid(
            "slug may only contain lowercase letters, digits, and '-'".into(),
        ));
    }
    Ok(s.to_string())
}

/// 清洗原始上传文件名：去掉路径前缀（浏览器可能带 `C:\fakepath\`），
/// 丢弃危险字符与过名字符。返回空串 = 非法，调用方回退 `{version}.zip`。
fn sanitize_filename(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if base.is_empty() || base.len() > 120 || base.contains("..") || base.contains(['/', '\\']) {
        return String::new();
    }
    base
}

/// 版本：仅 `[0-9A-Za-z._-]`，禁路径分隔符与 `..`（防路径穿越），长度 ≤64。
fn validate_version(v: &str) -> Result<(), SkillHubError> {
    if v.is_empty() || v.len() > 64 {
        return Err(SkillHubError::Invalid(
            "version must be 1..=64 chars".into(),
        ));
    }
    if v.contains('/') || v.contains('\\') || v.contains("..") {
        return Err(SkillHubError::Invalid("invalid version string".into()));
    }
    if !v
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(SkillHubError::Invalid("invalid version string".into()));
    }
    Ok(())
}

/// 从 zip 中提取 `SKILL.md`（Agent Skill 标准）。
///
/// 容忍两种打包方式：`SKILL.md` 位于根目录，或位于**单个顶层目录**内
/// （用户常见地把技能包先包进一层文件夹再 zip）。报错信息明确到原因。
fn extract_skill_md(bytes: &[u8]) -> Result<String, SkillHubError> {
    let mut archive = open_zip(bytes)?;
    let entries = list_entries(&mut archive)?;

    // 1) 根目录 SKILL.md
    if let Some(idx) = entries.iter().position(|n| n == "SKILL.md") {
        return read_entry(&mut archive, idx);
    }
    // 2) 单个顶层目录内的 SKILL.md
    if let Some(prefix) = single_top_dir(&entries) {
        let target = format!("{prefix}/SKILL.md");
        if let Some(idx) = entries.iter().position(|n| n == &target) {
            return read_entry(&mut archive, idx);
        }
    }
    Err(SkillHubError::Invalid(
        "zip 缺少 SKILL.md：请把 SKILL.md 放在 zip 根目录（或仅包一层文件夹）".into(),
    ))
}

/// 从 zip 中提取 `fluxeme.yaml`（backing-api 运行时声明，可选）。
/// 同样容忍根目录或单层目录。
fn extract_manifest_yaml(bytes: &[u8]) -> Result<Option<String>, SkillHubError> {
    let mut archive = open_zip(bytes)?;
    let entries = list_entries(&mut archive)?;

    let find = |name: &str| -> Option<usize> { entries.iter().position(|n| n == name) };
    let idx = find("fluxeme.yaml")
        .or_else(|| single_top_dir(&entries).and_then(|p| find(&format!("{p}/fluxeme.yaml"))));
    match idx {
        Some(i) => {
            let mut text = read_entry(&mut archive, i)?;
            if text.len() > MAX_MANIFEST_BYTES {
                return Err(SkillHubError::Invalid(
                    "fluxeme.yaml exceeds the 256 KiB limit".into(),
                ));
            }
            text.shrink_to_fit();
            Ok(Some(text))
        }
        None => Ok(None),
    }
}

fn open_zip(bytes: &[u8]) -> Result<zip::ZipArchive<std::io::Cursor<&[u8]>>, SkillHubError> {
    let cursor = std::io::Cursor::new(bytes);
    zip::ZipArchive::new(cursor)
        .map_err(|e| SkillHubError::Invalid(format!("无效的 zip 文件：{e}")))
}

fn list_entries(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
) -> Result<Vec<String>, SkillHubError> {
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err(SkillHubError::Invalid(format!(
            "zip contains too many entries (maximum {MAX_ZIP_ENTRIES})"
        )));
    }
    let mut names = Vec::with_capacity(archive.len());
    let mut total_size = 0u64;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| SkillHubError::Invalid(format!("zip 读取错误：{e}")))?;
        let name = entry.name().to_string();
        let path = std::path::Path::new(&name);
        if path.is_absolute() || name.contains('\\') || name.split('/').any(|part| part == "..") {
            return Err(SkillHubError::Invalid(
                "zip contains an unsafe entry path".into(),
            ));
        }
        if !names.iter().all(|existing| existing != &name) {
            return Err(SkillHubError::Invalid(
                "zip contains duplicate entries".into(),
            ));
        }
        total_size = total_size.saturating_add(entry.size());
        if total_size > MAX_ZIP_UNCOMPRESSED_BYTES {
            return Err(SkillHubError::Invalid(
                "zip uncompressed size exceeds limit".into(),
            ));
        }
        names.push(name);
    }
    Ok(names)
}

/// 若所有条目都在同一个顶层目录下，返回该目录名。
fn single_top_dir(entries: &[String]) -> Option<String> {
    let mut dirs = std::collections::HashSet::new();
    for n in entries {
        if let Some(first) = n.split('/').next() {
            if !first.is_empty() {
                dirs.insert(first.to_string());
            }
        }
    }
    if dirs.len() == 1 {
        dirs.into_iter().next()
    } else {
        None
    }
}

fn read_entry(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    idx: usize,
) -> Result<String, SkillHubError> {
    let mut entry = archive
        .by_index(idx)
        .map_err(|e| SkillHubError::Invalid(format!("zip 读取错误：{e}")))?;
    let mut text = String::new();
    if entry.size() > MAX_SKILL_MARKDOWN_BYTES as u64 {
        return Err(SkillHubError::Invalid(
            "SKILL.md exceeds the 512 KiB limit".into(),
        ));
    }
    entry
        .read_to_string(&mut text)
        .map_err(|e| SkillHubError::Invalid(format!("文件必须为 utf-8：{e}")))?;
    if text.len() > MAX_SKILL_MARKDOWN_BYTES {
        return Err(SkillHubError::Invalid(
            "SKILL.md exceeds the 512 KiB limit".into(),
        ));
    }
    Ok(text)
}

fn storage_err(e: fluxeme_contract::ContractError) -> SkillHubError {
    match e {
        fluxeme_contract::ContractError::NotFound(m) => SkillHubError::NotFound(m),
        fluxeme_contract::ContractError::Invalid(m) => SkillHubError::Invalid(m),
        fluxeme_contract::ContractError::Conflict(m) => SkillHubError::Conflict(m),
        fluxeme_contract::ContractError::Storage(m) => SkillHubError::Storage(m),
        fluxeme_contract::ContractError::Internal(m) => SkillHubError::Internal(m),
    }
}
