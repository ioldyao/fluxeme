//! 技能包存储：`SkillArtifactStore` port 的本地磁盘实现。
//!
//! 存储位置：`{root}/{skill_id}/{version}.zip`。将来多实例时换
//! S3/MinIO 实现即可（只改组合根注入，域逻辑不动）。

use std::path::PathBuf;

use fluxeme_contract::{ContractError, SkillArtifactStore};

/// 本地磁盘实现。`root` 为技能包根目录（如 `data/skills`）。
pub struct LocalArtifactStore {
    root: PathBuf,
}

impl LocalArtifactStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// 防止路径穿越：key 为 `{skill_id}/{version}.zip`（两层嵌套），version
    /// 已在上传层校验（仅 `[0-9A-Za-z._-]`，禁 `/`、`\`、`..`）。这里兜底：
    /// 只要求最终路径落在 root 目录内（允许嵌套，仍阻断 `..`/绝对路径）。
    fn safe_path(&self, key: &str) -> Result<PathBuf, ContractError> {
        if key.is_empty() || key.contains("..") || key.starts_with('/') || key.contains('\\') {
            return Err(ContractError::Invalid(format!(
                "unsafe artifact key: {key}"
            )));
        }
        let path = self.root.join(key);
        if !path.starts_with(&self.root) {
            return Err(ContractError::Invalid(format!(
                "artifact key escapes root: {key}"
            )));
        }
        Ok(path)
    }
}

#[async_trait::async_trait]
impl SkillArtifactStore for LocalArtifactStore {
    async fn put(&self, key: &str, bytes: Vec<u8>) -> Result<(), ContractError> {
        let path = self.safe_path(key)?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ContractError::Storage(format!("mkdir {}: {e}", parent.display())))?;
        }
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|e| ContractError::Storage(format!("write {}: {e}", path.display())))
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, ContractError> {
        let path = self.safe_path(key)?;
        tokio::fs::read(&path).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ContractError::NotFound(format!("artifact {key}")),
            _ => ContractError::Storage(format!("read {}: {e}", path.display())),
        })
    }

    async fn delete(&self, key: &str) -> Result<(), ContractError> {
        let path = self.safe_path(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ContractError::Storage(format!(
                "delete {}: {e}",
                path.display()
            ))),
        }
    }
}
