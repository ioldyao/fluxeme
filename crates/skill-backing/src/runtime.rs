//! Skill Runtime 执行逻辑：outbox poller / 部署 / 运行时请求链。
//!
//! 请求链（数据面）：① Bearer API Key 鉴权 → ② resolve 发布态技能 →
//! ③ scope 校验（skill:{slug}:invoke）→ ④ 解析端点（绑当前版本）→
//! ⑤ HTTP 代理（SSRF 防护）→ ⑥ 计量（CH 观测 + PG 计费钩子）。

use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::Response;
use fluxeme_contract::{
    ApiKeyAuthorizer, RuntimeMeter, RuntimeUsageRecord, SkillId, SkillRuntimeCatalog, SkillSlug,
    SkillVersionId,
};
use reqwest::redirect::Policy;

use crate::domain::{EndpointRow, SkillRuntimeStatus, TaskRow};
use crate::manifest::parse_manifest;
use crate::policy::UpstreamPolicy;
use crate::repo::{BackingError, BackingRepository};

/// 请求路由参数：`/api/skills/{slug}/{*rest}`。
#[derive(Debug, serde::Deserialize)]
pub struct RuntimePath {
    pub slug: String,
    /// 剩余路径（含子路径；空 = 根）。
    pub rest: String,
}

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Skill Runtime 数据面入口。组合根创建并注入 AppState / 挂路由 / spawn poller。
pub struct SkillBackingModule {
    repo: BackingRepository,
    catalog: Arc<dyn SkillRuntimeCatalog>,
    authorizer: Arc<dyn ApiKeyAuthorizer>,
    meter: Arc<dyn RuntimeMeter>,
    client: reqwest::Client,
    policy: UpstreamPolicy,
}

impl SkillBackingModule {
    pub fn new(
        pool: sqlx_postgres::PgPool,
        catalog: Arc<dyn SkillRuntimeCatalog>,
        authorizer: Arc<dyn ApiKeyAuthorizer>,
        meter: Arc<dyn RuntimeMeter>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .build()
            .expect("build reqwest client");
        Self {
            repo: BackingRepository::new(pool),
            catalog,
            authorizer,
            meter,
            client,
            policy: UpstreamPolicy::default(),
        }
    }

    pub async fn migrate(&self) -> Result<(), BackingError> {
        self.repo.migrate().await
    }

    pub fn set_policy(&mut self, policy: UpstreamPolicy) {
        self.policy = policy;
    }

    /// 技能级运行状态汇总（UI 展示）。
    pub async fn runtime_statuses(&self) -> Result<Vec<SkillRuntimeStatus>, BackingError> {
        self.repo.runtime_statuses().await
    }

    // ── Outbox poller ───────────────────────────────────────────────────

