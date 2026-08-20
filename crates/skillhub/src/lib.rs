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
    CreateSkill, PackageStatus, RuntimeTaskRow, SkillRow, SkillVersionRow, UpdateSkill,
};
pub use crate::error::SkillHubError;
use crate::repo::SkillRepository;

/// 单个技能包上限（50 MB）。
pub const MAX_ARTIFACT_BYTES: usize = 50 * 1024 * 1024;

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
            return Err(SkillHubError::Conflict(format!("slug '{slug}' already exists")));
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
    ) -> Result<SkillRow, SkillHubError> {
        let skill = self
            .repo
            .get_skill_by_id(id)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))?;
        if skill.status == status.as_str() {
            return Ok(skill);
        }
        // 控制面门禁：无包不可发布（运行时可用性由 Skill Runtime 决定）。
        if status == PackageStatus::Published && skill.artifact_path.is_none() {
            return Err(SkillHubError::Invalid(
                "cannot publish a skill without an uploaded artifact".into(),
            ));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let published_at = match status {
            PackageStatus::Published => Some(now.as_str()),
            _ => None,
        };
        // 当前版本 id（发布/取消发布都需定位 Runtime 侧的部署版本）。
        let current_version_id = self
            .repo
            .get_version(&skill.id, &skill.version)
            .await?
            .map(|v| v.id);
        let task = match status {
            PackageStatus::Published => current_version_id
                .as_ref()
                .map(|vid| self.build_task(&skill, vid, "skill_published")),
            _ if skill.status == PackageStatus::Published.as_str() => current_version_id
                .as_ref()
                .map(|vid| self.build_task(&skill, vid, "skill_disabled")),
            _ => None,
        };
        self.repo
            .set_status_with_task(id, status.as_str(), published_at, task.as_ref())
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
    /// 写版本行 + 更新技能当前版本 + outbox 任务（同事务）。
    pub async fn upload_artifact(
        &self,
        skill_id: &str,
        version: &str,
        changelog: Option<&str>,
        created_by: &str,
        bytes: Vec<u8>,
    ) -> Result<SkillVersionRow, SkillHubError> {
        validate_version(version)?;
        if bytes.is_empty() || bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(SkillHubError::Invalid(format!(
                "artifact size must be 1..={} bytes",
                MAX_ARTIFACT_BYTES
            )));
        }
        let skill = self
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
        let key = format!("{skill_id}/{version}.zip");
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
        // 已发布技能上新版本 → 同事务发 skill_version_deployed 任务，
        // 让 Runtime 重部署端点（发布一致性：任务与版本写原子提交）。
        let task = if skill.status == PackageStatus::Published.as_str() {
            Some(self.build_task(&skill, &vrow.id, "skill_version_deployed"))
        } else {
            None
        };
        self.repo
            .upload_artifact_with_task(&vrow, task.as_ref())
            .await?;
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

    /// 发布态目录（门禁：status=published）。阶段 1 不含 runtime_ready 判断。
    pub async fn list_published_skills(&self) -> Result<Vec<SkillRow>, SkillHubError> {
        self.repo.list_skills(Some("published"), None).await
    }

    /// 发布态详情（非 published 对用户端表现为不存在）。
    pub async fn get_published_skill(&self, slug: &str) -> Result<Option<SkillRow>, SkillHubError> {
        let skill = self.repo.get_skill_by_slug(slug).await?;
        Ok(skill.filter(|s| s.status == "published"))
    }

    /// 发布态技能的版本列表（用户端详情页用，仅已发布技能可见）。
    pub async fn list_published_versions(
        &self,
        slug: &str,
    ) -> Result<Vec<SkillVersionRow>, SkillHubError> {
        let skill = self
            .repo
            .get_skill_by_slug(slug)
            .await?
            .filter(|s| s.status == "published")
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))?;
        self.repo.list_versions(&skill.id).await
    }

    /// 下载技能包。门禁：`published AND 包已上传`。
    /// 阶段 1 以"包存在"代理 runtime_ready（阶段 2 由 Skill Runtime 接管）。
    pub async fn download(
        &self,
        slug: &str,
        version: Option<&str>,
    ) -> Result<DownloadPayload, SkillHubError> {
        let skill = self
            .get_published_skill(slug)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("skill".into()))?;
        let version = version.unwrap_or(&skill.version).to_string();
        let vrow = self
            .repo
            .get_version(&skill.id, &version)
            .await?
            .ok_or_else(|| SkillHubError::NotFound("version".into()))?;
        let path = vrow
            .artifact_path
            .ok_or_else(|| SkillHubError::NotFound("artifact".into()))?;
        let bytes = self.store.get(&path).await.map_err(storage_err)?;
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
            .filter(|s| s.status == "published")
            .ok_or_else(|| ContractError::NotFound(format!("skill {slug}")))?;
        let version = if version.is_empty() { &skill.version } else { version };
        let vrow = self
            .repo
            .get_version(&skill.id, version)
            .await
            .map_err(dberr_to_contract)?
            .ok_or_else(|| ContractError::NotFound(format!("version {version}")))?;
        Ok(RuntimeSkillManifest {
            skill: SkillId(skill.id.clone()),
            slug: slug.clone(),
            version: vrow.version,
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
            .ok_or_else(|| ContractError::NotFound("skill".into()))?;
        Ok(RuntimeSkillManifest {
            skill: skill.clone(),
            slug: SkillSlug(sk.slug),
            version: vrow.version,
            source_markdown: vrow.source_markdown,
            manifest_yaml: vrow.manifest_yaml,
            artifact_path: vrow.artifact_path,
        })
    }
}

fn dberr_to_contract(e: SkillHubError) -> ContractError {
    ContractError::Internal(e.to_string())
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

/// 版本：仅 `[0-9A-Za-z._-]`，禁路径分隔符与 `..`（防路径穿越），长度 ≤64。
fn validate_version(v: &str) -> Result<(), SkillHubError> {
    if v.is_empty() || v.len() > 64 {
        return Err(SkillHubError::Invalid("version must be 1..=64 chars".into()));
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
    let idx = find("fluxeme.yaml").or_else(|| {
        single_top_dir(&entries).and_then(|p| find(&format!("{p}/fluxeme.yaml")))
    });
    match idx {
        Some(i) => Ok(Some(read_entry(&mut archive, i)?)),
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
    let mut names = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let name = archive
            .by_index(i)
            .map_err(|e| SkillHubError::Invalid(format!("zip 读取错误：{e}")))?
            .name()
            .to_string();
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
    entry
        .read_to_string(&mut text)
        .map_err(|e| SkillHubError::Invalid(format!("文件必须为 utf-8：{e}")))?;
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
