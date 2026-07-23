use async_trait::async_trait;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, QueryBuilder, Row};

use crate::config::types::GatewayRuntimeConfig;
use crate::db::backend::DbBackend;
use crate::db::{DbError, ProbeResultRow, RechargeKeyRow, WalletTransactionRow};
use crate::domain::channel::{Channel, Endpoint};
use crate::domain::model::{Model, ModelChannel, Pricing};
use crate::domain::moderation::ContentFilterRule;
use crate::domain::routing::RoutingRule;
use crate::domain::usage::{UsageFilter, UsageRecord};
use crate::domain::user::{ApiKey, User};

pub struct PgBackend {
    pool: PgPool,
}

impl PgBackend {
    pub async fn new(pg_url: &str) -> Result<Self, DbError> {
        let pool = PgPool::connect(pg_url)
            .await
            .map_err(|e| DbError(format!("Failed to connect to PostgreSQL: {}", e)))?;
        Ok(Self { pool })
    }

    // ── Private helpers ──────────────────────────────────────────────────────────

    #[allow(dead_code)]
    async fn pricing_lookup(&self, model_name: &str) -> (f64, f64) {
        let result = sqlx::query_as::<_, (f64, f64)>(
            "SELECT prompt_price, completion_price FROM models WHERE name = $1",
        )
        .bind(model_name)
        .fetch_optional(&self.pool)
        .await;

        match result {
            Ok(Some(p)) => p,
            _ => {
                // Fall back to pattern matching
                let rows = sqlx::query_as::<_, (f64, f64, String)>(
                    "SELECT prompt_price, completion_price, model_pattern FROM models",
                )
                .fetch_all(&self.pool)
                .await;

                if let Ok(rows) = rows {
                    for (p, c, pattern) in rows {
                        if pattern.ends_with('*') {
                            let prefix = &pattern[..pattern.len() - 1];
                            if model_name.starts_with(prefix) {
                                return (p, c);
                            }
                        }
                        if pattern == model_name {
                            return (p, c);
                        }
                    }
                }
                (0.0, 0.0)
            }
        }
    }

    /// Build helper for tz-aware day expression.
    fn day_expr(tz_offset_seconds: i64) -> String {
        if tz_offset_seconds >= 0 {
            format!(
                "LEFT((timestamp::timestamp + INTERVAL '{} seconds')::text, 10) AS day",
                tz_offset_seconds
            )
        } else {
            format!(
                "LEFT((timestamp::timestamp - INTERVAL '{} seconds')::text, 10) AS day",
                -tz_offset_seconds
            )
        }
    }

