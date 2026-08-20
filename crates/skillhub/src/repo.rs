//! SkillHub 仓储：直接读写 PostgreSQL（业务数据归属 PG，见项目 HARD RULE）。
//! 与根 crate 的 `pg_backend.rs` 风格一致：`sqlx_core` + 手动行映射。

use sqlx_core::query::Query;
use sqlx_core::query_builder::QueryBuilder;
use sqlx_core::row::Row;
use sqlx_postgres::{PgPool, PgRow, Postgres};

use crate::domain::{RuntimeTaskRow, SkillRow, SkillVersionRow};
use crate::error::SkillHubError;

fn map_skill_row(row: &PgRow) -> SkillRow {
    SkillRow {
        id: row.try_get::<String, _>(0).unwrap_or_default(),
        slug: row.try_get::<String, _>(1).unwrap_or_default(),
        name: row.try_get::<String, _>(2).unwrap_or_default(),
        description: row.try_get::<String, _>(3).unwrap_or_default(),
        category: row.try_get::<String, _>(4).unwrap_or_default(),
        tags: row.try_get::<Vec<String>, _>(5).unwrap_or_default(),
        author_id: row.try_get::<String, _>(6).unwrap_or_default(),
        version: row.try_get::<String, _>(7).unwrap_or_default(),
        artifact_path: row.try_get::<Option<String>, _>(8).unwrap_or(None),
        artifact_size: row.try_get::<i64, _>(9).unwrap_or(0),
        source_markdown: row.try_get::<Option<String>, _>(10).unwrap_or(None),
        visibility: row.try_get::<String, _>(11).unwrap_or_default(),
        status: row.try_get::<String, _>(12).unwrap_or_default(),
        published_at: row.try_get::<Option<String>, _>(13).unwrap_or(None),
        created_at: row.try_get::<String, _>(14).unwrap_or_default(),
        updated_at: row.try_get::<String, _>(15).unwrap_or_default(),
    }
}

fn map_version_row(row: &PgRow) -> SkillVersionRow {
    SkillVersionRow {
        id: row.try_get::<String, _>(0).unwrap_or_default(),
        skill_id: row.try_get::<String, _>(1).unwrap_or_default(),
        version: row.try_get::<String, _>(2).unwrap_or_default(),
        changelog: row.try_get::<Option<String>, _>(3).unwrap_or(None),
        artifact_path: row.try_get::<Option<String>, _>(4).unwrap_or(None),
        artifact_size: row.try_get::<i64, _>(5).unwrap_or(0),
        source_markdown: row.try_get::<Option<String>, _>(6).unwrap_or(None),
        manifest_yaml: row.try_get::<Option<String>, _>(7).unwrap_or(None),
        status: row.try_get::<String, _>(8).unwrap_or_default(),
        created_by: row.try_get::<String, _>(9).unwrap_or_default(),
        created_at: row.try_get::<String, _>(10).unwrap_or_default(),
    }
}

const SKILL_COLUMNS: &str =
    "id, slug, name, description, category, tags, author_id, version, \
     artifact_path, artifact_size, source_markdown, visibility, status, \
     published_at, created_at, updated_at";

pub struct SkillRepository {
    pool: PgPool,
}

