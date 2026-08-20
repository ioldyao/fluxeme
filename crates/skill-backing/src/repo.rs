//! Skill Runtime / Backing 仓储：直接读写 PostgreSQL（业务数据归属 PG）。
//! 运行时状态只落在 Runtime 自有的表上（endpoints / tasks / events）。

use sqlx_core::query::Query;
use sqlx_core::row::Row;
use sqlx_postgres::{PgPool, PgRow, Postgres};

use crate::domain::{EndpointRow, SkillRuntimeStatus, TaskRow};

#[derive(Debug)]
pub struct BackingError(pub String);

impl std::fmt::Display for BackingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for BackingError {}

impl From<sqlx_core::Error> for BackingError {
    fn from(e: sqlx_core::Error) -> Self {
        BackingError(e.to_string())
    }
}

fn map_endpoint_row(row: &PgRow) -> EndpointRow {
    EndpointRow {
        id: row.try_get::<String, _>(0).unwrap_or_default(),
        skill_id: row.try_get::<String, _>(1).unwrap_or_default(),
        skill_version_id: row.try_get::<String, _>(2).unwrap_or_default(),
        slug: row.try_get::<String, _>(3).unwrap_or_default(),
        version: row.try_get::<String, _>(4).unwrap_or_default(),
        endpoint_name: row.try_get::<String, _>(5).unwrap_or_default(),
        method: row.try_get::<String, _>(6).unwrap_or_default(),
        public_path: row.try_get::<String, _>(7).unwrap_or_default(),
        upstream_url: row.try_get::<String, _>(8).unwrap_or_default(),
        upstream_path: row.try_get::<Option<String>, _>(9).unwrap_or(None),
        timeout_ms: row.try_get::<i64, _>(10).unwrap_or(0),
        status: row.try_get::<String, _>(11).unwrap_or_default(),
        created_at: row.try_get::<String, _>(12).unwrap_or_default(),
        updated_at: row.try_get::<String, _>(13).unwrap_or_default(),
    }
}

fn map_task_row(row: &PgRow) -> TaskRow {
    TaskRow {
        id: row.try_get::<String, _>(0).unwrap_or_default(),
        skill_id: row.try_get::<String, _>(1).unwrap_or_default(),
        version_id: row.try_get::<String, _>(2).unwrap_or_default(),
        event_type: row.try_get::<String, _>(3).unwrap_or_default(),
        payload: row.try_get::<String, _>(4).unwrap_or_default(),
        status: row.try_get::<String, _>(5).unwrap_or_default(),
        attempts: row.try_get::<i32, _>(6).unwrap_or(0),
        last_error: row.try_get::<Option<String>, _>(7).unwrap_or(None),
        created_at: row.try_get::<String, _>(8).unwrap_or_default(),
        processed_at: row.try_get::<Option<String>, _>(9).unwrap_or(None),
    }
}

const ENDPOINT_COLUMNS: &str = "id, skill_id, skill_version_id, slug, version, endpoint_name, \
     method, public_path, upstream_url, upstream_path, timeout_ms, status, created_at, updated_at";

pub struct BackingRepository {
    pool: PgPool,
}