    fn map_user_row(row: &PgRow, idx: &mut usize) -> User {
        let id: String = row.get(*idx); *idx += 1;
        let name: String = row.get(*idx); *idx += 1;
        let rpm: Option<i64> = row.get(*idx); *idx += 1;
        let tpm: Option<i64> = row.get(*idx); *idx += 1;
        let timezone: Option<String> = row.get(*idx); *idx += 1;
        let token_version: i64 = row.get(*idx); *idx += 1;
        let role_val: Option<String> = row.get(*idx); *idx += 1;
        let concurrency_val: i64 = row.get(*idx); *idx += 1;
        let currency: String = row.get(*idx); *idx += 1;
        User {
            id,
            name,
            password_hash: None,
            rate_limits: {
                let rpm = rpm.map(|v| v as u64);
                let tpm = tpm.map(|v| v as u64);
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: timezone.unwrap_or_default(),
            token_version,
            role: role_val.unwrap_or_default(),
            concurrency_limit: concurrency_val as u32,
            currency,
        }
    }

    fn map_user_with_pw_row(row: &PgRow, idx: &mut usize) -> User {
        let id: String = row.get(*idx); *idx += 1;
        let name: String = row.get(*idx); *idx += 1;
        let password_hash: String = row.get(*idx); *idx += 1;
        let rpm: Option<i64> = row.get(*idx); *idx += 1;
        let tpm: Option<i64> = row.get(*idx); *idx += 1;
        let timezone: Option<String> = row.get(*idx); *idx += 1;
        let token_version: i64 = row.get(*idx); *idx += 1;
        let role_val: Option<String> = row.get(*idx); *idx += 1;
        let concurrency_val: i64 = row.get(*idx); *idx += 1;
        let currency: String = row.get(*idx); *idx += 1;
        User {
            id,
            name,
            password_hash: Some(password_hash),
            rate_limits: {
                let rpm = rpm.map(|v| v as u64);
                let tpm = tpm.map(|v| v as u64);
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: timezone.unwrap_or_default(),
            token_version,
            role: role_val.unwrap_or_default(),
            concurrency_limit: concurrency_val as u32,
            currency,
        }
    }

    fn map_usage_record(row: &PgRow, idx: &mut usize) -> UsageRecord {
        let timestamp: String = row.get(*idx); *idx += 1;
        let request_id: String = row.get(*idx); *idx += 1;
        let user_id: String = row.get(*idx); *idx += 1;
        let user_name: String = row.get(*idx); *idx += 1;
        let channel_id: String = row.get(*idx); *idx += 1;
        let model: String = row.get(*idx); *idx += 1;
        let prompt_tokens: i64 = row.get(*idx); *idx += 1;
        let completion_tokens: i64 = row.get(*idx); *idx += 1;
        let total_tokens: i64 = row.get(*idx); *idx += 1;
        let latency_ms: i64 = row.get(*idx); *idx += 1;
        let status_code: i32 = row.get(*idx); *idx += 1;
        let success: bool = row.get(*idx); *idx += 1;
        let api_key_name: Option<String> = row.get(*idx); *idx += 1;
        let api_format: String = row.get(*idx); *idx += 1;
        let stream: bool = row.get(*idx); *idx += 1;
        let cache_hit_input_tokens: i64 = row.get(*idx); *idx += 1;
        let prompt_price: f64 = row.get(*idx); *idx += 1;
        let completion_price: f64 = row.get(*idx); *idx += 1;
        let cache_read_price: f64 = row.get(*idx); *idx += 1;
        let client_ip: Option<String> = row.get(*idx); *idx += 1;
        UsageRecord {
            timestamp,
            request_id,
            user_id,
            user_name,
            channel_id,
            model,
            prompt_tokens: prompt_tokens as u64,
            completion_tokens: completion_tokens as u64,
            total_tokens: total_tokens as u64,
            latency_ms: latency_ms as u64,
            status_code: status_code as u16,
            success,
            request_body: None,
            response_body: None,
            reasoning_body: None,
            api_key_name,
            api_format,
            stream,
            cache_hit_input_tokens: cache_hit_input_tokens as u64,
            prompt_price,
            completion_price,
            cache_read_price,
            client_ip,
        }
    }

    fn map_usage_with_bodies(row: &PgRow, idx: &mut usize) -> UsageRecord {
        let timestamp: String = row.get(*idx); *idx += 1;
        let request_id: String = row.get(*idx); *idx += 1;
        let user_id: String = row.get(*idx); *idx += 1;
        let user_name: String = row.get(*idx); *idx += 1;
        let channel_id: String = row.get(*idx); *idx += 1;
        let model: String = row.get(*idx); *idx += 1;
        let prompt_tokens: i64 = row.get(*idx); *idx += 1;
        let completion_tokens: i64 = row.get(*idx); *idx += 1;
        let total_tokens: i64 = row.get(*idx); *idx += 1;
        let latency_ms: i64 = row.get(*idx); *idx += 1;
        let status_code: i32 = row.get(*idx); *idx += 1;
        let success: bool = row.get(*idx); *idx += 1;
        let request_body: Option<String> = row.get(*idx); *idx += 1;
        let response_body: Option<String> = row.get(*idx); *idx += 1;
        let reasoning_body: Option<String> = row.get(*idx); *idx += 1;
        let api_key_name: Option<String> = row.get(*idx); *idx += 1;
        let api_format: String = row.get(*idx); *idx += 1;
        let stream: bool = row.get(*idx); *idx += 1;
        let cache_hit_input_tokens: i64 = row.get(*idx); *idx += 1;
        let prompt_price: f64 = row.get(*idx); *idx += 1;
        let completion_price: f64 = row.get(*idx); *idx += 1;
        let cache_read_price: f64 = row.get(*idx); *idx += 1;
        let client_ip: Option<String> = row.get(*idx); *idx += 1;
        UsageRecord {
            timestamp,
            request_id,
            user_id,
            user_name,
            channel_id,
            model,
            prompt_tokens: prompt_tokens as u64,
            completion_tokens: completion_tokens as u64,
            total_tokens: total_tokens as u64,
            latency_ms: latency_ms as u64,
            status_code: status_code as u16,
            success,
            request_body,
            response_body,
            reasoning_body,
            api_key_name,
            api_format,
            stream,
            cache_hit_input_tokens: cache_hit_input_tokens as u64,
            prompt_price,
            completion_price,
            cache_read_price,
            client_ip,
        }
    }
}

#[async_trait]
impl DbBackend for PgBackend {
    // ── Migration ────────────────────────────────────────────────────────

    async fn migrate(&self) -> Result<(), DbError> {
        sqlx::raw_sql(
            "
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                password_hash TEXT NOT NULL DEFAULT '',
                rpm BIGINT,
                tpm BIGINT,
                concurrency_limit BIGINT NOT NULL DEFAULT 2000,
                currency TEXT NOT NULL DEFAULT 'usd'
            );

            CREATE TABLE IF NOT EXISTS api_keys (
                key TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                name TEXT DEFAULT '',
                enabled BOOLEAN NOT NULL DEFAULT true,
                expires_at TEXT
            );

            CREATE TABLE IF NOT EXISTS channels (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL DEFAULT '',
                provider TEXT NOT NULL,
                priority INTEGER NOT NULL DEFAULT 1,
                enabled BOOLEAN NOT NULL DEFAULT true,
                anthropic_compat BOOLEAN NOT NULL DEFAULT false
            );

            CREATE TABLE IF NOT EXISTS endpoints (
                id BIGSERIAL PRIMARY KEY,
                channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
                url TEXT NOT NULL,
                api_key TEXT DEFAULT '',
                weight INTEGER NOT NULL DEFAULT 1,
                timeout_secs BIGINT
            );

            CREATE TABLE IF NOT EXISTS models (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                model_pattern TEXT NOT NULL,
                prompt_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                completion_price DOUBLE PRECISION NOT NULL DEFAULT 0.0
            );

            CREATE TABLE IF NOT EXISTS model_channels (
                model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
                channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
                priority INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (model_id, channel_id)
            );

            CREATE TABLE IF NOT EXISTS routing_rules (
                name TEXT PRIMARY KEY,
                user_id TEXT NOT NULL DEFAULT '*',
                model_pattern TEXT NOT NULL,
                channel_id TEXT NOT NULL REFERENCES channels(id)
            );

            CREATE TABLE IF NOT EXISTS usage_logs (
                id BIGSERIAL PRIMARY KEY,
                timestamp TEXT NOT NULL,
                request_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                user_name TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_tokens BIGINT NOT NULL,
                completion_tokens BIGINT NOT NULL,
                total_tokens BIGINT NOT NULL,
                latency_ms BIGINT NOT NULL,
                status_code INTEGER NOT NULL,
                success BOOLEAN NOT NULL,
                request_body TEXT,
                response_body TEXT,
                reasoning_body TEXT,
                api_key_name TEXT,
                api_format TEXT NOT NULL DEFAULT '',
                stream BOOLEAN NOT NULL DEFAULT false,
                cache_hit_input_tokens BIGINT NOT NULL DEFAULT 0,
                prompt_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                completion_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                client_ip TEXT
            );

            CREATE TABLE IF NOT EXISTS user_subscriptions (
                user_id TEXT NOT NULL,
                model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL,
                PRIMARY KEY (user_id, model_id)
            );

            CREATE TABLE IF NOT EXISTS wallet_transactions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                type TEXT NOT NULL,
                amount DOUBLE PRECISION NOT NULL,
                balance_before DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                balance_after DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                method TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'completed',
                note TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS recharge_keys (
                key TEXT PRIMARY KEY,
                amount DOUBLE PRECISION NOT NULL,
                used_by TEXT,
                used_at TEXT,
                created_by TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS balancer_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration error: {}", e)))?;

        // Backward-compat columns — inline helper to avoid async closure issues
        macro_rules! add_col {
            ($sql:expr) => {
                let _ = sqlx::raw_sql($sql)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| DbError(format!("Migration alter error: {}", e)));
            };
        }

        add_col!("ALTER TABLE models ADD COLUMN IF NOT EXISTS published BOOLEAN NOT NULL DEFAULT false");
        add_col!("ALTER TABLE models ADD COLUMN IF NOT EXISTS context_length BIGINT");
        add_col!("ALTER TABLE models ADD COLUMN IF NOT EXISTS cache_read_price DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!("ALTER TABLE models ADD COLUMN IF NOT EXISTS cache_write_price DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!("ALTER TABLE models ADD COLUMN IF NOT EXISTS image_input_price DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!("ALTER TABLE models ADD COLUMN IF NOT EXISTS audio_input_price DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!("ALTER TABLE models ADD COLUMN IF NOT EXISTS audio_output_price DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!("ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS spend_limit DOUBLE PRECISION");
        add_col!("ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS allowed_models TEXT");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS concurrency_limit BIGINT NOT NULL DEFAULT 2000");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS currency TEXT NOT NULL DEFAULT 'usd'");
        add_col!("ALTER TABLE endpoints ADD COLUMN IF NOT EXISTS enabled BOOLEAN NOT NULL DEFAULT true");
        add_col!("ALTER TABLE models ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT ''");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS timezone TEXT NOT NULL DEFAULT 'UTC'");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS balance DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS frozen DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS token_version BIGINT NOT NULL DEFAULT 0");
        add_col!("ALTER TABLE channels ADD COLUMN IF NOT EXISTS anthropic_compat BOOLEAN NOT NULL DEFAULT false");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'user'");
        add_col!("ALTER TABLE recharge_keys ADD COLUMN IF NOT EXISTS expires_at TEXT");
        add_col!("ALTER TABLE recharge_keys ADD COLUMN IF NOT EXISTS revoked BOOLEAN NOT NULL DEFAULT false");
        add_col!("ALTER TABLE usage_logs ADD COLUMN IF NOT EXISTS client_ip TEXT");
        add_col!("ALTER TABLE model_channels ADD COLUMN IF NOT EXISTS upstream_model TEXT");
        add_col!("ALTER TABLE usage_logs ADD COLUMN IF NOT EXISTS cache_read_price DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!("ALTER TABLE usage_logs ADD COLUMN IF NOT EXISTS endpoint_id BIGINT");

        // Indexes
        macro_rules! add_idx {
            ($sql:expr) => {
                let _ = sqlx::raw_sql($sql)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| DbError(format!("Migration index error: {}", e)));
            };
        }
        add_idx!("CREATE INDEX IF NOT EXISTS idx_usage_user_id ON usage_logs(user_id)");
        add_idx!("CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_logs(timestamp)");
        add_idx!("CREATE INDEX IF NOT EXISTS idx_usage_user_timestamp ON usage_logs(user_id, timestamp)");
        add_idx!("CREATE INDEX IF NOT EXISTS idx_wallet_tx_user ON wallet_transactions(user_id)");
        add_idx!("CREATE INDEX IF NOT EXISTS idx_wallet_tx_created ON wallet_transactions(created_at)");

        // Create content_filter_rules table
        let _ = sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS content_filter_rules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL DEFAULT '',
                pattern_type TEXT NOT NULL DEFAULT 'keyword',
                pattern TEXT NOT NULL,
                action TEXT NOT NULL DEFAULT 'block',
                scope TEXT NOT NULL DEFAULT 'both',
                channel_id TEXT,
                replacement TEXT DEFAULT '[REDACTED]',
                enabled BOOLEAN NOT NULL DEFAULT true,
                priority INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration error: {}", e)))?;

        // Create probe_results table
        let _ = sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS probe_results (
                id TEXT PRIMARY KEY,
                channel_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                success BOOLEAN NOT NULL,
                latency_ms BIGINT NOT NULL,
                error TEXT,
                probed_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration error: {}", e)))?;
        let _ = sqlx::raw_sql("CREATE INDEX IF NOT EXISTS idx_probe_channel ON probe_results(channel_id)")
            .execute(&self.pool).await;
        let _ = sqlx::raw_sql("CREATE INDEX IF NOT EXISTS idx_probe_model ON probe_results(model_id)")
            .execute(&self.pool).await;

        // Set admin role for any user who was historically created as 'admin'
        let _ = sqlx::raw_sql("UPDATE users SET role='admin' WHERE id='admin' AND role='user'")
            .execute(&self.pool)
            .await;

        // ── Deduplicate models by name ──────────────────────────────────
        // Step 1: merge duplicate rows (idempotent — safe to run repeatedly).
        let duplicates: Vec<(String, i64)> = sqlx::query_as(
            "SELECT LOWER(name), count(*) FROM models GROUP BY LOWER(name) HAVING count(*) > 1",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        for (name_lower, _) in &duplicates {
            let winner: Option<(String, String)> = sqlx::query_as(
                "SELECT id, name FROM models WHERE LOWER(name) = $1 \
                 ORDER BY (prompt_price + completion_price) DESC, id ASC LIMIT 1",
            )
            .bind(name_lower)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();

            if let Some((ref winner_id, ref canonical_name)) = winner {
                let _ = sqlx::query(
                    "INSERT INTO model_channels (model_id, channel_id, priority)
                     SELECT $1, mc.channel_id, mc.priority
                     FROM model_channels mc JOIN models m ON mc.model_id = m.id
                     WHERE LOWER(m.name) = $2 AND m.id != $1
                     ON CONFLICT (model_id, channel_id) DO NOTHING",
                )
                .bind(winner_id).bind(name_lower)
                .execute(&self.pool).await;

                let _ = sqlx::query(
                    "INSERT INTO user_subscriptions (user_id, model_id, created_at)
                     SELECT us.user_id, $1, us.created_at
                     FROM user_subscriptions us JOIN models m ON us.model_id = m.id
                     WHERE LOWER(m.name) = $2 AND m.id != $1
                     ON CONFLICT (user_id, model_id) DO NOTHING",
                )
                .bind(winner_id).bind(name_lower)
                .execute(&self.pool).await;

                let _ = sqlx::query(
                    "DELETE FROM models WHERE LOWER(name) = $1 AND id != $2",
                )
                .bind(name_lower).bind(winner_id)
                .execute(&self.pool).await;

                let _ = sqlx::query("UPDATE models SET name = $1 WHERE id = $2")
                    .bind(canonical_name).bind(winner_id)
                    .execute(&self.pool).await;

                tracing::info!("Migration: deduplicated model '{}' → kept id={}", name_lower, winner_id);
            }
        }

        // Step 2: verify. If duplicates remain after dedup, abort startup.
        let remaining: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM (SELECT 1 FROM models GROUP BY LOWER(name) HAVING count(*) > 1) t",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        if remaining > 0 {
            tracing::error!(
                "Migration dedup failed: {} model names still have duplicate rows. \
                 Startup aborted — fix data manually.",
                remaining
            );
            return Err(DbError(
                "Duplicate model names remain after dedup — cannot add UNIQUE constraint".into(),
            ));
        }

        // Step 3: add the constraint. ADD CONSTRAINT does not support
        // IF NOT EXISTS in PostgreSQL — try and catch "already exists".
        let result = sqlx::raw_sql(
            "ALTER TABLE models ADD CONSTRAINT models_name_unique UNIQUE (name)",
        )
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => tracing::info!("models.name UNIQUE constraint created"),
            Err(e) if e.to_string().contains("already exists") => {
                tracing::info!("models.name UNIQUE constraint already exists, skipping");
            }
            Err(e) => {
                tracing::error!(
                    "Failed to create models.name UNIQUE constraint: {}. \
                     This usually means duplicate rows exist.",
                    e
                );
                return Err(DbError(format!(
                    "Model name UNIQUE constraint creation failed: {}", e
                )));
            }
        }

        tracing::info!("models.name UNIQUE constraint ready");

        Ok(())
    }

    // ── Users ────────────────────────────────────────────────────────────

    async fn list_users(&self) -> Result<Vec<User>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency FROM users ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let mut idx = 0usize;
                Self::map_user_row(r, &mut idx)
            })
            .collect())
    }

    async fn get_user(&self, id: &str) -> Result<Option<User>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| {
            let mut idx = 0usize;
            Self::map_user_row(r, &mut idx)
        }))
    }

    async fn get_user_with_password(&self, id: &str) -> Result<Option<User>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| {
            let mut idx = 0usize;
            Self::map_user_with_pw_row(r, &mut idx)
        }))
    }

    async fn create_user(&self, user: &User) -> Result<(), DbError> {
        let (rpm, tpm) = user
            .rate_limits
            .as_ref()
            .map(|r| (r.rpm.map(|v| v as i64), r.tpm.map(|v| v as i64)))
            .unwrap_or((None, None));
        let pw_hash = user.password_hash.as_deref().unwrap_or("");
        let tz = if user.timezone.is_empty() { "UTC" } else { &user.timezone };
        let role = if user.role.is_empty() { "user" } else { &user.role };
        sqlx::query(
            "INSERT INTO users (id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&user.id)
        .bind(&user.name)
        .bind(pw_hash)
        .bind(rpm)
        .bind(tpm)
        .bind(tz)
        .bind(user.token_version)
        .bind(role)
        .bind(user.concurrency_limit as i64)
        .bind(&user.currency)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_user(&self, user: &User) -> Result<(), DbError> {
        let (rpm, tpm) = user
            .rate_limits
            .as_ref()
            .map(|r| (r.rpm.map(|v| v as i64), r.tpm.map(|v| v as i64)))
            .unwrap_or((None, None));
        let tz = if user.timezone.is_empty() { "UTC" } else { &user.timezone };
        if let Some(ref pw) = user.password_hash {
            sqlx::query(
                "UPDATE users SET name = $1, password_hash = $2, rpm = $3, tpm = $4, timezone = $5, token_version = $6, role = $7, concurrency_limit = $8, currency = $9 WHERE id = $10",
            )
            .bind(&user.name)
            .bind(pw)
            .bind(rpm)
            .bind(tpm)
            .bind(tz)
            .bind(user.token_version)
            .bind(&user.role)
            .bind(user.concurrency_limit as i64)
            .bind(&user.currency)
            .bind(&user.id)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                "UPDATE users SET name = $1, rpm = $2, tpm = $3, timezone = $4, token_version = $5, role = $6, concurrency_limit = $7, currency = $8 WHERE id = $9",
            )
            .bind(&user.name)
            .bind(rpm)
            .bind(tpm)
            .bind(tz)
            .bind(user.token_version)
            .bind(&user.role)
            .bind(user.concurrency_limit as i64)
            .bind(&user.currency)
            .bind(&user.id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn delete_user(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn count_admins(&self) -> Result<i64, DbError> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE role = 'admin'")
            .fetch_one(&self.pool)
            .await?;
        Ok(count)
    }

    async fn get_user_timezone(&self, id: &str) -> Result<String, DbError> {
        let result: Option<(String,)> = sqlx::query_as("SELECT timezone FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result.map(|r| r.0).unwrap_or_else(|| "UTC".to_string()))
    }

    async fn update_user_timezone(&self, id: &str, timezone: &str) -> Result<(), DbError> {
        let tz = if timezone.is_empty() { "UTC" } else { timezone };
        sqlx::query("UPDATE users SET timezone = $1 WHERE id = $2")
            .bind(tz)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_user_currency(&self, id: &str) -> Result<String, DbError> {
        let rows = sqlx::query_as::<_, (String,)>("SELECT currency FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(rows.map(|r| r.0).unwrap_or_else(|| "usd".to_string()))
    }

    async fn update_user_currency(&self, id: &str, currency: &str) -> Result<(), DbError> {
        let cur = if currency.is_empty() { "usd" } else { currency };
        sqlx::query("UPDATE users SET currency = $1 WHERE id = $2")
            .bind(cur)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── API Keys ─────────────────────────────────────────────────────────

    async fn list_api_keys(&self, user_id: &str) -> Result<Vec<ApiKey>, DbError> {
        let rows = sqlx::query(
            "SELECT key, user_id, name, enabled, expires_at, spend_limit, allowed_models FROM api_keys WHERE user_id = $1 ORDER BY key",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let allowed_models_str: Option<String> = r.get(6);
                ApiKey {
                    key: r.get(0),
                    user_id: r.get(1),
                    name: r.get(2),
                    enabled: r.get(3),
                    expires_at: r.get(4),
                    spend_limit: r.get(5),
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                }
            })
            .collect())
    }

    async fn create_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
        sqlx::query(
            "INSERT INTO api_keys (key, user_id, name, enabled, expires_at, spend_limit, allowed_models) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&key.key)
        .bind(&key.user_id)
        .bind(&key.name)
        .bind(key.enabled)
        .bind(&key.expires_at)
        .bind(key.spend_limit)
        .bind(allowed)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_api_key(&self, key: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM api_keys WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
        sqlx::query(
            "UPDATE api_keys SET name = $1, enabled = $2, expires_at = $3, spend_limit = $4, allowed_models = $5 WHERE key = $6",
        )
        .bind(&key.name)
        .bind(key.enabled)
        .bind(&key.expires_at)
        .bind(key.spend_limit)
        .bind(allowed)
        .bind(&key.key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn lookup_key(&self, key: &str) -> Result<Option<(User, ApiKey)>, DbError> {
        let rows = sqlx::query(
            "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, u.token_version, u.role, u.concurrency_limit, u.currency, \
             a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models \
             FROM api_keys a JOIN users u ON u.id = a.user_id WHERE a.key = $1",
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| {
            let allowed_models_str: Option<String> = r.get(15);
            let api_key = ApiKey {
                key: r.get(9),
                user_id: r.get(10),
                name: r.get(11),
                enabled: r.get(12),
                expires_at: r.get(13),
                spend_limit: r.get(14),
                allowed_models: allowed_models_str
                    .filter(|s| !s.is_empty())
                    .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
            };
            let user = {
                let rpm: Option<i64> = r.get(2);
                let tpm: Option<i64> = r.get(3);
                User {
                    id: r.get(0),
                    name: r.get(1),
                    password_hash: None,
                    rate_limits: {
                        let rpm = rpm.map(|v| v as u64);
                        let tpm = tpm.map(|v| v as u64);
                        if rpm.is_some() || tpm.is_some() {
                            Some(crate::domain::user::RateLimit { rpm, tpm })
                        } else {
                            None
                        }
                    },
                    timezone: r.get::<Option<String>, _>(4).unwrap_or_default(),
                    token_version: r.get::<i64, _>(5),
                    role: r.get::<Option<String>, _>(6).unwrap_or_default(),
                    concurrency_limit: r.get::<i64, _>(7) as u32,
                    currency: r.get::<Option<String>, _>(8).unwrap_or_default(),
                }
            };
            (user, api_key)
        }))
    }

    async fn all_api_keys(&self) -> Result<Vec<(User, ApiKey)>, DbError> {
        let rows = sqlx::query(
            "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, u.token_version, u.role, u.concurrency_limit, u.currency, \
             a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models \
             FROM api_keys a JOIN users u ON u.id = a.user_id ORDER BY a.key",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let allowed_models_str: Option<String> = r.get(15);
                let api_key = ApiKey {
                    key: r.get(9),
                    user_id: r.get(10),
                    name: r.get(11),
                    enabled: r.get(12),
                    expires_at: r.get(13),
                    spend_limit: r.get(14),
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                };
                let user = {
                    let rpm: Option<i64> = r.get(2);
                    let tpm: Option<i64> = r.get(3);
                    User {
                        id: r.get(0),
                        name: r.get(1),
                        password_hash: None,
                        rate_limits: {
                            let rpm = rpm.map(|v| v as u64);
                            let tpm = tpm.map(|v| v as u64);
                            if rpm.is_some() || tpm.is_some() {
                                Some(crate::domain::user::RateLimit { rpm, tpm })
                            } else {
                                None
                            }
                        },
                        timezone: r.get::<Option<String>, _>(4).unwrap_or_default(),
                        token_version: r.get::<i64, _>(5),
                        role: r.get::<Option<String>, _>(6).unwrap_or_default(),
                        concurrency_limit: r.get::<i64, _>(7) as u32,
                        currency: r.get::<Option<String>, _>(8).unwrap_or_default(),
                    }
                };
                (user, api_key)
            })
            .collect())
    }

    // ── Channels & Endpoints ─────────────────────────────────────────────

    async fn list_channels(&self) -> Result<Vec<Channel>, DbError> {
        let ch_rows = sqlx::query(
            "SELECT id, name, provider, priority, enabled, anthropic_compat FROM channels ORDER BY priority, id",
        )
        .fetch_all(&self.pool)
        .await?;

        let ep_rows = sqlx::query(
            "SELECT id, channel_id, url, api_key, weight, timeout_secs, enabled FROM endpoints ORDER BY channel_id",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut channels: Vec<Channel> = ch_rows
            .iter()
            .map(|r| Channel {
                id: r.get(0),
                name: r.get(1),
                provider: r.get(2),
                priority: r.get(3),
                enabled: r.get(4),
                anthropic_compat: r.get(5),
                endpoints: Vec::new(),
            })
            .collect();

        let mut eps_by_channel: std::collections::HashMap<String, Vec<Endpoint>> =
            std::collections::HashMap::new();
        for r in &ep_rows {
            let ch_id: String = r.get(1);
            eps_by_channel.entry(ch_id).or_default().push(Endpoint {
                id: Some(r.get(0)),
                channel_id: r.get(1),
                url: r.get(2),
                api_key: r.get(3),
                weight: {
                    let w: i32 = r.get(4);
                    w as u32
                },
                timeout_secs: {
                    let t: Option<i64> = r.get(5);
                    t.map(|v| v as u64)
                },
                enabled: r.get(6),
            });
        }
        for ch in &mut channels {
            if let Some(eps) = eps_by_channel.remove(&ch.id) {
                ch.endpoints = eps;
            }
        }
        Ok(channels)
    }

    async fn get_channel(&self, id: &str) -> Result<Option<Channel>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, provider, priority, enabled, anthropic_compat FROM channels WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        if let Some(r) = rows.first() {
            let mut ch = Channel {
                id: r.get(0),
                name: r.get(1),
                provider: r.get(2),
                priority: r.get(3),
                enabled: r.get(4),
                anthropic_compat: r.get(5),
                endpoints: Vec::new(),
            };
            let eps = sqlx::query(
                "SELECT id, channel_id, url, api_key, weight, timeout_secs, enabled FROM endpoints WHERE channel_id = $1",
            )
            .bind(&ch.id)
            .fetch_all(&self.pool)
            .await?;
            ch.endpoints = eps
                .iter()
                .map(|r| Endpoint {
                    id: Some(r.get(0)),
                    channel_id: r.get(1),
                    url: r.get(2),
                    api_key: r.get(3),
                    weight: {
                        let w: i32 = r.get(4);
                        w as u32
                    },
                    timeout_secs: {
                        let t: Option<i64> = r.get(5);
                        t.map(|v| v as u64)
                    },
                    enabled: r.get(6),
                })
                .collect();
            Ok(Some(ch))
        } else {
            Ok(None)
        }
    }

    async fn create_channel(&self, ch: &Channel) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO channels (id, name, provider, priority, enabled, anthropic_compat) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&ch.id)
        .bind(&ch.name)
        .bind(&ch.provider)
        .bind(ch.priority)
        .bind(ch.enabled)
        .bind(ch.anthropic_compat)
        .execute(&self.pool)
        .await?;
        for ep in &ch.endpoints {
            sqlx::query(
                "INSERT INTO endpoints (channel_id, url, api_key, weight, timeout_secs, enabled) VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&ch.id)
            .bind(&ep.url)
            .bind(&ep.api_key)
            .bind(ep.weight as i32)
            .bind(ep.timeout_secs.map(|v| v as i64))
            .bind(ep.enabled)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn update_channel(&self, ch: &Channel) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE channels SET name = $1, provider = $2, priority = $3, enabled = $4, anthropic_compat = $5 WHERE id = $6",
        )
        .bind(&ch.name)
        .bind(&ch.provider)
        .bind(ch.priority)
        .bind(ch.enabled)
        .bind(ch.anthropic_compat)
        .bind(&ch.id)
        .execute(&self.pool)
        .await?;
        sqlx::query("DELETE FROM endpoints WHERE channel_id = $1")
            .bind(&ch.id)
            .execute(&self.pool)
            .await?;
        for ep in &ch.endpoints {
            sqlx::query(
                "INSERT INTO endpoints (channel_id, url, api_key, weight, timeout_secs, enabled) VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&ch.id)
            .bind(&ep.url)
            .bind(&ep.api_key)
            .bind(ep.weight as i32)
            .bind(ep.timeout_secs.map(|v| v as i64))
            .bind(ep.enabled)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn delete_channel(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM channels WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_endpoint(&self, id: i64) -> Result<Option<Endpoint>, DbError> {
        let rows = sqlx::query(
            "SELECT id, channel_id, url, api_key, weight, timeout_secs, enabled FROM endpoints WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| Endpoint {
            id: Some(r.get(0)),
            channel_id: r.get(1),
            url: r.get(2),
            api_key: r.get(3),
            weight: {
                let w: i32 = r.get(4);
                w as u32
            },
            timeout_secs: {
                let t: Option<i64> = r.get(5);
                t.map(|v| v as u64)
            },
            enabled: r.get(6),
        }))
    }

    async fn update_endpoint_enabled(&self, id: i64, enabled: bool) -> Result<(), DbError> {
        sqlx::query("UPDATE endpoints SET enabled = $1 WHERE id = $2")
            .bind(enabled)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Models ───────────────────────────────────────────────────────────

    async fn list_models(&self) -> Result<Vec<Model>, DbError> {
        let m_rows = sqlx::query(
            "SELECT id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category FROM models ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;

        let b_rows = sqlx::query(
            "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, ''), mc.upstream_model \
             FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id \
             ORDER BY mc.model_id, mc.priority",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut models: Vec<Model> = m_rows
            .iter()
            .map(|r| Model {
                id: r.get(0),
                name: r.get(1),
                model_pattern: r.get(2),
                pricing: Pricing {
                    prompt_price: r.get(3),
                    completion_price: r.get(4),
                    cache_read_price: r.get(5),
                    cache_write_price: r.get(6),
                    image_input_price: r.get(7),
                    audio_input_price: r.get(8),
                    audio_output_price: r.get(9),
                },
                channels: Vec::new(),
                published: r.get::<bool, _>(10),
                context_length: r.get(11),
                category: r.get::<Option<String>, _>(12).unwrap_or_default(),
            })
            .collect();

        let mut by_model: std::collections::HashMap<String, Vec<ModelChannel>> =
            std::collections::HashMap::new();
        for r in &b_rows {
            let model_id: String = r.get(0);
            by_model.entry(model_id).or_default().push(ModelChannel {
                model_id: r.get(0),
                channel_id: r.get(1),
                priority: r.get(2),
                provider: r.get::<Option<String>, _>(3).unwrap_or_default(),
                upstream_model: r.get::<Option<String>, _>(4),
            });
        }
        for m in &mut models {
            if let Some(bindings) = by_model.remove(&m.id) {
                m.channels = bindings;
            }
        }
        Ok(models)
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category FROM models WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        if let Some(r) = rows.first() {
            let mut m = Model {
                id: r.get(0),
                name: r.get(1),
                model_pattern: r.get(2),
                pricing: Pricing {
                    prompt_price: r.get(3),
                    completion_price: r.get(4),
                    cache_read_price: r.get(5),
                    cache_write_price: r.get(6),
                    image_input_price: r.get(7),
                    audio_input_price: r.get(8),
                    audio_output_price: r.get(9),
                },
                channels: Vec::new(),
                published: r.get::<bool, _>(10),
                context_length: r.get(11),
                category: r.get::<Option<String>, _>(12).unwrap_or_default(),
            };
            let bindings = sqlx::query(
                "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, '') \
                 FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id \
                 WHERE mc.model_id = $1 ORDER BY mc.priority",
            )
            .bind(&m.id)
            .fetch_all(&self.pool)
            .await?;
            m.channels = bindings
                .iter()
                .map(|r| ModelChannel {
                    model_id: r.get(0),
                    channel_id: r.get(1),
                    priority: r.get(2),
                    provider: r.get::<Option<String>, _>(3).unwrap_or_default(),
                    upstream_model: r.get::<Option<String>, _>(4),
                })
                .collect();
            Ok(Some(m))
        } else {
            Ok(None)
        }
    }

    async fn create_model(&self, m: &Model) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO models (id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(&m.id)
        .bind(&m.name)
        .bind(&m.model_pattern)
        .bind(m.pricing.prompt_price)
        .bind(m.pricing.completion_price)
        .bind(m.pricing.cache_read_price)
        .bind(m.pricing.cache_write_price)
        .bind(m.pricing.image_input_price)
        .bind(m.pricing.audio_input_price)
        .bind(m.pricing.audio_output_price)
        .bind(m.published)
        .bind(m.context_length)
        .bind(&m.category)
        .execute(&self.pool)
        .await?;

        // Use the actual ID in the DB (may differ from m.id after upsert)
        let model_id: (String,) = sqlx::query_as("SELECT id FROM models WHERE name = $1")
            .bind(&m.name)
            .fetch_one(&self.pool)
            .await?;

        for binding in &m.channels {
            sqlx::query(
                "INSERT INTO model_channels (model_id, channel_id, priority, upstream_model) \
                 VALUES ($1, $2, $3, $4) ON CONFLICT (model_id, channel_id) DO UPDATE SET priority = EXCLUDED.priority, upstream_model = EXCLUDED.upstream_model",
            )
            .bind(&model_id.0)
            .bind(&binding.channel_id)
            .bind(binding.priority)
            .bind(&binding.upstream_model)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn update_model(&self, old_id: &str, m: &Model) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE models SET id=$1, name=$2, model_pattern=$3, prompt_price=$4, completion_price=$5, \
             cache_read_price=$6, cache_write_price=$7, image_input_price=$8, audio_input_price=$9, \
             audio_output_price=$10, published=$11, context_length=$12, category=$13 WHERE id=$14",
        )
        .bind(&m.id)
        .bind(&m.name)
        .bind(&m.model_pattern)
        .bind(m.pricing.prompt_price)
        .bind(m.pricing.completion_price)
        .bind(m.pricing.cache_read_price)
        .bind(m.pricing.cache_write_price)
        .bind(m.pricing.image_input_price)
        .bind(m.pricing.audio_input_price)
        .bind(m.pricing.audio_output_price)
        .bind(m.published)
        .bind(m.context_length)
        .bind(&m.category)
        .bind(old_id)
        .execute(&self.pool)
        .await?;
        // Delete old bindings by old_id (model_channels FK references old model id)
        sqlx::query("DELETE FROM model_channels WHERE model_id = $1")
            .bind(old_id)
            .execute(&self.pool)
            .await?;
        for binding in &m.channels {
            sqlx::query(
                "INSERT INTO model_channels (model_id, channel_id, priority, upstream_model) VALUES ($1, $2, $3, $4)",
            )
            .bind(&m.id)
            .bind(&binding.channel_id)
            .bind(binding.priority)
            .bind(&binding.upstream_model)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn delete_model(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM models WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_published_models(&self) -> Result<Vec<Model>, DbError> {
        let m_rows = sqlx::query(
            "SELECT id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category FROM models \
             WHERE published = true ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;

        let b_rows = sqlx::query(
            "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, ''), mc.upstream_model \
             FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id \
             ORDER BY mc.model_id, mc.priority",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut models: Vec<Model> = m_rows
            .iter()
            .map(|r| Model {
                id: r.get(0),
                name: r.get(1),
                model_pattern: r.get(2),
                pricing: Pricing {
                    prompt_price: r.get(3),
                    completion_price: r.get(4),
                    cache_read_price: r.get(5),
                    cache_write_price: r.get(6),
                    image_input_price: r.get(7),
                    audio_input_price: r.get(8),
                    audio_output_price: r.get(9),
                },
                channels: Vec::new(),
                published: true,
                context_length: r.get(11),
                category: r.get::<Option<String>, _>(12).unwrap_or_default(),
            })
            .collect();

        let mut by_model: std::collections::HashMap<String, Vec<ModelChannel>> =
            std::collections::HashMap::new();
        for r in &b_rows {
            let model_id: String = r.get(0);
            by_model.entry(model_id).or_default().push(ModelChannel {
                model_id: r.get(0),
                channel_id: r.get(1),
                priority: r.get(2),
                provider: r.get::<Option<String>, _>(3).unwrap_or_default(),
                upstream_model: r.get::<Option<String>, _>(4),
            });
        }
        for m in &mut models {
            if let Some(bindings) = by_model.remove(&m.id) {
                m.channels = bindings;
            }
        }
        Ok(models)
    }

    async fn set_model_published(&self, id: &str, published: bool) -> Result<(), DbError> {
        sqlx::query("UPDATE models SET published = $1 WHERE id = $2")
            .bind(published)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_model_pricing(&self, id: &str, pricing: &Pricing) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE models SET prompt_price=$1, completion_price=$2, cache_read_price=$3, \
             cache_write_price=$4, image_input_price=$5, audio_input_price=$6, \
             audio_output_price=$7 WHERE id=$8",
        )
        .bind(pricing.prompt_price)
        .bind(pricing.completion_price)
        .bind(pricing.cache_read_price)
        .bind(pricing.cache_write_price)
        .bind(pricing.image_input_price)
        .bind(pricing.audio_input_price)
        .bind(pricing.audio_output_price)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_model_context_length(&self, id: &str, context_length: i64) -> Result<(), DbError> {
        sqlx::query("UPDATE models SET context_length = $1 WHERE id = $2")
            .bind(context_length)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Subscriptions ────────────────────────────────────────────────────

    async fn subscribe_user(&self, user_id: &str, model_id: &str) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO user_subscriptions (user_id, model_id, created_at) VALUES ($1, $2, $3) \
             ON CONFLICT DO NOTHING",
        )
        .bind(user_id)
        .bind(model_id)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn unsubscribe_user(&self, user_id: &str, model_id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM user_subscriptions WHERE user_id = $1 AND model_id = $2")
            .bind(user_id)
            .bind(model_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete_subscriptions_by_model(&self, model_id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM user_subscriptions WHERE model_id = $1")
            .bind(model_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_subscribed_model_ids(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT model_id FROM user_subscriptions WHERE user_id = $1")
                .bind(user_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn list_subscriptions(&self, user_id: &str) -> Result<Vec<Model>, DbError> {
        let m_rows = sqlx::query(
            "SELECT m.id, m.name, m.model_pattern, m.prompt_price, m.completion_price, \
             m.cache_read_price, m.cache_write_price, m.image_input_price, m.audio_input_price, \
             m.audio_output_price, m.published, m.context_length, m.category \
             FROM models m INNER JOIN user_subscriptions s ON m.id = s.model_id \
             WHERE s.user_id = $1 ORDER BY m.id",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        let mut models: Vec<Model> = m_rows
            .iter()
            .map(|r| Model {
                id: r.get(0),
                name: r.get(1),
                model_pattern: r.get(2),
                pricing: Pricing {
                    prompt_price: r.get(3),
                    completion_price: r.get(4),
                    cache_read_price: r.get(5),
                    cache_write_price: r.get(6),
                    image_input_price: r.get(7),
                    audio_input_price: r.get(8),
                    audio_output_price: r.get(9),
                },
                channels: Vec::new(),
                published: r.get::<bool, _>(10),
                context_length: r.get(11),
                category: r.get::<Option<String>, _>(12).unwrap_or_default(),
            })
            .collect();

        let b_rows = sqlx::query(
            "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, ''), mc.upstream_model \
             FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id \
             ORDER BY mc.model_id, mc.priority",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut by_model: std::collections::HashMap<String, Vec<ModelChannel>> =
            std::collections::HashMap::new();
        for r in &b_rows {
            let model_id: String = r.get(0);
            by_model.entry(model_id).or_default().push(ModelChannel {
                model_id: r.get(0),
                channel_id: r.get(1),
                priority: r.get(2),
                provider: r.get::<Option<String>, _>(3).unwrap_or_default(),
                upstream_model: r.get::<Option<String>, _>(4),
            });
        }
        for m in &mut models {
            if let Some(bindings) = by_model.remove(&m.id) {
                m.channels = bindings;
            }
        }
        Ok(models)
    }

    // ── Routing Rules ────────────────────────────────────────────────────

    async fn list_rules(&self) -> Result<Vec<RoutingRule>, DbError> {
        let rows = sqlx::query(
            "SELECT name, user_id, model_pattern, channel_id FROM routing_rules ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RoutingRule {
                name: r.get(0),
                user_id: r.get(1),
                model_pattern: r.get(2),
                channel_id: r.get(3),
            })
            .collect())
    }

    async fn create_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO routing_rules (name, user_id, model_pattern, channel_id) VALUES ($1, $2, $3, $4)",
        )
        .bind(&r.name)
        .bind(&r.user_id)
        .bind(&r.model_pattern)
        .bind(&r.channel_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE routing_rules SET user_id = $1, model_pattern = $2, channel_id = $3 WHERE name = $4",
        )
        .bind(&r.user_id)
        .bind(&r.model_pattern)
        .bind(&r.channel_id)
        .bind(&r.name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_rule(&self, name: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM routing_rules WHERE name = $1")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Usage Logs ───────────────────────────────────────────────────────

    async fn insert_usage(&self, record: &UsageRecord) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO usage_logs (timestamp, request_id, user_id, user_name, channel_id, model, \
             prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
             request_body, response_body, reasoning_body, api_key_name, api_format, stream, \
             cache_hit_input_tokens, prompt_price, completion_price, cache_read_price, client_ip) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)",
        )
        .bind(&record.timestamp)
        .bind(&record.request_id)
        .bind(&record.user_id)
        .bind(&record.user_name)
        .bind(&record.channel_id)
        .bind(&record.model)
        .bind(record.prompt_tokens as i64)
        .bind(record.completion_tokens as i64)
        .bind(record.total_tokens as i64)
        .bind(record.latency_ms as i64)
        .bind(record.status_code as i32)
        .bind(record.success)
        .bind(&record.request_body)
        .bind(&record.response_body)
        .bind(&record.reasoning_body)
        .bind(&record.api_key_name)
        .bind(&record.api_format)
        .bind(record.stream)
        .bind(record.cache_hit_input_tokens as i64)
        .bind(record.prompt_price)
        .bind(record.completion_price)
        .bind(record.cache_read_price)
        .bind(&record.client_ip)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn count_usage(&self) -> Result<usize, DbError> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_logs")
            .fetch_one(&self.pool)
            .await?;
        Ok(count as usize)
    }

    async fn count_usage_by_user(&self, user_id: &str) -> Result<usize, DbError> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM usage_logs WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(count as usize)
    }

    async fn count_usage_filtered(&self, filter: &UsageFilter) -> Result<usize, DbError> {
        let mut builder: QueryBuilder<'_, sqlx::Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM usage_logs WHERE 1=1");

        if let Some(ref uid) = filter.user_id {
            builder.push(" AND user_id = ");
            builder.push_bind(uid);
        }
        if let Some(ref m) = filter.model {
            builder.push(" AND model LIKE ");
            builder.push_bind(format!("%{}%", m));
        }
        if let Some(ref k) = filter.api_key_name {
            builder.push(" AND api_key_name LIKE ");
            builder.push_bind(format!("%{}%", k));
        }
        if let Some(ref f) = filter.api_format {
            builder.push(" AND api_format = ");
            builder.push_bind(f);
        }
        if let Some(ref sd) = filter.start_date {
            builder.push(" AND timestamp >= ");
            builder.push_bind(sd);
        }
        if let Some(ref ed) = filter.end_date {
            builder.push(" AND timestamp <= ");
            builder.push_bind(ed);
        }

        let (count,): (i64,) = builder
            .build_query_as()
            .fetch_one(&self.pool)
            .await?;
        Ok(count as usize)
    }

    async fn query_usage(
        &self,
        limit: usize,
        offset: usize,
        filter: &UsageFilter,
    ) -> Result<Vec<UsageRecord>, DbError> {
        let mut builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(
            "SELECT timestamp, request_id, user_id, user_name, channel_id, model, \
             prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
             api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, \
             cache_read_price, client_ip \
             FROM usage_logs WHERE 1=1",
        );

        if let Some(ref uid) = filter.user_id {
            builder.push(" AND user_id = ");
            builder.push_bind(uid);
        }
        if let Some(ref m) = filter.model {
            builder.push(" AND model LIKE ");
            builder.push_bind(format!("%{}%", m));
        }
        if let Some(ref k) = filter.api_key_name {
            builder.push(" AND api_key_name LIKE ");
            builder.push_bind(format!("%{}%", k));
        }
        if let Some(ref f) = filter.api_format {
            builder.push(" AND api_format = ");
            builder.push_bind(f);
        }
        if let Some(ref sd) = filter.start_date {
            builder.push(" AND timestamp >= ");
            builder.push_bind(sd);
        }
        if let Some(ref ed) = filter.end_date {
            builder.push(" AND timestamp <= ");
            builder.push_bind(ed);
        }

        builder.push(" ORDER BY id DESC LIMIT ");
        builder.push_bind(limit as i64);
        builder.push(" OFFSET ");
        builder.push_bind(offset as i64);

        let rows = builder.build().fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| {
                let mut idx = 0usize;
                Self::map_usage_record(r, &mut idx)
            })
            .collect())
    }

    async fn get_usage_detail(&self, request_id: &str) -> Result<Option<UsageRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT timestamp, request_id, user_id, user_name, channel_id, model, \
             prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
             request_body, response_body, reasoning_body, api_key_name, api_format, stream, \
             cache_hit_input_tokens, prompt_price, completion_price, cache_read_price, client_ip \
             FROM usage_logs WHERE request_id = $1",
        )
        .bind(request_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| {
            let mut idx = 0usize;
            Self::map_usage_with_bodies(r, &mut idx)
        }))
    }

    async fn purge_usage_logs(&self, cutoff: &str) -> Result<usize, DbError> {
        let result = sqlx::query("DELETE FROM usage_logs WHERE timestamp < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() as usize)
    }

    async fn usage_stats_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<(u64, u64, u64, u64), DbError> {
        if let Some(uid) = user_id {
            let row: (i64, i64, i64, i64) = sqlx::query_as(
                "SELECT COUNT(*), \
                 COALESCE(SUM(CASE WHEN success = true THEN 1 ELSE 0 END),0), \
                 COALESCE(SUM(latency_ms)::bigint,0), \
                 COALESCE(SUM(total_tokens)::bigint,0) \
                 FROM usage_logs WHERE user_id = $1 AND timestamp >= $2",
            )
            .bind(uid)
            .bind(since)
            .fetch_one(&self.pool)
            .await?;
            Ok((row.0 as u64, row.1 as u64, row.2 as u64, row.3 as u64))
        } else {
            let row: (i64, i64, i64, i64) = sqlx::query_as(
                "SELECT COUNT(*), \
                 COALESCE(SUM(CASE WHEN success = true THEN 1 ELSE 0 END),0), \
                 COALESCE(SUM(latency_ms)::bigint,0), \
                 COALESCE(SUM(total_tokens)::bigint,0) \
                 FROM usage_logs WHERE timestamp >= $1",
            )
            .bind(since)
            .fetch_one(&self.pool)
            .await?;
            Ok((row.0 as u64, row.1 as u64, row.2 as u64, row.3 as u64))
        }
    }

    async fn usage_cost_rows_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, DbError> {
        let rows = if let Some(uid) = user_id {
            sqlx::query(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, \
                 prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
                 api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, \
                 cache_read_price, client_ip \
                 FROM usage_logs WHERE user_id = $1 AND timestamp >= $2 ORDER BY id ASC",
            )
            .bind(uid)
            .bind(since)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, \
                 prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
                 api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, \
                 cache_read_price, client_ip \
                 FROM usage_logs WHERE timestamp >= $1 ORDER BY id ASC",
            )
            .bind(since)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .iter()
            .map(|r| {
                let mut idx = 0usize;
                Self::map_usage_record(r, &mut idx)
            })
            .collect())
    }

    async fn query_usage_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, DbError> {
        let rows = if let Some(uid) = user_id {
            sqlx::query(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, \
                 prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
                 api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, \
                 cache_read_price \
                 FROM usage_logs WHERE user_id = $1 AND timestamp >= $2 ORDER BY id ASC",
            )
            .bind(uid)
            .bind(since)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, \
                 prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
                 api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, \
                 cache_read_price \
                 FROM usage_logs WHERE timestamp >= $1 ORDER BY id ASC",
            )
            .bind(since)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .iter()
            .map(|r| {
                let mut idx = 0usize;
                let timestamp: String = r.get(idx); idx += 1;
                let request_id: String = r.get(idx); idx += 1;
                let user_id: String = r.get(idx); idx += 1;
                let user_name: String = r.get(idx); idx += 1;
                let channel_id: String = r.get(idx); idx += 1;
                let model: String = r.get(idx); idx += 1;
                let prompt_tokens: i64 = r.get(idx); idx += 1;
                let completion_tokens: i64 = r.get(idx); idx += 1;
                let total_tokens: i64 = r.get(idx); idx += 1;
                let latency_ms: i64 = r.get(idx); idx += 1;
                let status_code: i32 = r.get(idx); idx += 1;
                let success: bool = r.get(idx); idx += 1;
                let api_key_name: Option<String> = r.get(idx); idx += 1;
                let api_format: String = r.get(idx); idx += 1;
                let stream: bool = r.get(idx); idx += 1;
                let cache_hit_input_tokens: i64 = r.get(idx); idx += 1;
                let prompt_price: f64 = r.get(idx); idx += 1;
                let completion_price: f64 = r.get(idx); idx += 1;
                let cache_read_price: f64 = r.get(idx);
                UsageRecord {
                    timestamp,
                    request_id,
                    user_id,
                    user_name,
                    channel_id,
                    model,
                    prompt_tokens: prompt_tokens as u64,
                    completion_tokens: completion_tokens as u64,
                    total_tokens: total_tokens as u64,
                    latency_ms: latency_ms as u64,
                    status_code: status_code as u16,
                    success,
                    request_body: None,
                    response_body: None,
                    reasoning_body: None,
                    api_key_name,
                    api_format,
                    stream,
                    cache_hit_input_tokens: cache_hit_input_tokens as u64,
                    prompt_price,
                    completion_price,
                    cache_read_price,
                    client_ip: None,
                }
            })
            .collect())
    }

    async fn daily_usage_counts(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, i64)>, DbError> {
        let day_expr = Self::day_expr(tz_offset_seconds);
        if let Some(uid) = user_id {
            let sql = format!(
                "SELECT {}, COUNT(*) FROM usage_logs WHERE user_id = $1 AND timestamp >= $2 \
                 GROUP BY day ORDER BY day ASC",
                day_expr
            );
            let rows = sqlx::query_as::<_, (String, i64)>(&sql)
                .bind(uid)
                .bind(since)
                .fetch_all(&self.pool)
                .await?;
            Ok(rows)
        } else {
            let sql = format!(
                "SELECT {}, COUNT(*) FROM usage_logs WHERE timestamp >= $1 GROUP BY day ORDER BY day ASC",
                day_expr
            );
            let rows = sqlx::query_as::<_, (String, i64)>(&sql)
                .bind(since)
                .fetch_all(&self.pool)
                .await?;
            Ok(rows)
        }
    }

    async fn daily_usage_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64, u64)>, DbError> {
        let day_expr = Self::day_expr(tz_offset_seconds);
        if let Some(uid) = user_id {
            let sql = format!(
                "SELECT {}, COUNT(*)::bigint, COALESCE(SUM(prompt_tokens),0)::bigint, \
                 COALESCE(SUM(completion_tokens),0)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                 COALESCE(SUM(CASE WHEN success=true THEN 1 ELSE 0 END),0)::bigint, \
                 COALESCE(SUM(latency_ms),0)::bigint, \
                 COALESCE(SUM(cache_hit_input_tokens),0)::bigint \
                 FROM usage_logs WHERE user_id = $1 AND timestamp >= $2 \
                 GROUP BY day ORDER BY day ASC",
                day_expr
            );
            let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64, i64)>(&sql)
                .bind(uid)
                .bind(since)
                .fetch_all(&self.pool)
                .await?;
            Ok(rows
                .into_iter()
                .map(|r| (r.0, r.1 as u64, r.2 as u64, r.3 as u64, r.4 as u64, r.5 as u64, r.6 as u64, r.7 as u64))
                .collect())
        } else {
            let sql = format!(
                "SELECT {}, COUNT(*)::bigint, COALESCE(SUM(prompt_tokens),0)::bigint, \
                 COALESCE(SUM(completion_tokens),0)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                 COALESCE(SUM(CASE WHEN success=true THEN 1 ELSE 0 END),0)::bigint, \
                 COALESCE(SUM(latency_ms),0)::bigint, \
                 COALESCE(SUM(cache_hit_input_tokens),0)::bigint \
                 FROM usage_logs WHERE timestamp >= $1 \
                 GROUP BY day ORDER BY day ASC",
                day_expr
            );
            let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64, i64)>(&sql)
                .bind(since)
                .fetch_all(&self.pool)
                .await?;
            Ok(rows
                .into_iter()
                .map(|r| (r.0, r.1 as u64, r.2 as u64, r.3 as u64, r.4 as u64, r.5 as u64, r.6 as u64, r.7 as u64))
                .collect())
        }
    }

    async fn model_activity(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64)>, DbError> {
        let rows = if let Some(uid) = user_id {
            sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64)>(
                "SELECT model, COUNT(*)::bigint, COALESCE(SUM(prompt_tokens),0)::bigint, \
                 COALESCE(SUM(completion_tokens),0)::bigint, \
                 COALESCE(SUM(CASE WHEN success=true THEN 1 ELSE 0 END),0)::bigint, \
                 COALESCE(SUM(CASE WHEN success=false THEN 1 ELSE 0 END),0)::bigint, \
                 COALESCE(SUM(cache_hit_input_tokens)::bigint,0) \
                 FROM usage_logs WHERE timestamp >= $1 AND user_id = $2 \
                 GROUP BY model ORDER BY COUNT(*) DESC",
            )
            .bind(since)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, i64, i64, i64, i64, i64, i64)>(
                "SELECT model, COUNT(*)::bigint, COALESCE(SUM(prompt_tokens),0)::bigint, \
                 COALESCE(SUM(completion_tokens),0)::bigint, \
                 COALESCE(SUM(CASE WHEN success=true THEN 1 ELSE 0 END),0)::bigint, \
                 COALESCE(SUM(CASE WHEN success=false THEN 1 ELSE 0 END),0)::bigint, \
                 COALESCE(SUM(cache_hit_input_tokens)::bigint,0) \
                 FROM usage_logs WHERE timestamp >= $1 \
                 GROUP BY model ORDER BY COUNT(*) DESC",
            )
            .bind(since)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|r| (r.0, r.1 as u64, r.2 as u64, r.3 as u64, r.4 as u64, r.5 as u64, r.6 as u64))
            .collect())
    }

    // ── Billing / Period ─────────────────────────────────────────────────

    async fn period_summary(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<(f64, u64, u64), DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (cost, count, tokens): (f64, i64, i64) = if let Some(uid) = user_id {
            sqlx::query_as(
                "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
                 completion_tokens / 1000000.0 * completion_price + \
                 cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
                 completion_tokens / 1000000.0 * completion_price + \
                 cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2",
            )
            .bind(&start)
            .bind(&end)
            .fetch_one(&self.pool)
            .await?
        };
        Ok((cost, count as u64, tokens as u64))
    }

    async fn period_model_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, f64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            sqlx::query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
                 completion_tokens / 1000000.0 * completion_price + \
                 cache_hit_input_tokens / 1000000.0 * cache_read_price), 0) \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
                 completion_tokens / 1000000.0 * completion_price + \
                 cache_hit_input_tokens / 1000000.0 * cache_read_price), 0) \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows)
    }

    async fn period_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, f64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            sqlx::query_as::<_, (String, String, f64)>(
                "SELECT ul.channel_id, COALESCE(c.name, ul.channel_id), COALESCE(SUM(ul.prompt_tokens / 1000000.0 * ul.prompt_price + \
                 ul.completion_tokens / 1000000.0 * ul.completion_price + \
                 ul.cache_hit_input_tokens / 1000000.0 * ul.cache_read_price), 0) \
                 FROM usage_logs ul LEFT JOIN channels c ON c.id = ul.channel_id \
                 WHERE ul.timestamp >= $1 AND ul.timestamp < $2 AND ul.user_id = $3 \
                 GROUP BY ul.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, String, f64)>(
                "SELECT ul.channel_id, COALESCE(c.name, ul.channel_id), COALESCE(SUM(ul.prompt_tokens / 1000000.0 * ul.prompt_price + \
                 ul.completion_tokens / 1000000.0 * ul.completion_price + \
                 ul.cache_hit_input_tokens / 1000000.0 * ul.cache_read_price), 0) \
                 FROM usage_logs ul LEFT JOIN channels c ON c.id = ul.channel_id \
                 WHERE ul.timestamp >= $1 AND ul.timestamp < $2 \
                 GROUP BY ul.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows)
    }

    async fn daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, f64, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            sqlx::query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
                 completion_tokens / 1000000.0 * completion_price + \
                 cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COUNT(*)::bigint \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                 GROUP BY day ORDER BY day DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
                 completion_tokens / 1000000.0 * completion_price + \
                 cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COUNT(*)::bigint \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(|(d, c, n)| (d, c, n as u64)).collect())
    }

    async fn count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<usize, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (count,): (i64,) = if let Some(uid) = user_id {
            sqlx::query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2",
            )
            .bind(&start)
            .bind(&end)
            .fetch_one(&self.pool)
            .await?
        };
        Ok(count as usize)
    }

    async fn daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, f64, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            sqlx::query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
                 completion_tokens / 1000000.0 * completion_price + \
                 cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COUNT(*)::bigint \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                 GROUP BY day ORDER BY day DESC LIMIT $4 OFFSET $5",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
                 completion_tokens / 1000000.0 * completion_price + \
                 cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COUNT(*)::bigint \
                 FROM usage_logs WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day DESC LIMIT $3 OFFSET $4",
            )
            .bind(&start)
            .bind(&end)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows.into_iter().map(|(d, c, n)| (d, c, n as u64)).collect())
    }

    async fn billing_months(&self) -> Result<Vec<String>, DbError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT LEFT(timestamp::text, 7) AS month FROM usage_logs ORDER BY month DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn billing_months_for_user(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT DISTINCT LEFT(timestamp::text, 7) AS month FROM usage_logs WHERE user_id = $1 ORDER BY month DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn period_summary_all(&self) -> Result<Vec<(String, f64, u64, u64)>, DbError> {
        let rows = sqlx::query_as::<_, (String, f64, i64, i64)>(
            "SELECT LEFT(timestamp::text, 7) AS month, \
             COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
             completion_tokens / 1000000.0 * completion_price + \
             cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
             COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
             FROM usage_logs GROUP BY month ORDER BY month DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(m, c, n, t)| (m, c, n as u64, t as u64))
            .collect())
    }

    async fn period_summary_for_user(&self, user_id: &str) -> Result<Vec<(String, f64, u64, u64)>, DbError> {
        let rows = sqlx::query_as::<_, (String, f64, i64, i64)>(
            "SELECT LEFT(timestamp::text, 7) AS month, \
             COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
             completion_tokens / 1000000.0 * completion_price + \
             cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
             COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
             FROM usage_logs WHERE user_id = $1 GROUP BY month ORDER BY month DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(m, c, n, t)| (m, c, n as u64, t as u64))
            .collect())
    }

    async fn lookup_model_pricing(&self, model_name: &str) -> Result<(f64, f64), DbError> {
        Ok(self.pricing_lookup(model_name).await)
    }

    // ── Wallet ───────────────────────────────────────────────────────────

    async fn get_wallet_balance(&self, user_id: &str) -> Result<(f64, f64), DbError> {
        let row: (f64, f64) = sqlx::query_as("SELECT balance, frozen FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row)
    }

    async fn update_wallet_balance(&self, user_id: &str, balance: f64) -> Result<(), DbError> {
        sqlx::query("UPDATE users SET balance = $1 WHERE id = $2")
            .bind(balance)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn add_wallet_transaction(
        &self,
        id: &str,
        user_id: &str,
        tx_type: &str,
        amount: f64,
        balance_before: f64,
        balance_after: f64,
        method: &str,
        status: &str,
        note: &str,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(id)
        .bind(user_id)
        .bind(tx_type)
        .bind(amount)
        .bind(balance_before)
        .bind(balance_after)
        .bind(method)
        .bind(status)
        .bind(note)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_wallet_transactions(
        &self,
        user_id: &str,
        page: usize,
        size: usize,
    ) -> Result<Vec<WalletTransactionRow>, DbError> {
        let offset = (page.saturating_sub(1)) * size;
        let rows = sqlx::query(
            "SELECT id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at \
             FROM wallet_transactions WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(user_id)
        .bind(size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| WalletTransactionRow {
                id: r.get(0),
                user_id: r.get(1),
                tx_type: r.get(2),
                amount: r.get(3),
                balance_before: r.get(4),
                balance_after: r.get(5),
                method: r.get(6),
                status: r.get(7),
                note: r.get(8),
                created_at: r.get(9),
            })
            .collect())
    }

    async fn count_wallet_transactions(&self, user_id: &str) -> Result<usize, DbError> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM wallet_transactions WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(count as usize)
    }

    async fn list_wallet_tx_by_dates(
        &self,
        user_id: Option<&str>,
        page: usize,
        size: usize,
        since: Option<&str>,
        until: Option<&str>,
        tx_type: Option<&str>,
    ) -> Result<(Vec<WalletTransactionRow>, usize), DbError> {
        // Build dynamic WHERE clause for wallet_transactions
        // Use a helper macro to avoid borrowing issues with the closure capturing &str
        macro_rules! add_filters {
            ($b:expr) => {
                if let Some(uid) = user_id {
                    $b.push(" AND user_id = ");
                    $b.push_bind(uid);
                }
                if let Some(s) = since {
                    $b.push(" AND created_at >= ");
                    $b.push_bind(s);
                }
                if let Some(u) = until {
                    $b.push(" AND created_at <= ");
                    $b.push_bind(u);
                }
                if let Some(t) = tx_type {
                    $b.push(" AND type = ");
                    $b.push_bind(t);
                }
            };
        }

        let mut count_builder: QueryBuilder<'_, sqlx::Postgres> =
            QueryBuilder::new("SELECT COUNT(DISTINCT LEFT(created_at::text, 10)) FROM wallet_transactions WHERE 1=1");

        let mut data_builder: QueryBuilder<'_, sqlx::Postgres> =
            QueryBuilder::new(
                "SELECT id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at \
                 FROM wallet_transactions WHERE 1=1",
            );

        let mut date_builder: QueryBuilder<'_, sqlx::Postgres> =
            QueryBuilder::new(
                "SELECT DISTINCT LEFT(created_at::text, 10) as tx_date FROM wallet_transactions WHERE 1=1",
            );

        add_filters!(count_builder);
        add_filters!(data_builder);
        add_filters!(date_builder);

        // Total distinct dates
        let (total_dates,): (i64,) = count_builder
            .build_query_as()
            .fetch_one(&self.pool)
            .await?;
        let total_dates = total_dates as usize;

        // Paginated dates
        let page_offset = (page.saturating_sub(1)) * size;
        date_builder.push(" ORDER BY tx_date DESC LIMIT ");
        date_builder.push_bind(size as i64);
        date_builder.push(" OFFSET ");
        date_builder.push_bind(page_offset as i64);

        let dates: Vec<String> = date_builder
            .build()
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(|r| r.get::<String, _>(0))
            .collect();

        if dates.is_empty() {
            return Ok((Vec::new(), total_dates));
        }

        // Fetch transactions for those dates using IN clause
        data_builder.push(" AND LEFT(created_at::text, 10) IN (");
        for (i, _) in dates.iter().enumerate() {
            if i > 0 {
                data_builder.push(", ");
            }
            data_builder.push_bind(&dates[i]);
        }
        data_builder.push(") ORDER BY created_at DESC");

        let rows = data_builder.build().fetch_all(&self.pool).await?;
        let transactions = rows
            .iter()
            .map(|r| WalletTransactionRow {
                id: r.get(0),
                user_id: r.get(1),
                tx_type: r.get(2),
                amount: r.get(3),
                balance_before: r.get(4),
                balance_after: r.get(5),
                method: r.get(6),
                status: r.get(7),
                note: r.get(8),
                created_at: r.get(9),
            })
            .collect();

        Ok((transactions, total_dates))
    }

    async fn get_total_consumed(&self, user_id: &str) -> Result<f64, DbError> {
        let result: Result<(f64,), _> = sqlx::query_as(
            "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
             completion_tokens / 1000000.0 * completion_price + \
             cache_hit_input_tokens / 1000000.0 * cache_read_price), 0) \
             FROM usage_logs WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await;
        Ok(result.unwrap_or((0.0,)).0)
    }

    async fn get_total_recharged(&self, user_id: &str) -> Result<f64, DbError> {
        let (amount,): (f64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(amount), 0) FROM wallet_transactions \
             WHERE user_id = $1 AND type = 'recharge' AND status = 'completed'",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(amount)
    }

    async fn get_wallet_estimated_days(&self, user_id: &str) -> Result<Option<f64>, DbError> {
        let thirty_days_ago =
            (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        let total_cost: f64 = sqlx::query_as::<_, (f64,)>(
            "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
             completion_tokens / 1000000.0 * completion_price + \
             cache_hit_input_tokens / 1000000.0 * cache_read_price), 0) \
             FROM usage_logs WHERE user_id = $1 AND timestamp >= $2",
        )
        .bind(user_id)
        .bind(&thirty_days_ago)
        .fetch_one(&self.pool)
        .await
        .map(|r| r.0)
        .unwrap_or(0.0);

        let balance: f64 = sqlx::query_as::<_, (f64,)>(
            "SELECT balance FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map(|r| r.0)
        .unwrap_or(0.0);

        let daily_avg = total_cost / 30.0;
        if daily_avg <= 0.0 {
            return Ok(None);
        }
        Ok(Some(balance / daily_avg))
    }

    // ── Recharge Keys ────────────────────────────────────────────────────

    async fn create_recharge_key(
        &self,
        key: &str,
        amount: f64,
        created_by: &str,
        expires_at: Option<&str>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO recharge_keys (key, amount, created_by, created_at, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(key)
        .bind(amount)
        .bind(created_by)
        .bind(&now)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn redeem_recharge_key(&self, key: &str, user_id: &str) -> Result<f64, DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;

        // Atomically mark as used — only if not already used/revoked
        let updated = sqlx::query(
            "UPDATE recharge_keys SET used_by = $1, used_at = $2 \
             WHERE key = $3 AND used_by IS NULL AND (revoked IS NULL OR revoked = false)",
        )
        .bind(user_id)
        .bind(&now)
        .bind(key)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            // Key doesn't exist or was already used/revoked — fetch details for error message
            let existing = sqlx::query(
                "SELECT used_by, revoked, expires_at FROM recharge_keys WHERE key = $1",
            )
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?;
            let msg = match existing {
                None => "Invalid recharge key".to_string(),
                Some(r) => {
                    let used_by: Option<String> = r.get(0);
                    let revoked: bool = r.get(1);
                    let expires_at: Option<String> = r.get(2);
                    if used_by.is_some() {
                        "Recharge key already used".to_string()
                    } else if revoked {
                        "Recharge key has been revoked".to_string()
                    } else if let Some(exp) = &expires_at {
                        if let Ok(exp_time) = chrono::DateTime::parse_from_rfc3339(exp) {
                            if chrono::Utc::now() > exp_time {
                                "Recharge key has expired".to_string()
                            } else {
                                "Invalid recharge key".to_string()
                            }
                        } else {
                            "Invalid recharge key".to_string()
                        }
                    } else {
                        "Invalid recharge key".to_string()
                    }
                }
            };
            return Err(DbError(msg));
        }

        // Get amount from the key
        let (amount,): (f64,) = sqlx::query_as(
            "SELECT amount FROM recharge_keys WHERE key = $1",
        )
        .bind(key)
        .fetch_one(&mut *tx)
        .await?;

        // Get current balance
        let (balance,): (f64,) = sqlx::query_as("SELECT balance FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| DbError("User not found".to_string()))?;

        let new_balance = balance + amount;
        sqlx::query("UPDATE users SET balance = $1 WHERE id = $2")
            .bind(new_balance)
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        // Record transaction
        sqlx::query(
            "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, \
             balance_after, method, status, note, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(user_id)
        .bind("recharge")
        .bind(amount)
        .bind(balance)
        .bind(new_balance)
        .bind("recharge_key")
        .bind("completed")
        .bind(format!("Key recharge: {}", key))
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(amount)
    }

    async fn revoke_recharge_key(&self, key: &str) -> Result<(), DbError> {
        let result = sqlx::query(
            "UPDATE recharge_keys SET revoked = true WHERE key = $1 \
             AND used_by IS NULL AND (revoked IS NULL OR revoked = false)",
        )
        .bind(key)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(DbError("Key not found or already used/revoked".to_string()));
        }
        Ok(())
    }

    async fn list_recharge_keys(&self) -> Result<Vec<RechargeKeyRow>, DbError> {
        let rows = sqlx::query(
            "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked \
             FROM recharge_keys ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RechargeKeyRow {
                key: r.get(0),
                amount: r.get(1),
                used_by: r.get(2),
                used_at: r.get(3),
                created_by: r.get(4),
                created_at: r.get(5),
                expires_at: r.get(6),
                revoked: r.get::<bool, _>(7),
            })
            .collect())
    }

    async fn list_recharge_keys_paginated(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<RechargeKeyRow>, DbError> {
        let rows = sqlx::query(
            "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked \
             FROM recharge_keys ORDER BY created_at DESC LIMIT $1 OFFSET $2",
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RechargeKeyRow {
                key: r.get(0),
                amount: r.get(1),
                used_by: r.get(2),
                used_at: r.get(3),
                created_by: r.get(4),
                created_at: r.get(5),
                expires_at: r.get(6),
                revoked: r.get::<bool, _>(7),
            })
            .collect())
    }

    async fn count_recharge_keys_filtered(
        &self,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<usize, DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut builder: QueryBuilder<'_, sqlx::Postgres> =
            QueryBuilder::new("SELECT COUNT(*) FROM recharge_keys WHERE 1=1");

        Self::apply_recharge_key_filters(&mut builder, search, status, user_search, &now);

        let (count,): (i64,) = builder.build_query_as().fetch_one(&self.pool).await?;
        Ok(count as usize)
    }

    async fn list_recharge_keys_filtered(
        &self,
        limit: usize,
        offset: usize,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<Vec<RechargeKeyRow>, DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(
            "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked \
             FROM recharge_keys WHERE 1=1",
        );

        Self::apply_recharge_key_filters(&mut builder, search, status, user_search, &now);

        builder.push(" ORDER BY created_at DESC LIMIT ");
        builder.push_bind(limit as i64);
        builder.push(" OFFSET ");
        builder.push_bind(offset as i64);

        let rows = builder.build().fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| RechargeKeyRow {
                key: r.get(0),
                amount: r.get(1),
                used_by: r.get(2),
                used_at: r.get(3),
                created_by: r.get(4),
                created_at: r.get(5),
                expires_at: r.get(6),
                revoked: r.get::<bool, _>(7),
            })
            .collect())
    }

    // ── Settings ─────────────────────────────────────────────────────────

    async fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let result: Option<(String,)> =
            sqlx::query_as("SELECT value FROM balancer_settings WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(result.map(|r| r.0))
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO balancer_settings (key, value) VALUES ($1, $2) \
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_gateway_config(&self) -> Result<GatewayRuntimeConfig, DbError> {
        match self.get_setting("gateway_config").await? {
            Some(json) => serde_json::from_str(&json)
                .map_err(|e| DbError(format!("Invalid gateway config JSON: {}", e))),
            None => Ok(GatewayRuntimeConfig::default()),
        }
    }

    async fn set_gateway_config(
        &self,
        config: &GatewayRuntimeConfig,
    ) -> Result<(), DbError> {
        let json = serde_json::to_string(config)
            .map_err(|e| DbError(format!("Failed to serialize gateway config: {}", e)))?;
        self.set_setting("gateway_config", &json).await
    }

    // ── Content Filter Rules ─────────────────────────────────────────

    async fn list_filter_rules(&self) -> Result<Vec<ContentFilterRule>, DbError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, String, String, Option<String>, Option<String>, bool, i32, String, String)>(
            "SELECT id, name, pattern_type, pattern, action, scope, channel_id, replacement, enabled, priority, created_at, updated_at FROM content_filter_rules ORDER BY priority ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to list filter rules: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|(id, name, pattern_type, pattern, action, scope, channel_id, replacement, enabled, priority, created_at, updated_at)| ContentFilterRule {
                id, name, pattern_type, pattern, action, scope, channel_id, replacement,
                enabled: enabled as bool,
                priority,
                created_at,
                updated_at,
            })
            .collect())
    }

    async fn create_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO content_filter_rules (id, name, pattern_type, pattern, action, scope, channel_id, replacement, enabled, priority, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"
        )
        .bind(&rule.id)
        .bind(&rule.name)
        .bind(&rule.pattern_type)
        .bind(&rule.pattern)
        .bind(&rule.action)
        .bind(&rule.scope)
        .bind(&rule.channel_id)
        .bind(&rule.replacement)
        .bind(rule.enabled)
        .bind(rule.priority)
        .bind(&rule.created_at)
        .bind(&rule.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to create filter rule: {}", e)))?;
        Ok(())
    }

    async fn update_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE content_filter_rules SET name=$1, pattern_type=$2, pattern=$3, action=$4, scope=$5, channel_id=$6, replacement=$7, enabled=$8, priority=$9, updated_at=$10 WHERE id=$11"
        )
        .bind(&rule.name)
        .bind(&rule.pattern_type)
        .bind(&rule.pattern)
        .bind(&rule.action)
        .bind(&rule.scope)
        .bind(&rule.channel_id)
        .bind(&rule.replacement)
        .bind(rule.enabled)
        .bind(rule.priority)
        .bind(&rule.updated_at)
        .bind(&rule.id)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to update filter rule: {}", e)))?;
        Ok(())
    }

    async fn delete_filter_rule(&self, id: &str) -> Result<(), DbError> {
        sqlx::query("DELETE FROM content_filter_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("Failed to delete filter rule: {}", e)))?;
        Ok(())
    }

    // ── Health Probe Results ─────────────────────────────────────────

    async fn insert_probe_result(&self, row: &ProbeResultRow) -> Result<(), DbError> {
        sqlx::query(
            "INSERT INTO probe_results (id, channel_id, model_id, success, latency_ms, error, probed_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&row.id)
        .bind(&row.channel_id)
        .bind(&row.model_id)
        .bind(row.success)
        .bind(row.latency_ms as i64)
        .bind(&row.error)
        .bind(&row.probed_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to insert probe result: {}", e)))?;
        Ok(())
    }

    async fn all_latest_probe_results(&self) -> Result<Vec<ProbeResultRow>, DbError> {
        let rows = sqlx::query_as::<_, (String, String, String, bool, i64, Option<String>, String)>(
            "SELECT p.id, p.channel_id, p.model_id, p.success, p.latency_ms, p.error, p.probed_at
             FROM probe_results p
             INNER JOIN (
                 SELECT channel_id, MAX(probed_at) AS max_ts
                 FROM probe_results
                 GROUP BY channel_id
             ) latest ON p.channel_id = latest.channel_id AND p.probed_at = latest.max_ts
             ORDER BY p.channel_id"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to list probe results: {}", e)))?;

        Ok(rows.into_iter().map(|(id, channel_id, model_id, success, latency_ms, error, probed_at)| ProbeResultRow {
            id, channel_id, model_id, success, latency_ms: latency_ms as u64, error, probed_at,
        }).collect())
    }

    async fn channel_usage_24h(&self) -> Result<Vec<(String, String, u64, u64, f64, f64)>, DbError> {
        let rows = sqlx::query_as::<_, (String, String, i64, i64, f64, f64)>(
            "SELECT channel_id, model, COUNT(*)::bigint, SUM(CASE WHEN success THEN 1 ELSE 0 END)::bigint, COALESCE(AVG(latency_ms)::float8, 0), COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms)::float8, 0)
             FROM usage_logs
             WHERE timestamp::timestamptz >= NOW() - INTERVAL '1 day'
             GROUP BY channel_id, model ORDER BY COUNT(*) DESC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to query channel usage: {}", e)))?;

        Ok(rows.into_iter().map(|(ch, m, req, suc, avg, p95)| {
            (ch, m, req as u64, suc as u64, avg, p95)
        }).collect())
    }

    async fn recent_request_paths(&self, limit: usize) -> Result<Vec<(String, String, String, Option<i64>, u64, bool)>, DbError> {
        let rows = sqlx::query_as::<_, (String, String, String, Option<i64>, i64, bool)>(
            "SELECT timestamp, model, channel_id, endpoint_id, latency_ms, success FROM usage_logs ORDER BY id DESC LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to query recent paths: {}", e)))?;

        Ok(rows.into_iter().map(|(ts, m, ch, eid, lat, suc)| {
            (ts, m, ch, eid, lat as u64, suc)
        }).collect())
    }

    async fn routing_flow_snapshot(&self, hours: u32) -> Result<Vec<(String, String, Option<i64>, u64)>, DbError> {
        use sqlx::Row;
        let since = (chrono::Utc::now() - chrono::Duration::hours(hours as i64)).format("%Y-%m-%dT%H:%M:%S").to_string();
        let rows = sqlx::query("SELECT model, channel_id, endpoint_id, COUNT(*)::bigint FROM usage_logs WHERE \"timestamp\"::timestamp >= $1::timestamp GROUP BY model, channel_id, endpoint_id")
            .bind(&since).fetch_all(&self.pool).await.map_err(|e| DbError(format!("routing_flow_snapshot: {}", e)))?;
        Ok(rows.iter().map(|r| (r.try_get::<String,_>(0).unwrap_or_default(), r.try_get::<String,_>(1).unwrap_or_default(), r.try_get::<Option<i64>,_>(2).unwrap_or(None), r.try_get::<i64,_>(3).unwrap_or(0) as u64)).collect())
    }

    async fn routing_history_buckets(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<super::RoutingHistoryBucket>, DbError> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT
                CASE WHEN (EXTRACT(EPOCH FROM $2::timestamp - $1::timestamp)) < 172800
                  THEN date_trunc('hour', \"timestamp\"::timestamp)::text
                  ELSE date_trunc('day',  \"timestamp\"::timestamp)::text
                END AS bucket,
                channel_id,
                COUNT(*)::bigint AS requests,
                SUM(CASE WHEN success THEN 1 ELSE 0 END)::bigint AS successes,
                AVG(latency_ms)::float8 AS avg_latency
             FROM usage_logs
             WHERE \"timestamp\"::timestamp >= $1::timestamp
               AND \"timestamp\"::timestamp <= $2::timestamp
               AND ($3::text IS NULL OR model = $3)
             GROUP BY bucket, channel_id
             ORDER BY bucket ASC",
        )
        .bind(start).bind(end).bind(model)
        .fetch_all(&self.pool).await
        .map_err(|e| DbError(format!("routing_history_buckets: {}", e)))?;
        Ok(rows.iter().map(|r| super::RoutingHistoryBucket {
            bucket: r.try_get::<String, _>(0).unwrap_or_default(),
            channel_id: r.try_get::<String, _>(1).unwrap_or_default(),
            endpoint_id: None,
            requests: r.try_get::<i64, _>(2).unwrap_or(0) as u64,
            successes: r.try_get::<i64, _>(3).unwrap_or(0) as u64,
            avg_latency: r.try_get::<f64, _>(4).unwrap_or(0.0),
        }).collect())
    }

    async fn routing_history_endpoint_stats(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<super::RoutingEndpointStat>, DbError> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT channel_id,
                    COUNT(*)::bigint AS requests,
                    SUM(CASE WHEN success THEN 1 ELSE 0 END)::bigint AS successes,
                    AVG(latency_ms)::float8 AS avg_latency,
                    COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms), 0)::float8 AS p95_latency
             FROM usage_logs
             WHERE \"timestamp\"::timestamp >= $1::timestamp
               AND \"timestamp\"::timestamp <= $2::timestamp
               AND ($3::text IS NULL OR model = $3)
             GROUP BY channel_id
             ORDER BY requests DESC",
        )
        .bind(start).bind(end).bind(model)
        .fetch_all(&self.pool).await
        .map_err(|e| DbError(format!("routing_history_endpoint_stats: {}", e)))?;
        Ok(rows.iter().map(|r| super::RoutingEndpointStat {
            channel_id: r.try_get::<String, _>(0).unwrap_or_default(),
            endpoint_id: None,
            requests: r.try_get::<i64, _>(1).unwrap_or(0) as u64,
            successes: r.try_get::<i64, _>(2).unwrap_or(0) as u64,
            avg_latency: r.try_get::<f64, _>(3).unwrap_or(0.0),
            p95_latency: r.try_get::<f64, _>(4).unwrap_or(0.0),
        }).collect())
    }

    async fn routing_history_endpoint_details(
        &self, start: &str, end: &str, model: Option<&str>,
    ) -> Result<Vec<(String, Option<i64>, Option<String>, u64, u64, f64, f64)>, DbError> {
        use sqlx::Row;
        let rows = sqlx::query(
            "SELECT ul.channel_id, ul.endpoint_id, e.url,
                    COUNT(*)::bigint, SUM(CASE WHEN ul.success THEN 1 ELSE 0 END)::bigint,
                    AVG(ul.latency_ms)::float8,
                    COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY ul.latency_ms),0)::float8
             FROM usage_logs ul LEFT JOIN endpoints e ON e.id=ul.endpoint_id
             WHERE \"ul\".\"timestamp\"::timestamp>=$1::timestamp AND \"ul\".\"timestamp\"::timestamp<=$2::timestamp
               AND ($3::text IS NULL OR ul.model=$3)
             GROUP BY ul.channel_id, ul.endpoint_id, e.url ORDER BY ul.channel_id, COUNT(*) DESC",
        ).bind(start).bind(end).bind(model).fetch_all(&self.pool).await
        .map_err(|e| DbError(format!("routing_history_endpoint_details: {}", e)))?;
        Ok(rows.iter().map(|r| (
            r.try_get::<String,_>(0).unwrap_or_default(),
            r.try_get::<Option<i64>,_>(1).unwrap_or(None),
            r.try_get::<Option<String>,_>(2).unwrap_or(None),
            r.try_get::<i64,_>(3).unwrap_or(0) as u64,
            r.try_get::<i64,_>(4).unwrap_or(0) as u64,
            r.try_get::<f64,_>(5).unwrap_or(0.0),
            r.try_get::<f64,_>(6).unwrap_or(0.0),
        )).collect())
    }

    async fn get_balances_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, f64, f64)>, DbError> {
        let rows = sqlx::query_as::<_, (String, f64, f64)>(
            "SELECT id, balance, frozen FROM users LIMIT $1 OFFSET $2",
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // ── Batch Operations ────────────────────────────────────────────────

    async fn batch_insert_usage_with_billing(
        &self,
        batch: &[UsageRecord],
        billing_enabled: bool,
    ) -> Result<Vec<(String, f64, f64)>, DbError> {
        let mut tx = self.pool.begin().await?;
        let mut deductions: Vec<(String, f64, f64)> = Vec::new();

        for record in batch {
            let (prompt_price, completion_price, cache_read_price) = {
                // Lookup pricing within transaction
                let result = sqlx::query_as::<_, (f64, f64, f64)>(
                    "SELECT prompt_price, completion_price, cache_read_price FROM models WHERE name = $1",
                )
                .bind(&record.model)
                .fetch_optional(&mut *tx)
                .await;

                match result {
                    Ok(Some(p)) => p,
                    _ => {
                        // Fallback to pattern matching
                        let rows = sqlx::query_as::<_, (f64, f64, f64, String)>(
                            "SELECT prompt_price, completion_price, cache_read_price, model_pattern FROM models",
                        )
                        .fetch_all(&mut *tx)
                        .await
                        .unwrap_or_default();

                        let mut found = (0.0, 0.0, 0.0);
                        for (p, c, cr, pattern) in rows {
                            if pattern.ends_with('*') {
                                let prefix = &pattern[..pattern.len() - 1];
                                if record.model.starts_with(prefix) {
                                    found = (p, c, cr);
                                    break;
                                }
                            }
                            if pattern == record.model {
                                found = (p, c, cr);
                                break;
                            }
                        }
                        found
                    }
                }
            };

            // Insert usage record with pricing snapshot
            sqlx::query(
                "INSERT INTO usage_logs (timestamp, request_id, user_id, user_name, channel_id, \
                 model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, \
                 success, request_body, response_body, reasoning_body, api_key_name, api_format, \
                 stream, cache_hit_input_tokens, prompt_price, completion_price, cache_read_price, client_ip) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
                 $16, $17, $18, $19, $20, $21, $22, $23)",
            )
            .bind(&record.timestamp)
            .bind(&record.request_id)
            .bind(&record.user_id)
            .bind(&record.user_name)
            .bind(&record.channel_id)
            .bind(&record.model)
            .bind(record.prompt_tokens as i64)
            .bind(record.completion_tokens as i64)
            .bind(record.total_tokens as i64)
            .bind(record.latency_ms as i64)
            .bind(record.status_code as i32)
            .bind(record.success)
            .bind(&record.request_body)
            .bind(&record.response_body)
            .bind(&record.reasoning_body)
            .bind(&record.api_key_name)
            .bind(&record.api_format)
            .bind(record.stream)
            .bind(record.cache_hit_input_tokens as i64)
            .bind(prompt_price)
            .bind(completion_price)
            .bind(cache_read_price)
            .bind(&record.client_ip)
            .execute(&mut *tx)
            .await?;

            if billing_enabled {
                let cost = record.prompt_tokens as f64 / 1000000.0 * prompt_price
                    + record.completion_tokens as f64 / 1000000.0 * completion_price
                    + record.cache_hit_input_tokens as f64 / 1000000.0 * cache_read_price;

                if cost > 0.0 {
                    let (balance, frozen): (f64, f64) = sqlx::query_as(
                        "SELECT balance, frozen FROM users WHERE id = $1",
                    )
                    .bind(&record.user_id)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap_or((0.0, 0.0));

                    let new_balance = balance - cost;
                    sqlx::query("UPDATE users SET balance = $1 WHERE id = $2")
                        .bind(new_balance)
                        .bind(&record.user_id)
                        .execute(&mut *tx)
                        .await?;

                    let now = chrono::Utc::now().to_rfc3339();
                    sqlx::query(
                        "INSERT INTO wallet_transactions (id, user_id, type, amount, \
                         balance_before, balance_after, method, status, note, created_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                    )
                    .bind(uuid::Uuid::new_v4().to_string())
                    .bind(&record.user_id)
                    .bind("deduction")
                    .bind(-cost)
                    .bind(balance)
                    .bind(new_balance)
                    .bind("usage")
                    .bind("completed")
                    .bind(format!("Usage: {}", record.model))
                    .bind(&now)
                    .execute(&mut *tx)
                    .await?;

                    deductions.push((record.user_id.clone(), new_balance, frozen));
                }
            }
        }

        tx.commit().await?;
        Ok(deductions)
    }
}

impl PgBackend {
    fn apply_recharge_key_filters<'a>(
        builder: &mut QueryBuilder<'a, sqlx::Postgres>,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
        now: &'a str,
    ) {
        if let Some(s) = search.filter(|s| !s.is_empty()) {
            builder.push(" AND key LIKE ");
            builder.push_bind(format!("%{}%", s));
        }
        if let Some(u) = user_search.filter(|u| !u.is_empty()) {
            builder.push(" AND (used_by LIKE ");
            builder.push_bind(format!("%{}%", u));
            builder.push(" OR created_by LIKE ");
            builder.push_bind(format!("%{}%", u));
            builder.push(")");
        }
        match status {
            Some("active") => {
                builder.push(" AND used_by IS NULL");
                builder.push(" AND (revoked IS NULL OR revoked = false)");
                builder.push(" AND (expires_at IS NULL OR expires_at > ");
                builder.push_bind(now);
                builder.push(")");
            }
            Some("used") => {
                builder.push(" AND used_by IS NOT NULL");
            }
            Some("expired") => {
                builder.push(" AND used_by IS NULL");
                builder.push(" AND (revoked IS NULL OR revoked = false)");
                builder.push(" AND expires_at IS NOT NULL");
                builder.push(" AND expires_at < ");
                builder.push_bind(now);
            }
            Some("revoked") => {
                builder.push(" AND revoked = true");
            }
            _ => {}
        }
    }
}
