//! 组合根层：Skill Runtime 两个 Port 的实现。
//!
//! `ApiKeyAuthorizer` / `RuntimeMeter` 定义在 contract；本模块提供实现，
//! skill-backing 只依赖 contract 接口，看不到这里的细节。

use std::sync::Arc;

use async_trait::async_trait;
use fluxeme_contract::{
    ApiKeyAuthorizer, ContractError, RuntimeMeter, RuntimePrincipal, RuntimeUsageRecord,
};

use crate::ch_backend::{ClickHouseBackend, SkillRuntimeCall};
use crate::db::Database;

/// 请求链 ③：现有 API Key 查找 + `api_key_scopes` 校验
/// （`skill:{slug}:invoke`）。未授权返回 NotFound 以隐藏资源存在性。
pub struct SkillKeyAuthorizer {
    db: Arc<Database>,
}

impl SkillKeyAuthorizer {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl ApiKeyAuthorizer for SkillKeyAuthorizer {
    async fn authorize(
        &self,
        bearer: &str,
        resource_type: &str,
        _resource_id: &str,
        action: &str,
    ) -> Result<RuntimePrincipal, ContractError> {
        let (user, key) = self
            .db
            .lookup_key(bearer)
            .await
            .map_err(|e| ContractError::Internal(e.to_string()))?
            .ok_or_else(|| ContractError::NotFound("invalid api key".into()))?;
        // 访问范围 = 资源类型级：key 勾选了该资源类型即放行（不按单技能授权）。
        let has = self
            .db
            .api_key_has_resource_scope(&key.key, resource_type, action)
            .await
            .map_err(|e| ContractError::Internal(e.to_string()))?;
        if !has {
            return Err(ContractError::NotFound("missing required scope".into()));
        }
        Ok(RuntimePrincipal {
            user_id: user.id,
            api_key_id: key.key,
        })
    }
}

/// 请求链 ⑥：计量 → ClickHouse `skill_runtime_calls`（高吞吐可观测）。
///
/// 计费钩子（PG `billing_events` + 钱包扣费）留待下一步接入 —— 钱包/账单等
/// 财务事实永远以 PostgreSQL 为准（不在此表内算钱）。
pub struct SkillRuntimeMeter {
    ch: Option<Arc<ClickHouseBackend>>,
}

impl SkillRuntimeMeter {
    pub fn new(ch: Option<Arc<ClickHouseBackend>>) -> Self {
        Self { ch }
    }
}

#[async_trait]
impl RuntimeMeter for SkillRuntimeMeter {
    async fn record(&self, record: RuntimeUsageRecord) -> Result<(), ContractError> {
        if let Some(ch) = &self.ch {
            let row = SkillRuntimeCall {
                timestamp: chrono::Utc::now().timestamp() as u32,
                request_id: uuid::Uuid::new_v4().to_string(),
                skill_id: record.skill.0,
                slug: record.slug.0,
                version: record.version,
                method: record.method,
                path: record.path,
                status_code: record.status,
                latency_ms: record.latency_ms,
                user_id: record.user_id,
                api_key_id: record.api_key_id,
            };
            ch.insert_skill_runtime_calls(&[row])
                .await
                .map_err(|e| ContractError::Internal(e))?;
        }
        Ok(())
    }
}