impl BackingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn exec<'q>(&self, sql: &'q str) -> Query<'q, Postgres, sqlx_postgres::PgArguments> {
        sqlx_core::query::query(sql)
    }

    // ── Migration（Runtime 自有表；outbox 任务表由 skillhub 兜底建过） ──

    pub async fn migrate(&self) -> Result<(), BackingError> {
        self.exec(
            "CREATE TABLE IF NOT EXISTS agent_skill_endpoints (
                id               TEXT PRIMARY KEY,
                skill_id         TEXT NOT NULL,
                skill_version_id TEXT NOT NULL,
                slug             TEXT NOT NULL,
                version          TEXT NOT NULL,
                endpoint_name    TEXT NOT NULL,
                method           TEXT NOT NULL,
                public_path      TEXT NOT NULL,
                upstream_url     TEXT NOT NULL,
                upstream_path    TEXT,
                timeout_ms       INT  NOT NULL DEFAULT 30000,
                status           TEXT NOT NULL DEFAULT 'pending',
                created_at       TEXT NOT NULL DEFAULT now(),
                updated_at       TEXT NOT NULL DEFAULT now(),
                UNIQUE (skill_id, skill_version_id, public_path, method)
            )",
        )
        .execute(&self.pool)
        .await?;
        self.exec(
            "CREATE INDEX IF NOT EXISTS idx_agent_skill_endpoints_skill \
             ON agent_skill_endpoints(skill_id, status)",
        )
        .execute(&self.pool)
        .await?;
        self.exec(
            "CREATE INDEX IF NOT EXISTS idx_agent_skill_endpoints_version \
             ON agent_skill_endpoints(skill_version_id)",
        )
        .execute(&self.pool)
        .await?;

        self.exec(
            "CREATE TABLE IF NOT EXISTS agent_skill_runtime_tasks (
                id           TEXT PRIMARY KEY,
                skill_id     TEXT NOT NULL,
                version_id   TEXT NOT NULL,
                event_type   TEXT NOT NULL,
                payload      TEXT NOT NULL DEFAULT '{}',
                status       TEXT NOT NULL DEFAULT 'pending',
                attempts     INT  NOT NULL DEFAULT 0,
                last_error   TEXT,
                created_at   TEXT NOT NULL DEFAULT now(),
                processed_at TEXT
            )",
        )
        .execute(&self.pool)
        .await?;
        self.exec(
            "CREATE INDEX IF NOT EXISTS idx_agent_skill_runtime_tasks_status \
             ON agent_skill_runtime_tasks(status, created_at)",
        )
        .execute(&self.pool)
        .await?;

        self.exec(
            "CREATE TABLE IF NOT EXISTS agent_skill_runtime_events (
                id         TEXT PRIMARY KEY,
                skill_id   TEXT NOT NULL,
                version_id TEXT,
                event_type TEXT NOT NULL,
                detail     TEXT,
                created_at TEXT NOT NULL DEFAULT now()
            )",
        )
        .execute(&self.pool)
        .await?;
        self.exec(
            "CREATE INDEX IF NOT EXISTS idx_agent_skill_runtime_events_skill \
             ON agent_skill_runtime_events(skill_id, created_at DESC)",
        )
        .execute(&self.pool)
        .await?;

        // 存量 UUID 库 → TEXT（幂等）：卸约束、改类型、重建。
        for sql in [
            "ALTER TABLE agent_skill_endpoints DROP CONSTRAINT IF EXISTS agent_skill_endpoints_skill_id_skill_version_id_public_path_method_key",
            "ALTER TABLE agent_skill_endpoints ALTER COLUMN id TYPE TEXT USING id::text",
            "ALTER TABLE agent_skill_endpoints ALTER COLUMN skill_id TYPE TEXT USING skill_id::text",
            "ALTER TABLE agent_skill_endpoints ALTER COLUMN skill_version_id TYPE TEXT USING skill_version_id::text",
            "ALTER TABLE agent_skill_endpoints ADD UNIQUE (skill_id, skill_version_id, public_path, method)",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN id TYPE TEXT USING id::text",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN skill_id TYPE TEXT USING skill_id::text",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN version_id TYPE TEXT USING version_id::text",
            "ALTER TABLE agent_skill_runtime_events ALTER COLUMN id TYPE TEXT USING id::text",
            "ALTER TABLE agent_skill_runtime_events ALTER COLUMN skill_id TYPE TEXT USING skill_id::text",
            "ALTER TABLE agent_skill_runtime_events ALTER COLUMN version_id TYPE TEXT USING version_id::text",
            "ALTER TABLE agent_skill_endpoints ALTER COLUMN created_at TYPE TEXT USING created_at::text",
            "ALTER TABLE agent_skill_endpoints ALTER COLUMN updated_at TYPE TEXT USING updated_at::text",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN created_at TYPE TEXT USING created_at::text",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN processed_at TYPE TEXT USING processed_at::text",
            "ALTER TABLE agent_skill_runtime_events ALTER COLUMN created_at TYPE TEXT USING created_at::text",
        ] {
            self.exec(sql).execute(&self.pool).await?;
        }

        tracing::info!("skill-backing tables ready");
        Ok(())
    }

    // ── Outbox 消费 ─────────────────────────────────────────────────────

    /// 认领 pending 任务：`FOR UPDATE SKIP LOCKED` 防多实例重复消费，
    /// 并原子标记为 processing、attempts+1。
    pub async fn claim_pending_tasks(&self, limit: i64) -> Result<Vec<TaskRow>, BackingError> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx_core::query::query(
            "SELECT id, skill_id, version_id, event_type, payload, status, attempts, \
                    last_error, created_at, processed_at \
             FROM agent_skill_runtime_tasks \
             WHERE status = 'pending' \
             ORDER BY created_at \
             LIMIT $1 FOR UPDATE SKIP LOCKED",
        )
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        let tasks: Vec<TaskRow> = rows.iter().map(map_task_row).collect();
        for t in &tasks {
            sqlx_core::query::query(
                "UPDATE agent_skill_runtime_tasks \
                 SET status='processing', attempts=attempts+1, processed_at=now()::text WHERE id=$1",
            )
            .bind(&t.id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(tasks)
    }

    /// 完成任务（done / failed + last_error）。幂等。
    pub async fn finish_task(
        &self,
        id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<(), BackingError> {
        self.exec(
            "UPDATE agent_skill_runtime_tasks SET status=$2, last_error=$3, processed_at=now()::text \
             WHERE id=$1",
        )
        .bind(id)
        .bind(status)
        .bind(last_error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ── Endpoints ───────────────────────────────────────────────────────

    /// 幂等重部署：清掉该版本的旧端点再插入（同版本重放不重复）。
    pub async fn replace_endpoints(&self, rows: &[EndpointRow]) -> Result<(), BackingError> {
        let mut tx = self.pool.begin().await?;
        if let Some(first) = rows.first() {
            sqlx_core::query::query(
                "DELETE FROM agent_skill_endpoints \
                 WHERE skill_id=$1 AND skill_version_id=$2",
            )
            .bind(&first.skill_id)
            .bind(&first.skill_version_id)
            .execute(&mut *tx)
            .await?;
        }
        for e in rows {
            sqlx_core::query::query(
                "INSERT INTO agent_skill_endpoints \
                 (id, skill_id, skill_version_id, slug, version, endpoint_name, method, \
                  public_path, upstream_url, upstream_path, timeout_ms, status, created_at, \
                  updated_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
            )
            .bind(&e.id)
            .bind(&e.skill_id)
            .bind(&e.skill_version_id)
            .bind(&e.slug)
            .bind(&e.version)
            .bind(&e.endpoint_name)
            .bind(&e.method)
            .bind(&e.public_path)
            .bind(&e.upstream_url)
            .bind(&e.upstream_path)
            .bind(e.timeout_ms)
            .bind(&e.status)
            .bind(&e.created_at)
            .bind(&e.updated_at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 取消发布：禁用该技能全部端点。
    pub async fn disable_endpoints(&self, skill_id: &str) -> Result<(), BackingError> {
        self.exec(
            "UPDATE agent_skill_endpoints SET status='disabled', updated_at=now()::text WHERE skill_id=$1",
        )
        .bind(skill_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 请求链 ④：按当前已部署版本的 method+public_path 找端点。
    pub async fn find_endpoint(
        &self,
        skill_id: &str,
        method: &str,
        public_path: &str,
    ) -> Result<Option<EndpointRow>, BackingError> {
        let row = self
            .exec(&format!(
                "SELECT {ENDPOINT_COLUMNS} FROM agent_skill_endpoints e \
                 WHERE e.skill_id=$1 AND e.method=$2 AND e.public_path=$3 AND e.status='ready' \
                 AND e.skill_version_id = (SELECT skill_version_id FROM agent_skill_endpoints e2 \
                     WHERE e2.skill_id = e.skill_id ORDER BY e2.created_at DESC LIMIT 1)"
            ))
            .bind(skill_id)
            .bind(method)
            .bind(public_path)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(map_endpoint_row))
    }

    pub async fn list_endpoints(&self, skill_id: &str) -> Result<Vec<EndpointRow>, BackingError> {
        let rows = self
            .exec(&format!(
                "SELECT {ENDPOINT_COLUMNS} FROM agent_skill_endpoints WHERE skill_id=$1 \
                 ORDER BY created_at DESC"
            ))
            .bind(skill_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(map_endpoint_row).collect())
    }

    // ── Runtime 状态聚合 ────────────────────────────────────────────────

    /// 每个技能取其最新已部署版本端点聚合运行状态。
    /// ready=全部 ready；failed=任一 failed；disabled=全部 disabled；否则 pending。
    pub async fn runtime_statuses(&self) -> Result<Vec<SkillRuntimeStatus>, BackingError> {
        let rows = self
            .exec(
                "SELECT e.skill_id, e.slug, e.version, \
                        bool_and(e.status='ready') AS all_ready, \
                        bool_or(e.status='failed') AS any_failed, \
                        bool_and(e.status='disabled') AS all_disabled, \
                        count(*) AS n \
                 FROM agent_skill_endpoints e \
                 WHERE e.skill_version_id = (SELECT skill_version_id FROM agent_skill_endpoints e2 \
                     WHERE e2.skill_id = e.skill_id ORDER BY e2.created_at DESC LIMIT 1) \
                 GROUP BY e.skill_id, e.slug, e.version",
            )
            .fetch_all(&self.pool)
            .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            let n: i64 = row.try_get::<i64, _>(6).unwrap_or(0);
            let state = if n == 0 {
                "pending"
            } else if row.try_get::<bool, _>(3).unwrap_or(false) {
                "ready"
            } else if row.try_get::<bool, _>(4).unwrap_or(false) {
                "failed"
            } else if row.try_get::<bool, _>(5).unwrap_or(false) {
                "disabled"
            } else {
                "pending"
            };
            out.push(SkillRuntimeStatus {
                skill_id: row.try_get::<String, _>(0).unwrap_or_default(),
                slug: row.try_get::<String, _>(1).unwrap_or_default(),
                version: row.try_get::<String, _>(2).unwrap_or_default(),
                state: state.to_string(),
            });
        }
        Ok(out)
    }

    // ── Runtime 事件（控制面） ──────────────────────────────────────────

    pub async fn insert_event(
        &self,
        skill_id: &str,
        version_id: Option<&str>,
        event_type: &str,
        detail: Option<&str>,
    ) -> Result<(), BackingError> {
        self.exec(
            "INSERT INTO agent_skill_runtime_events \
             (id, skill_id, version_id, event_type, detail, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(skill_id)
        .bind(version_id)
        .bind(event_type)
        .bind(detail)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