    /// 后台循环：认领 pending 任务并处理。组合根 spawn。
    pub async fn run_poller(&self) {
        loop {
            if let Err(e) = self.poll_once().await {
                tracing::warn!("skill-backing poll error: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    async fn poll_once(&self) -> Result<(), BackingError> {
        let tasks = self.repo.claim_pending_tasks(10).await?;
        for t in tasks {
            self.process_task(&t).await;
        }
        Ok(())
    }

    async fn process_task(&self, task: &TaskRow) {
        let result = match task.event_type.as_str() {
            "skill_published" | "skill_version_deployed" => self.deploy(task).await,
            "skill_disabled" => {
                if let Err(e) = self.repo.disable_endpoints(&task.skill_id).await {
                    Err(e.to_string())
                } else if let Err(e) = self
                    .repo
                    .insert_event(
                        &task.skill_id,
                        Some(&task.version_id),
                        "endpoint_disabled",
                        None,
                    )
                    .await
                {
                    Err(e.to_string())
                } else {
                    Ok(())
                }
            }
            other => Err(format!("unknown event type {other}")),
        };
        match result {
            Ok(()) => {
                if let Err(e) = self.repo.finish_task(&task.id, "done", None).await {
                    tracing::warn!("finish task {}: {e}", task.id);
                }
            }
            Err(msg) => {
                tracing::warn!(
                    "task {} ({} {}) failed: {}",
                    task.id,
                    task.event_type,
                    task.skill_id,
                    msg
                );
                let _ = self.repo.finish_task(&task.id, "failed", Some(&msg)).await;
            }
        }
    }

    /// 部署：resolve → 解析 fluxeme.yaml → SSRF 校验 → 幂等注册端点。
    /// 结果：全部通过 = ready；任一被策略拦截 = failed（该端点登记 failed）。
    async fn deploy(&self, task: &TaskRow) -> Result<(), String> {
        let resolved = self
            .catalog
            .resolve_by_id(
                &SkillId(task.skill_id.clone()),
                &SkillVersionId(task.version_id.clone()),
            )
            .await
            .map_err(|e| e.to_string())?;
        let manifest = match &resolved.manifest_yaml {
            Some(y) => parse_manifest(y).map_err(|e| e.to_string())?,
            // 无 fluxeme.yaml = 纯指令技能，无运行时端点。
            None => fluxeme_contract::SkillManifest {
                name: SkillSlug(resolved.slug.0.clone()),
                version: resolved.version.clone(),
                endpoints: vec![],
            },
        };
        let now = chrono::Utc::now().to_rfc3339();
        let mut rows: Vec<EndpointRow> = Vec::new();
        let mut blocked: Option<String> = None;
        for ep in &manifest.endpoints {
            let status = match self.policy.validate(&ep.upstream, ep.timeout_ms).await {
                Ok(_) => "ready",
                Err(e) => {
                    if blocked.is_none() {
                        blocked = Some(format!("endpoint {}: {e}", ep.name));
                    }
                    "failed"
                }
            };
            rows.push(EndpointRow {
                id: uuid::Uuid::new_v4().to_string(),
                skill_id: task.skill_id.clone(),
                skill_version_id: task.version_id.clone(),
                slug: resolved.slug.0.clone(),
                version: resolved.version.clone(),
                endpoint_name: ep.name.clone(),
                method: ep.method.clone(),
                public_path: ep.path.clone(),
                upstream_url: ep.upstream.clone(),
                upstream_path: None,
                timeout_ms: ep.timeout_ms.unwrap_or(30_000) as i64,
                status: status.to_string(),
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }
        self.repo
            .replace_endpoints(&rows)
            .await
            .map_err(|e| e.to_string())?;
        self.repo
            .insert_event(
                &task.skill_id,
                Some(&task.version_id),
                if blocked.is_some() {
                    "endpoint_deploy_failed"
                } else {
                    "endpoint_deployed"
                },
                blocked.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())?;
        match blocked {
            Some(detail) => Err(detail),
            None => Ok(()),
        }
    }

    // ── 运行时请求链 ────────────────────────────────────────────────────
    // 组合根（根 crate）挂路由后调用本方法；Backing 不感知 AppState。

    /// 运行时请求入口：`/api/skills/{slug}/{*rest}`。
    pub async fn handle_runtime_request(
        &self,
        slug: &str,
        rest_path: &str,
        method: &Method,
        headers: &HeaderMap,
        body: axum::body::Bytes,
    ) -> Response {
        let start = Instant::now();

        // ① API Key 鉴权 + ③ scope 校验（skill:{slug}:invoke）
        let principal = match self.bearer(headers) {
            Some(bearer) => match self
                .authorizer
                .authorize(&bearer, "skill", slug, "invoke")
                .await
            {
                Ok(p) => p,
                Err(_) => {
                    return json_error(
                        StatusCode::UNAUTHORIZED,
                        "unauthorized: API key lacks skill:{slug}:invoke scope",
                        slug,
                    )
                }
            },
            None => {
                return json_error(
                    StatusCode::UNAUTHORIZED,
                    "missing Authorization: Bearer <api-key>",
                    slug,
                )
            }
        };

        // ② resolve 发布态技能（空版本 = 当前发布版本）
        let manifest = match self.catalog.resolve(&SkillSlug(slug.to_string()), "").await {
            Ok(m) => m,
            Err(_) => {
                return json_error(
                    StatusCode::NOT_FOUND,
                    "skill not found or not published",
                    slug,
                )
            }
        };

        // ④ 端点解析：public path = "/" + rest
        let path = if rest_path.is_empty() {
            "/".to_string()
        } else {
            format!("/{rest_path}")
        };
        let endpoint = match self
            .repo
            .find_endpoint(&manifest.skill.0, method.as_str(), &path)
            .await
        {
            Ok(Some(e)) => e,
            _ => return json_error(StatusCode::NOT_FOUND, "endpoint not found", slug),
        };

        if let Err(e) = self.policy.check_body(body.len()) {
            return json_error(StatusCode::PAYLOAD_TOO_LARGE, &e.to_string(), slug);
        }

        // ⑤ 代理（禁重定向防 SSRF 绕过；超时取端点声明）
        let timeout = std::time::Duration::from_millis(endpoint.timeout_ms.max(1) as u64);
        let mut req = self
            .client
            .request(method.clone(), &endpoint.upstream_url)
            .timeout(timeout)
            .body(body);
        for (name, value) in headers.iter() {
            let n = name.as_str().to_lowercase();
            if HOP_BY_HOP.contains(&n.as_str())
                || n == "authorization"
                || n == "host"
                || n == "content-length"
                || n == "content-type"
            {
                continue;
            }
            req = req.header(name, value);
        }

        let upstream = match req.send().await {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("upstream error: {e}");
                self.meter(start, &manifest, method, &path, 502, &principal)
                    .await;
                return json_error(StatusCode::BAD_GATEWAY, &msg, slug);
            }
        };

        let status = upstream.status();
        let upstream_headers = upstream.headers().clone();
        let body = match upstream.bytes().await {
            Ok(b) => b,
            Err(_) => {
                self.meter(start, &manifest, method, &path, 502, &principal)
                    .await;
                return json_error(StatusCode::BAD_GATEWAY, "upstream body read error", slug);
            }
        };

        self.meter(start, &manifest, method, &path, status.as_u16(), &principal)
            .await;

        let mut resp = Response::new(Body::from(body));
        *resp.status_mut() = status;
        for (name, value) in upstream_headers.iter() {
            let n = name.as_str().to_lowercase();
            if HOP_BY_HOP.contains(&n.as_str()) {
                continue;
            }
            if let Ok(v) = value.to_str() {
                if let Ok(hv) = HeaderValue::from_str(v) {
                    resp.headers_mut().insert(name, hv);
                }
            }
        }
        resp
    }

    /// ⑥ 计量（尽力而为，不阻塞请求路径）。
    async fn meter(
        &self,
        start: Instant,
        manifest: &fluxeme_contract::RuntimeSkillManifest,
        method: &Method,
        path: &str,
        status: u16,
        principal: &fluxeme_contract::RuntimePrincipal,
    ) {
        let record = RuntimeUsageRecord {
            skill: SkillId(manifest.skill.0.clone()),
            slug: SkillSlug(manifest.slug.0.clone()),
            version: manifest.version.clone(),
            method: method.to_string(),
            path: path.to_string(),
            status,
            latency_ms: start.elapsed().as_millis() as u64,
            bytes_in: 0,
            bytes_out: 0,
            user_id: principal.user_id.clone(),
            api_key_id: principal.api_key_id.clone(),
        };
        if let Err(e) = self.meter.record(record).await {
            tracing::warn!("skill-runtime meter: {e}");
        }
    }

    fn bearer(&self, headers: &HeaderMap) -> Option<String> {
        headers
            .get(header::AUTHORIZATION)?
            .to_str()
            .ok()
            .and_then(|v| {
                v.strip_prefix("Bearer ")
                    .or_else(|| v.strip_prefix("bearer "))
                    .map(|s| s.to_string())
            })
    }
}

fn json_error(status: StatusCode, message: &str, slug: &str) -> Response {
    let body = serde_json::json!({
        "error": { "message": message, "type": "skill_runtime_error" },
        "skill": slug,
    })
    .to_string();
    let mut resp = Response::new(Body::from(body));
    *resp.status_mut() = status;
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    resp
}