impl SkillRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    fn exec<'q>(&self, sql: &'q str) -> Query<'q, Postgres, sqlx_postgres::PgArguments> {
        sqlx_core::query::query(sql)
    }

    // ── Migration ─────────────────────────────────────────────────────
    /// SkillHub 拥有自己的 schema 迁移（自洽子系统：表归属 = 建表方）。
    pub async fn migrate(&self) -> Result<(), SkillHubError> {
        self.exec(
            "CREATE TABLE IF NOT EXISTS agent_skills (
                id              TEXT PRIMARY KEY,
                slug            TEXT NOT NULL UNIQUE,
                name            TEXT NOT NULL,
                description     TEXT NOT NULL DEFAULT '',
                category        TEXT NOT NULL DEFAULT 'general',
                tags            TEXT[] NOT NULL DEFAULT '{}',
                author_id       TEXT NOT NULL,
                version         TEXT NOT NULL DEFAULT '0.0.0',
                artifact_path   TEXT,
                artifact_size   BIGINT NOT NULL DEFAULT 0,
                source_markdown TEXT,
                visibility      TEXT NOT NULL DEFAULT 'internal',
                status          TEXT NOT NULL DEFAULT 'draft',
                published_at    TEXT,
                created_at      TEXT NOT NULL DEFAULT now(),
                updated_at      TEXT NOT NULL DEFAULT now()
            )",
        )
        .execute(&self.pool)
        .await?;

        self.exec(
            "CREATE INDEX IF NOT EXISTS idx_agent_skills_slug ON agent_skills(slug)",
        )
        .execute(&self.pool)
        .await?;
        self.exec(
            "CREATE INDEX IF NOT EXISTS idx_agent_skills_status ON agent_skills(status, visibility)",
        )
        .execute(&self.pool)
        .await?;
        self.exec(
            "CREATE INDEX IF NOT EXISTS idx_agent_skills_published ON agent_skills(published_at DESC)",
        )
        .execute(&self.pool)
        .await?;

        self.exec(
            "CREATE TABLE IF NOT EXISTS agent_skill_versions (
                id              TEXT PRIMARY KEY,
                skill_id        TEXT NOT NULL REFERENCES agent_skills(id) ON DELETE CASCADE,
                version         TEXT NOT NULL,
                changelog       TEXT,
                artifact_path   TEXT,
                artifact_size   BIGINT NOT NULL DEFAULT 0,
                source_markdown TEXT,
                manifest_yaml   TEXT,
                status          TEXT NOT NULL DEFAULT 'draft',
                created_by      TEXT NOT NULL,
                created_at      TEXT NOT NULL DEFAULT now(),
                UNIQUE (skill_id, version)
            )",
        )
        .execute(&self.pool)
        .await?;
        // 存量库升级：CREATE IF NOT EXISTS 不会给已有表加列，单独补。
        self.exec(
            "ALTER TABLE agent_skill_versions ADD COLUMN IF NOT EXISTS manifest_yaml TEXT",
        )
        .execute(&self.pool)
        .await?;
        self.exec(
            "CREATE INDEX IF NOT EXISTS idx_agent_skill_versions_skill \
             ON agent_skill_versions(skill_id, version)",
        )
        .execute(&self.pool)
        .await?;

        // outbox 传输表（agent_skill_runtime_tasks）：非域数据，SkillHub 发布侧
        // 与 Skill Runtime 消费侧都读写，此处由 SkillHub 也兜底建一次。
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
            "CREATE TABLE IF NOT EXISTS agent_skill_installs (
                id           TEXT PRIMARY KEY,
                skill_id     TEXT NOT NULL REFERENCES agent_skills(id) ON DELETE CASCADE,
                user_id      TEXT NOT NULL,
                version      TEXT NOT NULL,
                source       TEXT NOT NULL DEFAULT 'user',
                installed_at TEXT NOT NULL DEFAULT now(),
                UNIQUE (skill_id, user_id)
            )",
        )
        .execute(&self.pool)
        .await?;
        self.exec(
            "CREATE INDEX IF NOT EXISTS idx_agent_skill_installs_user \
             ON agent_skill_installs(user_id)",
        )
        .execute(&self.pool)
        .await?;

        // 存量 UUID 库 → TEXT（幂等）：先卸 FK/UNIQUE，改类型，再重建约束。
        // 对齐项目约定（users.id / api_keys.key 均为 TEXT），避免 uuid = text。
        for sql in [
            "ALTER TABLE agent_skill_versions DROP CONSTRAINT IF EXISTS agent_skill_versions_skill_id_fkey",
            "ALTER TABLE agent_skill_versions DROP CONSTRAINT IF EXISTS agent_skill_versions_skill_id_version_key",
            "ALTER TABLE agent_skill_installs DROP CONSTRAINT IF EXISTS agent_skill_installs_skill_id_fkey",
            "ALTER TABLE agent_skill_installs DROP CONSTRAINT IF EXISTS agent_skill_installs_skill_id_user_id_key",
            "ALTER TABLE agent_skills ALTER COLUMN id TYPE TEXT USING id::text",
            "ALTER TABLE agent_skills ALTER COLUMN author_id TYPE TEXT USING author_id::text",
            "ALTER TABLE agent_skill_versions ALTER COLUMN id TYPE TEXT USING id::text",
            "ALTER TABLE agent_skill_versions ALTER COLUMN skill_id TYPE TEXT USING skill_id::text",
            "ALTER TABLE agent_skill_versions ALTER COLUMN created_by TYPE TEXT USING created_by::text",
            "ALTER TABLE agent_skill_installs ALTER COLUMN id TYPE TEXT USING id::text",
            "ALTER TABLE agent_skill_installs ALTER COLUMN skill_id TYPE TEXT USING skill_id::text",
            "ALTER TABLE agent_skill_installs ALTER COLUMN user_id TYPE TEXT USING user_id::text",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN id TYPE TEXT USING id::text",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN skill_id TYPE TEXT USING skill_id::text",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN version_id TYPE TEXT USING version_id::text",
            "ALTER TABLE agent_skill_versions ADD CONSTRAINT agent_skill_versions_skill_id_fkey FOREIGN KEY (skill_id) REFERENCES agent_skills(id) ON DELETE CASCADE",
            "ALTER TABLE agent_skill_versions ADD UNIQUE (skill_id, version)",
            "ALTER TABLE agent_skill_installs ADD CONSTRAINT agent_skill_installs_skill_id_fkey FOREIGN KEY (skill_id) REFERENCES agent_skills(id) ON DELETE CASCADE",
            "ALTER TABLE agent_skill_installs ADD UNIQUE (skill_id, user_id)",
            "ALTER TABLE agent_skills ALTER COLUMN published_at TYPE TEXT USING published_at::text",
            "ALTER TABLE agent_skills ALTER COLUMN created_at TYPE TEXT USING created_at::text",
            "ALTER TABLE agent_skills ALTER COLUMN updated_at TYPE TEXT USING updated_at::text",
            "ALTER TABLE agent_skill_versions ALTER COLUMN created_at TYPE TEXT USING created_at::text",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN created_at TYPE TEXT USING created_at::text",
            "ALTER TABLE agent_skill_runtime_tasks ALTER COLUMN processed_at TYPE TEXT USING processed_at::text",
            "ALTER TABLE agent_skill_installs ALTER COLUMN installed_at TYPE TEXT USING installed_at::text",
        ] {
            self.exec(sql).execute(&self.pool).await?;
        }

        tracing::info!("skillhub tables ready");
        Ok(())
    }

    // ── Skills ────────────────────────────────────────────────────────

    pub async fn insert_skill(&self, s: &SkillRow) -> Result<(), SkillHubError> {
        self.exec(
            "INSERT INTO agent_skills \
             (id, slug, name, description, category, tags, author_id, version, \
              artifact_path, artifact_size, source_markdown, visibility, status, \
              published_at, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        )
        .bind(&s.id)
        .bind(&s.slug)
        .bind(&s.name)
        .bind(&s.description)
        .bind(&s.category)
        .bind(&s.tags)
        .bind(&s.author_id)
        .bind(&s.version)
        .bind(&s.artifact_path)
        .bind(s.artifact_size)
        .bind(&s.source_markdown)
        .bind(&s.visibility)
        .bind(&s.status)
        .bind(&s.published_at)
        .bind(&s.created_at)
        .bind(&s.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_skill(&self, s: &SkillRow) -> Result<(), SkillHubError> {
        self.exec(
            "UPDATE agent_skills SET name=$2, description=$3, category=$4, tags=$5, \
             visibility=$6, updated_at=$7 WHERE id=$1",
        )
        .bind(&s.id)
        .bind(&s.name)
        .bind(&s.description)
        .bind(&s.category)
        .bind(&s.tags)
        .bind(&s.visibility)
        .bind(&s.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_skills(
        &self,
        status: Option<&str>,
        visibility: Option<&str>,
    ) -> Result<Vec<SkillRow>, SkillHubError> {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new(format!("SELECT {SKILL_COLUMNS} FROM agent_skills WHERE 1=1"));
        if let Some(status) = status {
            qb.push(" AND status = ").push_bind(status);
        }
        if let Some(visibility) = visibility {
            qb.push(" AND visibility = ").push_bind(visibility);
        }
        qb.push(" ORDER BY updated_at DESC");
        let rows = qb.build().fetch_all(&self.pool).await?;
        Ok(rows.iter().map(map_skill_row).collect())
    }

    pub async fn get_skill_by_id(&self, id: &str) -> Result<Option<SkillRow>, SkillHubError> {
        let row = self
            .exec(&format!("SELECT {SKILL_COLUMNS} FROM agent_skills WHERE id = $1"))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(map_skill_row))
    }

    pub async fn get_skill_by_slug(&self, slug: &str) -> Result<Option<SkillRow>, SkillHubError> {
        let row = self
            .exec(&format!("SELECT {SKILL_COLUMNS} FROM agent_skills WHERE slug = $1"))
            .bind(slug)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(map_skill_row))
    }

    pub async fn delete_skill(&self, id: &str) -> Result<(), SkillHubError> {
        self.exec("DELETE FROM agent_skills WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 状态变更 + outbox 任务同事务提交。发布可靠性：PG 已 COMMIT 即任务已
    /// 持久化，进程崩溃后 poller 可重放，不会丢。
    pub async fn set_status_with_task(
        &self,
        id: &str,
        status: &str,
        published_at: Option<&str>,
        task: Option<&RuntimeTaskRow>,
    ) -> Result<(), SkillHubError> {
        let mut tx = self.pool.begin().await?;
        sqlx_core::query::query(
            "UPDATE agent_skills SET status=$2, published_at=$3, updated_at=now()::text WHERE id=$1",
        )
        .bind(id)
        .bind(status)
        .bind(published_at)
        .execute(&mut *tx)
        .await?;
        if let Some(t) = task {
            Self::insert_task_tx(&mut tx, t).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 按版本 id 取版本行（SkillRuntimeCatalog::resolve_by_id 用）。
    pub async fn get_version_by_id(
        &self,
        version_id: &str,
    ) -> Result<Option<SkillVersionRow>, SkillHubError> {
        let row = self
            .exec(
                "SELECT id, skill_id, version, changelog, artifact_path, artifact_size, \
                 source_markdown, manifest_yaml, status, created_by, created_at \
                 FROM agent_skill_versions WHERE id=$1",
            )
            .bind(version_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(map_version_row))
    }

    // ── Outbox（传输/基础设施） ──────────────────────────────────────────

    async fn insert_task_tx<'c>(
        tx: &mut sqlx_core::transaction::Transaction<'c, Postgres>,
        t: &RuntimeTaskRow,
    ) -> Result<(), SkillHubError> {
        sqlx_core::query::query(
            "INSERT INTO agent_skill_runtime_tasks \
             (id, skill_id, version_id, event_type, payload, status, attempts, \
              last_error, created_at, processed_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(&t.id)
        .bind(&t.skill_id)
        .bind(&t.version_id)
        .bind(&t.event_type)
        .bind(&t.payload)
        .bind(&t.status)
        .bind(t.attempts)
        .bind(&t.last_error)
        .bind(&t.created_at)
        .bind(&t.processed_at)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    // ── Versions ──────────────────────────────────────────────────────

    /// 上传：版本行 + 技能当前包 + outbox 任务同事务提交。
    pub async fn upload_artifact_with_task(
        &self,
        v: &SkillVersionRow,
        task: Option<&RuntimeTaskRow>,
    ) -> Result<(), SkillHubError> {
        let mut tx = self.pool.begin().await?;
        sqlx_core::query::query(
            "INSERT INTO agent_skill_versions \
             (id, skill_id, version, changelog, artifact_path, artifact_size, \
              source_markdown, manifest_yaml, status, created_by, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(&v.id)
        .bind(&v.skill_id)
        .bind(&v.version)
        .bind(&v.changelog)
        .bind(&v.artifact_path)
        .bind(v.artifact_size)
        .bind(&v.source_markdown)
        .bind(&v.manifest_yaml)
        .bind(&v.status)
        .bind(&v.created_by)
        .bind(&v.created_at)
        .execute(&mut *tx)
        .await?;
        sqlx_core::query::query(
            "UPDATE agent_skills SET version=$2, artifact_path=$3, artifact_size=$4, \
             source_markdown=$5, updated_at=now()::text WHERE id=$1",
        )
        .bind(&v.skill_id)
        .bind(&v.version)
        .bind(&v.artifact_path)
        .bind(v.artifact_size)
        .bind(&v.source_markdown)
        .execute(&mut *tx)
        .await?;
        if let Some(t) = task {
            Self::insert_task_tx(&mut tx, t).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_version(
        &self,
        skill_id: &str,
        version: &str,
    ) -> Result<Option<SkillVersionRow>, SkillHubError> {
        let row = self
            .exec(
                "SELECT id, skill_id, version, changelog, artifact_path, artifact_size, \
                 source_markdown, manifest_yaml, status, created_by, created_at \
                 FROM agent_skill_versions WHERE skill_id=$1 AND version=$2",
            )
            .bind(skill_id)
            .bind(version)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.as_ref().map(map_version_row))
    }

    pub async fn list_versions(
        &self,
        skill_id: &str,
    ) -> Result<Vec<SkillVersionRow>, SkillHubError> {
        let rows = self
            .exec(
                "SELECT id, skill_id, version, changelog, artifact_path, artifact_size, \
                 source_markdown, manifest_yaml, status, created_by, created_at \
                 FROM agent_skill_versions WHERE skill_id=$1 ORDER BY created_at DESC",
            )
            .bind(skill_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(map_version_row).collect())
    }

}
