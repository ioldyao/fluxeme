use async_trait::async_trait;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sqlx_core::{
    query::query, query_as::query_as, query_builder::QueryBuilder, query_scalar::query_scalar,
    raw_sql::raw_sql, row::Row,
};
use sqlx_postgres::{PgPool, PgRow, Postgres};

use crate::config::types::GatewayRuntimeConfig;
use crate::db::backend::DbBackend;
use crate::db::{AnnouncementRow, DbError, RechargeKeyRow, WalletTransactionRow};
use crate::domain::billing_group::{BillingGroupRow, BillingPaymentMode};
use crate::domain::channel::{Channel, Endpoint};
use crate::domain::model::{Model, ModelChannel, Pricing};
use crate::domain::moderation::ContentFilterRule;
use crate::domain::routing::RoutingRule;
use crate::domain::sso::SsoConfigRow;
use crate::domain::team::{Team, TeamMember};
use crate::domain::usage::UsageRecord;
use crate::domain::user::{ApiKey, User, USER_STATUS_ACTIVE, USER_STATUS_SUSPENDED};

pub struct PgBackend {
    pool: PgPool,
}

fn map_announcement_row(row: &PgRow) -> AnnouncementRow {
    AnnouncementRow {
        id: row.try_get::<String, _>(0).unwrap_or_default(),
        title: row.try_get::<String, _>(1).unwrap_or_default(),
        content: row.try_get::<String, _>(2).unwrap_or_default(),
        created_by: row.try_get::<String, _>(3).unwrap_or_default(),
        created_at: row.try_get::<String, _>(4).unwrap_or_default(),
        updated_at: row.try_get::<String, _>(5).unwrap_or_default(),
        published: row.try_get::<bool, _>(6).unwrap_or(false),
    }
}

fn map_billing_group_row(row: &PgRow) -> Result<BillingGroupRow, DbError> {
    Ok(BillingGroupRow {
        id: row.try_get(0)?,
        name: row.try_get(1)?,
        payment_mode: row.try_get::<String, _>(2)?.parse().map_err(DbError)?,
        status: row.try_get(3)?,
        is_default: row.try_get(4)?,
        created_by: row.try_get(5)?,
        created_at: row.try_get(6)?,
        updated_at: row.try_get(7)?,
        deleted_at: row.try_get(8).ok(),
        deleted_by: row.try_get(9).ok(),
    })
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
    async fn pricing_lookup(&self, model_name: &str) -> (Decimal, Decimal, Decimal, Decimal) {
        let result = query_as::<_, (f64, f64, f64, f64)>(
            "SELECT prompt_price, completion_price, cache_read_price, cache_write_price FROM models WHERE name = $1",
        )
        .bind(model_name)
        .fetch_optional(&self.pool)
        .await;

        match result {
            Ok(Some((p, c, cr, cw))) => (
                Decimal::try_from(p).unwrap_or(Decimal::ZERO),
                Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                Decimal::try_from(cr).unwrap_or(Decimal::ZERO),
                Decimal::try_from(cw).unwrap_or(Decimal::ZERO),
            ),
            _ => {
                // Fall back to pattern matching
                let rows = query_as::<_, (f64, f64, f64, f64, String)>(
                    "SELECT prompt_price, completion_price, cache_read_price, cache_write_price, model_pattern FROM models",
                )
                .fetch_all(&self.pool)
                .await;

                if let Ok(rows) = rows {
                    for (p, c, cr, cw, pattern) in rows {
                        if pattern.ends_with('*') {
                            let prefix = &pattern[..pattern.len() - 1];
                            if model_name.starts_with(prefix) {
                                return (
                                    Decimal::try_from(p).unwrap_or(Decimal::ZERO),
                                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                                    Decimal::try_from(cr).unwrap_or(Decimal::ZERO),
                                    Decimal::try_from(cw).unwrap_or(Decimal::ZERO),
                                );
                            }
                        }
                        if pattern == model_name {
                            return (
                                Decimal::try_from(p).unwrap_or(Decimal::ZERO),
                                Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                                Decimal::try_from(cr).unwrap_or(Decimal::ZERO),
                                Decimal::try_from(cw).unwrap_or(Decimal::ZERO),
                            );
                        }
                    }
                }
                (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO)
            }
        }
    }

    fn map_user_row(row: &PgRow, idx: &mut usize) -> User {
        let id: String = row.get(*idx);
        *idx += 1;
        let name: String = row.get(*idx);
        *idx += 1;
        let rpm: Option<i64> = row.get(*idx);
        *idx += 1;
        let tpm: Option<i64> = row.get(*idx);
        *idx += 1;
        let timezone: Option<String> = row.get(*idx);
        *idx += 1;
        let token_version: i64 = row.get(*idx);
        *idx += 1;
        let role_val: Option<String> = row.get(*idx);
        *idx += 1;
        let concurrency_val: i64 = row.get(*idx);
        *idx += 1;
        let currency: String = row.get(*idx);
        *idx += 1;
        let status: Option<String> = row.get(*idx);
        *idx += 1;
        let suspended_at: Option<String> = row.get(*idx);
        *idx += 1;
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
            status: status.unwrap_or_else(|| "active".to_string()),
            suspended_at,
        }
    }

    fn map_user_with_pw_row(row: &PgRow, idx: &mut usize) -> User {
        let id: String = row.get(*idx);
        *idx += 1;
        let name: String = row.get(*idx);
        *idx += 1;
        let password_hash: String = row.get(*idx);
        *idx += 1;
        let rpm: Option<i64> = row.get(*idx);
        *idx += 1;
        let tpm: Option<i64> = row.get(*idx);
        *idx += 1;
        let timezone: Option<String> = row.get(*idx);
        *idx += 1;
        let token_version: i64 = row.get(*idx);
        *idx += 1;
        let role_val: Option<String> = row.get(*idx);
        *idx += 1;
        let concurrency_val: i64 = row.get(*idx);
        *idx += 1;
        let currency: String = row.get(*idx);
        *idx += 1;
        let status: Option<String> = row.get(*idx);
        *idx += 1;
        let suspended_at: Option<String> = row.get(*idx);
        *idx += 1;
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
            status: status.unwrap_or_else(|| "active".to_string()),
            suspended_at,
        }
    }

    #[allow(dead_code)]
    fn map_usage_record(row: &PgRow, idx: &mut usize) -> UsageRecord {
        let timestamp: String = row.get(*idx);
        *idx += 1;
        let request_id: String = row.get(*idx);
        *idx += 1;
        let user_id: String = row.get(*idx);
        *idx += 1;
        let user_name: String = row.get(*idx);
        *idx += 1;
        let channel_id: String = row.get(*idx);
        *idx += 1;
        let model: String = row.get(*idx);
        *idx += 1;
        let prompt_tokens: i64 = row.get(*idx);
        *idx += 1;
        let completion_tokens: i64 = row.get(*idx);
        *idx += 1;
        let total_tokens: i64 = row.get(*idx);
        *idx += 1;
        let latency_ms: i64 = row.get(*idx);
        *idx += 1;
        let status_code: i32 = row.get(*idx);
        *idx += 1;
        let success: bool = row.get(*idx);
        *idx += 1;
        let api_key_name: Option<String> = row.get(*idx);
        *idx += 1;
        let api_format: String = row.get(*idx);
        *idx += 1;
        let stream: bool = row.get(*idx);
        *idx += 1;
        let cache_hit_input_tokens: i64 = row.get(*idx);
        *idx += 1;
        let cache_write_tokens: i64 = row.get(*idx);
        *idx += 1;
        let prompt_price: f64 = row.get(*idx);
        *idx += 1;
        let completion_price: f64 = row.get(*idx);
        *idx += 1;
        let cache_read_price: f64 = row.get(*idx);
        *idx += 1;
        let client_ip: Option<String> = row.get(*idx);
        *idx += 1;
        let original_model: String = if *idx < row.len() {
            row.get(*idx)
        } else {
            String::new()
        };
        *idx += 1;
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
            cache_write_tokens: cache_write_tokens as u64,
            prompt_price: Decimal::try_from(prompt_price).unwrap_or(Decimal::ZERO),
            completion_price: Decimal::try_from(completion_price).unwrap_or(Decimal::ZERO),
            cache_read_price: Decimal::try_from(cache_read_price).unwrap_or(Decimal::ZERO),
            cache_write_price: Decimal::ZERO,
            client_ip,
            endpoint_id: None,
            endpoint_url: None,
            original_model,
            team_id: None,
            ttft_ms: None,
            account_type: None,
            billing_payment_mode: None,
        }
    }

    #[allow(dead_code)]
    fn map_usage_with_bodies(row: &PgRow, idx: &mut usize) -> UsageRecord {
        let timestamp: String = row.get(*idx);
        *idx += 1;
        let request_id: String = row.get(*idx);
        *idx += 1;
        let user_id: String = row.get(*idx);
        *idx += 1;
        let user_name: String = row.get(*idx);
        *idx += 1;
        let channel_id: String = row.get(*idx);
        *idx += 1;
        let model: String = row.get(*idx);
        *idx += 1;
        let prompt_tokens: i64 = row.get(*idx);
        *idx += 1;
        let completion_tokens: i64 = row.get(*idx);
        *idx += 1;
        let total_tokens: i64 = row.get(*idx);
        *idx += 1;
        let latency_ms: i64 = row.get(*idx);
        *idx += 1;
        let status_code: i32 = row.get(*idx);
        *idx += 1;
        let success: bool = row.get(*idx);
        *idx += 1;
        let request_body: Option<String> = row.get(*idx);
        *idx += 1;
        let response_body: Option<String> = row.get(*idx);
        *idx += 1;
        let reasoning_body: Option<String> = row.get(*idx);
        *idx += 1;
        let api_key_name: Option<String> = row.get(*idx);
        *idx += 1;
        let api_format: String = row.get(*idx);
        *idx += 1;
        let stream: bool = row.get(*idx);
        *idx += 1;
        let cache_hit_input_tokens: i64 = row.get(*idx);
        *idx += 1;
        let cache_write_tokens: i64 = row.get(*idx);
        *idx += 1;
        let prompt_price: f64 = row.get(*idx);
        *idx += 1;
        let completion_price: f64 = row.get(*idx);
        *idx += 1;
        let cache_read_price: f64 = row.get(*idx);
        *idx += 1;
        let client_ip: Option<String> = row.get(*idx);
        *idx += 1;
        let original_model: String = if *idx < row.len() {
            row.get(*idx)
        } else {
            String::new()
        };
        *idx += 1;
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
            cache_write_tokens: cache_write_tokens as u64,
            prompt_price: Decimal::try_from(prompt_price).unwrap_or(Decimal::ZERO),
            completion_price: Decimal::try_from(completion_price).unwrap_or(Decimal::ZERO),
            cache_read_price: Decimal::try_from(cache_read_price).unwrap_or(Decimal::ZERO),
            cache_write_price: Decimal::ZERO,
            client_ip,
            endpoint_id: None,
            endpoint_url: None,
            original_model,
            team_id: None,
            ttft_ms: None,
            account_type: None,
            billing_payment_mode: None,
        }
    }
}

#[async_trait]
impl DbBackend for PgBackend {
    fn pg_pool(&self) -> &sqlx_postgres::PgPool {
        &self.pool
    }

    // ── Migration ────────────────────────────────────────────────────────

    async fn migrate(&self) -> Result<(), DbError> {
        raw_sql(
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

            -- Platform API Key scopes: api_key_id 引用 api_keys.key（sk_...）。
            -- 例：('key-001','skill','hpc3-slurm-query','invoke')
            CREATE TABLE IF NOT EXISTS api_key_scopes (
                id            TEXT PRIMARY KEY,
                api_key_id    TEXT NOT NULL REFERENCES api_keys(key) ON DELETE CASCADE,
                resource_type TEXT NOT NULL,
                resource_id   TEXT NOT NULL,
                action        TEXT NOT NULL,
                created_at    TEXT NOT NULL DEFAULT '',
                UNIQUE (api_key_id, resource_type, resource_id, action)
            );
            -- 存量库列类型转换（幂等）：id / created_at 均改 TEXT（对齐项目约定）
            ALTER TABLE api_key_scopes ALTER COLUMN id TYPE TEXT USING id::text;
            ALTER TABLE api_key_scopes ALTER COLUMN created_at TYPE TEXT USING created_at::text;

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
                id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
                name TEXT NOT NULL,
                scope TEXT NOT NULL DEFAULT 'system',
                user_id TEXT NOT NULL DEFAULT '*',
                source_model TEXT NOT NULL DEFAULT '*',
                target_model TEXT NOT NULL DEFAULT '',
                channel_id TEXT NOT NULL DEFAULT '',
                upstream_model TEXT NOT NULL DEFAULT '',
                priority INTEGER NOT NULL DEFAULT 0,
                enabled BOOLEAN NOT NULL DEFAULT true,
                description TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT NOW(),
                updated_at TEXT NOT NULL DEFAULT NOW()
            );
            -- Migrate old-format routing_rules to new schema (idempotent)
            -- Old schema: name PK, model_pattern, channel_id FK → new schema: id PK, name+new cols
            DO $$ BEGIN
                -- Drop legacy foreign key (user rules use empty channel_id)
                ALTER TABLE routing_rules DROP CONSTRAINT IF EXISTS routing_rules_channel_id_fkey;
                -- Drop legacy model_pattern column
                ALTER TABLE routing_rules DROP COLUMN IF EXISTS model_pattern;
                -- Add id column and make it the primary key
                ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS id TEXT;
                UPDATE routing_rules SET id = gen_random_uuid()::TEXT WHERE id IS NULL;
                ALTER TABLE routing_rules ALTER COLUMN id SET NOT NULL;
                -- Switch primary key from name to id (drop old PK, create new)
                ALTER TABLE routing_rules DROP CONSTRAINT IF EXISTS routing_rules_pkey;
                ALTER TABLE routing_rules ADD PRIMARY KEY (id);
                -- Add new columns
                ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS scope TEXT NOT NULL DEFAULT 'system';
                ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS source_model TEXT NOT NULL DEFAULT '*';
                ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS target_model TEXT NOT NULL DEFAULT '';
                ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS upstream_model TEXT NOT NULL DEFAULT '';
                ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS priority INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';
                ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS created_at TEXT NOT NULL DEFAULT NOW();
                ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS updated_at TEXT NOT NULL DEFAULT NOW();
                -- enabled column: new tables have BOOLEAN, old may have INTEGER
                IF EXISTS (SELECT 1 FROM information_schema.columns
                    WHERE table_name='routing_rules' AND column_name='enabled'
                    AND data_type='integer') THEN
                    ALTER TABLE routing_rules ALTER COLUMN enabled DROP DEFAULT;
                    ALTER TABLE routing_rules ALTER COLUMN enabled TYPE BOOLEAN USING (enabled::int::boolean);
                    ALTER TABLE routing_rules ALTER COLUMN enabled SET DEFAULT true;
                END IF;
            END $$;

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
                let _ = raw_sql($sql)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| DbError(format!("Migration alter error: {}", e)));
            };
        }

        add_col!(
            "ALTER TABLE models ADD COLUMN IF NOT EXISTS published BOOLEAN NOT NULL DEFAULT false"
        );
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
        add_col!(
            "ALTER TABLE endpoints ADD COLUMN IF NOT EXISTS enabled BOOLEAN NOT NULL DEFAULT true"
        );
        add_col!("ALTER TABLE models ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT ''");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS timezone TEXT NOT NULL DEFAULT 'UTC'");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS balance DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS frozen DOUBLE PRECISION NOT NULL DEFAULT 0.0");
        add_col!(
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS token_version BIGINT NOT NULL DEFAULT 0"
        );
        add_col!("ALTER TABLE channels ADD COLUMN IF NOT EXISTS anthropic_compat BOOLEAN NOT NULL DEFAULT false");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'user'");
        add_col!(
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active'"
        );
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS suspended_at TEXT");
        add_col!("ALTER TABLE recharge_keys ADD COLUMN IF NOT EXISTS expires_at TEXT");
        add_col!("ALTER TABLE recharge_keys ADD COLUMN IF NOT EXISTS revoked BOOLEAN NOT NULL DEFAULT false");
        add_col!("ALTER TABLE recharge_keys ADD COLUMN IF NOT EXISTS team_id TEXT REFERENCES teams(id) ON DELETE CASCADE");
        add_col!("ALTER TABLE model_channels ADD COLUMN IF NOT EXISTS upstream_model TEXT");

        // ── Billing groups ───────────────────────────────────────────────
        raw_sql(
            "CREATE TABLE IF NOT EXISTS billing_groups (\
                id TEXT PRIMARY KEY,\
                name TEXT NOT NULL,\
                payment_mode TEXT NOT NULL CHECK (payment_mode IN ('prepaid','postpaid')),\
                status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','inactive')),\
                is_default BOOLEAN NOT NULL DEFAULT false,\
                created_by TEXT NOT NULL DEFAULT '',\
                created_at TEXT NOT NULL,\
                updated_at TEXT NOT NULL\
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create billing_groups: {e}")))?;
        raw_sql(
            "INSERT INTO billing_groups (id, name, payment_mode, status, is_default, created_by, created_at, updated_at)\
             VALUES ('billing-group-default-prepaid', '默认按量计费', 'prepaid', 'active', true, '', now()::text, now()::text)\
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration seed default billing group: {e}")))?;
        add_col!("ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS billing_group_id TEXT");
        add_col!("ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS billing_payment_mode TEXT");
        add_col!("UPDATE api_keys SET billing_group_id = 'billing-group-default-prepaid' WHERE billing_group_id IS NULL OR billing_group_id = ''");
        add_col!("UPDATE api_keys SET billing_payment_mode = 'prepaid' WHERE billing_payment_mode IS NULL OR billing_payment_mode = ''");
        add_col!(
            "CREATE INDEX IF NOT EXISTS idx_api_keys_billing_group ON api_keys(billing_group_id)"
        );
        add_col!("ALTER TABLE billing_groups ADD COLUMN IF NOT EXISTS deleted_at TEXT");
        add_col!("ALTER TABLE billing_groups ADD COLUMN IF NOT EXISTS deleted_by TEXT");
        add_col!("ALTER TABLE billing_groups ADD COLUMN IF NOT EXISTS deletion_reason TEXT");
        add_col!("CREATE INDEX IF NOT EXISTS idx_billing_groups_status ON billing_groups(status, is_default)");
        add_col!("CREATE INDEX IF NOT EXISTS idx_token_reservations_group_state ON token_request_reservations(billing_group_id, state)");

        // Indexes
        macro_rules! add_idx {
            ($sql:expr) => {
                let _ = raw_sql($sql)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| DbError(format!("Migration index error: {}", e)));
            };
        }
        add_idx!("CREATE INDEX IF NOT EXISTS idx_wallet_tx_user ON wallet_transactions(user_id)");
        add_idx!(
            "CREATE INDEX IF NOT EXISTS idx_wallet_tx_created ON wallet_transactions(created_at)"
        );

        // Create content_filter_rules table
        let _ = raw_sql(
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

        // Set admin role for any user who was historically created as 'admin'
        let _ = raw_sql("UPDATE users SET role='admin' WHERE id='admin' AND role='user'")
            .execute(&self.pool)
            .await;

        // ── Deduplicate models by name ──────────────────────────────────
        // Step 1: merge duplicate rows (idempotent — safe to run repeatedly).
        let duplicates: Vec<(String, i64)> = query_as(
            "SELECT LOWER(name), count(*) FROM models GROUP BY LOWER(name) HAVING count(*) > 1",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        for (name_lower, _) in &duplicates {
            let winner: Option<(String, String)> = query_as(
                "SELECT id, name FROM models WHERE LOWER(name) = $1 \
                 ORDER BY (prompt_price + completion_price) DESC, id ASC LIMIT 1",
            )
            .bind(name_lower)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten();

            if let Some((ref winner_id, ref canonical_name)) = winner {
                let _ = query(
                    "INSERT INTO model_channels (model_id, channel_id, priority)
                     SELECT $1, mc.channel_id, mc.priority
                     FROM model_channels mc JOIN models m ON mc.model_id = m.id
                     WHERE LOWER(m.name) = $2 AND m.id != $1
                     ON CONFLICT (model_id, channel_id) DO NOTHING",
                )
                .bind(winner_id)
                .bind(name_lower)
                .execute(&self.pool)
                .await;

                let _ = query(
                    "INSERT INTO user_subscriptions (user_id, model_id, created_at)
                     SELECT us.user_id, $1, us.created_at
                     FROM user_subscriptions us JOIN models m ON us.model_id = m.id
                     WHERE LOWER(m.name) = $2 AND m.id != $1
                     ON CONFLICT (user_id, model_id) DO NOTHING",
                )
                .bind(winner_id)
                .bind(name_lower)
                .execute(&self.pool)
                .await;

                let _ = query("DELETE FROM models WHERE LOWER(name) = $1 AND id != $2")
                    .bind(name_lower)
                    .bind(winner_id)
                    .execute(&self.pool)
                    .await;

                let _ = query("UPDATE models SET name = $1 WHERE id = $2")
                    .bind(canonical_name)
                    .bind(winner_id)
                    .execute(&self.pool)
                    .await;

                tracing::info!(
                    "Migration: deduplicated model '{}' → kept id={}",
                    name_lower,
                    winner_id
                );
            }
        }

        // Step 2: verify. If duplicates remain after dedup, abort startup.
        let remaining: i64 = query_scalar(
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
        let result = raw_sql("ALTER TABLE models ADD CONSTRAINT models_name_unique UNIQUE (name)")
            .execute(&self.pool)
            .await;

        match result {
            Ok(_) => tracing::info!("models.name UNIQUE constraint created"),
            Err(e)
                if e.to_string().contains("already exists")
                    || e.to_string().contains("duplicate key value") =>
            {
                tracing::info!("models.name UNIQUE constraint already exists, skipping");
            }
            Err(e) => {
                tracing::error!(
                    "Failed to create models.name UNIQUE constraint: {}. \
                     This usually means duplicate rows exist.",
                    e
                );
                return Err(DbError(format!(
                    "Model name UNIQUE constraint creation failed: {}",
                    e
                )));
            }
        }

        tracing::info!("models.name UNIQUE constraint ready");

        // ── usage_billing table ──────────────────────────────────────────
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS usage_billing (\
                request_id TEXT PRIMARY KEY,\
                user_id TEXT NOT NULL,\
                user_name TEXT NOT NULL,\
                model TEXT NOT NULL,\
                channel_id TEXT NOT NULL,\
                prompt_tokens BIGINT NOT NULL,\
                completion_tokens BIGINT NOT NULL,\
                total_tokens BIGINT NOT NULL,\
                latency_ms BIGINT NOT NULL,\
                status_code INTEGER NOT NULL,\
                success BOOLEAN NOT NULL,\
                cache_hit_input_tokens BIGINT NOT NULL DEFAULT 0,\
                prompt_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
                completion_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
                cache_read_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
                cost_amount DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
                api_key_name TEXT,\
                api_format TEXT NOT NULL DEFAULT '',\
                stream BOOLEAN NOT NULL DEFAULT false,\
                client_ip TEXT,\
                endpoint_id BIGINT,\
                timestamp TEXT NOT NULL,\
                original_model TEXT NOT NULL DEFAULT '',\
                created_at TIMESTAMP NOT NULL DEFAULT NOW()\
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create usage_billing: {e}")))?;

        // Index for user-facing usage query
        let _ = raw_sql(
            "CREATE INDEX IF NOT EXISTS idx_usage_billing_user_time \
             ON usage_billing(user_id, timestamp)",
        )
        .execute(&self.pool)
        .await;

        // Backfill body columns on usage_billing (for usage detail page)
        add_col!("ALTER TABLE usage_billing ADD COLUMN IF NOT EXISTS request_body TEXT");
        add_col!("ALTER TABLE usage_billing ADD COLUMN IF NOT EXISTS response_body TEXT");
        add_col!("ALTER TABLE usage_billing ADD COLUMN IF NOT EXISTS reasoning_body TEXT");
        add_col!("ALTER TABLE usage_billing ADD COLUMN IF NOT EXISTS original_model TEXT NOT NULL DEFAULT ''");
        add_col!("ALTER TABLE usage_billing ADD COLUMN IF NOT EXISTS cache_write_tokens BIGINT NOT NULL DEFAULT 0");

        tracing::info!("usage_billing table ready");

        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS billing_events (\
                request_id TEXT PRIMARY KEY,\
                user_id TEXT NOT NULL,\
                user_name TEXT NOT NULL,\
                channel_id TEXT NOT NULL,\
                model TEXT NOT NULL,\
                prompt_tokens BIGINT NOT NULL,\
                completion_tokens BIGINT NOT NULL,\
                total_tokens BIGINT NOT NULL,\
                latency_ms BIGINT NOT NULL DEFAULT 0,\
                cache_hit_input_tokens BIGINT NOT NULL DEFAULT 0,\
                prompt_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
                completion_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
                cache_read_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
                cost_amount DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
                api_key_name TEXT,\
                api_format TEXT NOT NULL DEFAULT '',\
                stream BOOLEAN NOT NULL DEFAULT false,\
                client_ip TEXT,\
                endpoint_id BIGINT,\
                request_body TEXT,\
                response_body TEXT,\
                reasoning_body TEXT,\
                original_model TEXT NOT NULL DEFAULT '',\
                success BOOLEAN NOT NULL,\
                status_code INTEGER NOT NULL,\
                timestamp TEXT NOT NULL,\
                created_at TIMESTAMP NOT NULL DEFAULT NOW()\
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create billing_events: {e}")))?;
        for alter in [
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS latency_ms BIGINT NOT NULL DEFAULT 0",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS api_key_name TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS api_format TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS stream BOOLEAN NOT NULL DEFAULT false",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS client_ip TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS endpoint_id BIGINT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS request_body TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS response_body TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS reasoning_body TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS original_model TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS cache_write_tokens BIGINT NOT NULL DEFAULT 0",
        ] {
            let _ = raw_sql(alter).execute(&self.pool).await;
        }
        let _ = raw_sql(
            "INSERT INTO billing_events (\
                request_id, user_id, user_name, channel_id, model, \
                prompt_tokens, completion_tokens, total_tokens, latency_ms, cache_hit_input_tokens, \
                prompt_price, completion_price, cache_read_price, cost_amount, \
                api_key_name, api_format, stream, client_ip, endpoint_id, \
                request_body, response_body, reasoning_body, original_model, \
                success, status_code, timestamp\
             ) \
             SELECT request_id, user_id, user_name, channel_id, model, \
                prompt_tokens, completion_tokens, total_tokens, latency_ms, cache_hit_input_tokens, \
                prompt_price, completion_price, cache_read_price, cost_amount, \
                api_key_name, api_format, stream, client_ip, endpoint_id, \
                request_body, response_body, reasoning_body, original_model, \
                success, status_code, timestamp \
             FROM usage_billing \
             ON CONFLICT (request_id) DO NOTHING",
        )
        .execute(&self.pool)
        .await;
        let _ = raw_sql(
            "CREATE INDEX IF NOT EXISTS idx_billing_events_user_time \
             ON billing_events(user_id, timestamp)",
        )
        .execute(&self.pool)
        .await;
        tracing::info!("billing_events table ready");

        // ── Token resource packages / reservations ────────────────────────
        // Token package accounting is business data and therefore lives in PostgreSQL.
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS token_package_plans (
                id TEXT PRIMARY KEY,
                code TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                accounting_mode TEXT NOT NULL CHECK (accounting_mode IN ('raw_tokens','standardized_credits')),
                display_token_amount BIGINT NOT NULL CHECK (display_token_amount > 0),
                total_units BIGINT NOT NULL CHECK (total_units > 0),
                input_credit_factor DOUBLE PRECISION NOT NULL DEFAULT 1,
                output_credit_factor DOUBLE PRECISION NOT NULL DEFAULT 1,
                cache_credit_factor DOUBLE PRECISION NOT NULL DEFAULT 0,
                exhaustion_policy TEXT NOT NULL DEFAULT 'package_then_wallet' CHECK (exhaustion_policy IN ('package_then_wallet','package_only')),
                priority INTEGER NOT NULL DEFAULT 0,
                validity_days INTEGER,
                status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','inactive')),
                created_by TEXT NOT NULL REFERENCES users(id),
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create token_package_plans: {e}")))?;
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS token_package_model_factors (
                id TEXT PRIMARY KEY,
                plan_id TEXT NOT NULL REFERENCES token_package_plans(id) ON DELETE CASCADE,
                model_pattern TEXT NOT NULL,
                input_factor DOUBLE PRECISION NOT NULL DEFAULT 1,
                output_factor DOUBLE PRECISION NOT NULL DEFAULT 1,
                cache_factor DOUBLE PRECISION NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                UNIQUE (plan_id, model_pattern)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create token_package_model_factors: {e}")))?;
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS token_package_grants (
                id TEXT PRIMARY KEY,
                plan_id TEXT NOT NULL REFERENCES token_package_plans(id),
                user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
                team_id TEXT,
                accounting_mode TEXT NOT NULL CHECK (accounting_mode IN ('raw_tokens','standardized_credits')),
                display_token_amount BIGINT NOT NULL CHECK (display_token_amount > 0),
                total_units BIGINT NOT NULL CHECK (total_units > 0),
                consumed_units BIGINT NOT NULL DEFAULT 0 CHECK (consumed_units >= 0),
                reserved_units BIGINT NOT NULL DEFAULT 0 CHECK (reserved_units >= 0),
                priority INTEGER NOT NULL DEFAULT 0,
                exhaustion_policy TEXT NOT NULL DEFAULT 'package_then_wallet',
                status TEXT NOT NULL DEFAULT 'active',
                source TEXT NOT NULL DEFAULT 'admin_grant',
                note TEXT NOT NULL DEFAULT '',
                expires_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                CHECK ((user_id IS NOT NULL AND team_id IS NULL) OR (user_id IS NULL AND team_id IS NOT NULL))
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create token_package_grants: {e}")))?;
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS token_request_reservations (
                id TEXT PRIMARY KEY,
                request_id TEXT NOT NULL UNIQUE,
                request_fingerprint TEXT NOT NULL DEFAULT '',
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                user_name TEXT NOT NULL DEFAULT '',
                api_key_name TEXT NOT NULL DEFAULT '',
                team_id TEXT,
                account_type TEXT NOT NULL DEFAULT 'user',
                package_grant_id TEXT REFERENCES token_package_grants(id),
                model TEXT NOT NULL DEFAULT '',
                accounting_mode TEXT,
                reserved_prompt_tokens BIGINT NOT NULL DEFAULT 0,
                reserved_completion_tokens BIGINT NOT NULL DEFAULT 0,
                reserved_package_units BIGINT NOT NULL DEFAULT 0,
                reserved_total_units BIGINT NOT NULL DEFAULT 0,
                reserved_wallet_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
                actual_prompt_tokens BIGINT,
                actual_completion_tokens BIGINT,
                actual_package_units BIGINT,
                actual_wallet_amount DOUBLE PRECISION,
                factor_snapshot TEXT NOT NULL DEFAULT '{}',
                prompt_price DOUBLE PRECISION NOT NULL DEFAULT 0,
                completion_price DOUBLE PRECISION NOT NULL DEFAULT 0,
                cache_read_price DOUBLE PRECISION NOT NULL DEFAULT 0,
                cache_write_price DOUBLE PRECISION NOT NULL DEFAULT 0,
                state TEXT NOT NULL DEFAULT 'reserved',
                reason TEXT NOT NULL DEFAULT '',
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                settled_at TEXT
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create token_request_reservations: {e}")))?;
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS token_package_ledger (
                id TEXT PRIMARY KEY,
                package_grant_id TEXT NOT NULL REFERENCES token_package_grants(id),
                reservation_id TEXT REFERENCES token_request_reservations(id),
                request_id TEXT,
                user_id TEXT,
                team_id TEXT,
                entry_type TEXT NOT NULL,
                units BIGINT NOT NULL,
                display_tokens BIGINT NOT NULL DEFAULT 0,
                prompt_tokens BIGINT NOT NULL DEFAULT 0,
                completion_tokens BIGINT NOT NULL DEFAULT 0,
                credits BIGINT NOT NULL DEFAULT 0,
                model TEXT,
                note TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                UNIQUE (package_grant_id, request_id, entry_type)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create token_package_ledger: {e}")))?;
        for alter in [
            "ALTER TABLE users ADD COLUMN IF NOT EXISTS token_wallet_reserved DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE team_wallets ADD COLUMN IF NOT EXISTS token_wallet_reserved DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS reservation_id TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS package_grant_id TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS accounting_mode TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS package_units BIGINT NOT NULL DEFAULT 0",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS wallet_amount DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS activity_status TEXT NOT NULL DEFAULT 'unknown'",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS status_reason TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS charge_source TEXT NOT NULL DEFAULT 'unknown'",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS priced_cost_amount DOUBLE PRECISION NOT NULL DEFAULT 0",
            "UPDATE billing_events SET priced_cost_amount = cost_amount WHERE priced_cost_amount = 0 AND cost_amount <> 0",
            "UPDATE billing_events SET activity_status = CASE WHEN status_code = 499 THEN 'interrupted' WHEN success THEN 'success' ELSE 'failed' END WHERE activity_status = 'unknown' OR activity_status = ''",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS request_fingerprint TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS user_name TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS api_key_name TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS model TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS reserved_prompt_tokens BIGINT NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS reserved_completion_tokens BIGINT NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS reserved_total_units BIGINT NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS billing_group_id TEXT NOT NULL DEFAULT 'billing-group-default-prepaid'",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS billing_group_name TEXT NOT NULL DEFAULT '默认按量计费'",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS billing_payment_mode TEXT NOT NULL DEFAULT 'prepaid'",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS estimated_priced_cost_amount DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS prompt_price DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS completion_price DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS cache_read_price DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS cache_write_price DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS billing_group_id TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS billing_group_name TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS billing_payment_mode TEXT NOT NULL DEFAULT 'prepaid'",
            "ALTER TABLE token_package_plans ALTER COLUMN input_credit_factor TYPE DOUBLE PRECISION USING input_credit_factor::double precision",
            "ALTER TABLE token_package_plans ALTER COLUMN output_credit_factor TYPE DOUBLE PRECISION USING output_credit_factor::double precision",
            "ALTER TABLE token_package_plans ALTER COLUMN cache_credit_factor TYPE DOUBLE PRECISION USING cache_credit_factor::double precision",
            "ALTER TABLE token_package_model_factors ALTER COLUMN input_factor TYPE DOUBLE PRECISION USING input_factor::double precision",
            "ALTER TABLE token_package_model_factors ALTER COLUMN output_factor TYPE DOUBLE PRECISION USING output_factor::double precision",
            "ALTER TABLE token_package_model_factors ALTER COLUMN cache_factor TYPE DOUBLE PRECISION USING cache_factor::double precision",
        ] {
            let _ = raw_sql(alter).execute(&self.pool).await;
        }
        for index in [
            "CREATE INDEX IF NOT EXISTS idx_token_grants_user ON token_package_grants(user_id, status, expires_at)",
            "CREATE INDEX IF NOT EXISTS idx_token_grants_team ON token_package_grants(team_id, status, expires_at)",
            "CREATE INDEX IF NOT EXISTS idx_token_reservations_state ON token_request_reservations(state, expires_at)",
            "CREATE INDEX IF NOT EXISTS idx_token_ledger_grant ON token_package_ledger(package_grant_id, created_at)",
            "CREATE INDEX IF NOT EXISTS idx_billing_events_reservation ON billing_events(reservation_id)",
            "CREATE INDEX IF NOT EXISTS idx_billing_events_activity ON billing_events(user_id, timestamp DESC, activity_status, charge_source)",
        ] {
            let _ = raw_sql(index).execute(&self.pool).await;
        }
        tracing::info!("token package tables ready");

        // ── Announcements ─────────────────────────────────────────────────
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS announcements (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                created_by TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                published BOOLEAN NOT NULL DEFAULT false
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create announcements: {e}")))?;
        let _ = raw_sql(
            "CREATE INDEX IF NOT EXISTS idx_announcements_published \
             ON announcements(published, created_at DESC)",
        )
        .execute(&self.pool)
        .await;
        tracing::info!("announcements table ready");

        // ── Casbin authorization policies ────────────────────────────────
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS casbin_policies (
                id SERIAL PRIMARY KEY,
                ptype TEXT NOT NULL DEFAULT 'p',
                v0 TEXT NOT NULL DEFAULT '',
                v1 TEXT NOT NULL DEFAULT '',
                v2 TEXT NOT NULL DEFAULT '',
                v3 TEXT NOT NULL DEFAULT '',
                v4 TEXT NOT NULL DEFAULT '',
                v5 TEXT NOT NULL DEFAULT ''
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create casbin_policies: {e}")))?;
        let _ = raw_sql(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_casbin_policy_unique \
             ON casbin_policies(ptype, v0, v1, v2, v3, v4, v5)",
        )
        .execute(&self.pool)
        .await;
        tracing::info!("casbin_policies table ready");

        // ── Teams ─────────────────────────────────────────────────────────
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS teams (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
                updated_at TEXT NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create teams: {e}")))?;
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS team_members (
                team_id TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                role TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner','admin','member')),
                joined_at TEXT NOT NULL DEFAULT (now() AT TIME ZONE 'utc'),
                PRIMARY KEY (team_id, user_id)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create team_members: {e}")))?;
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS team_wallets (
                team_id TEXT PRIMARY KEY REFERENCES teams(id) ON DELETE CASCADE,
                balance DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                frozen DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                token_wallet_reserved DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                updated_at TEXT NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create team_wallets: {e}")))?;
        let _ =
            raw_sql("CREATE INDEX IF NOT EXISTS idx_team_members_user ON team_members(user_id)")
                .execute(&self.pool)
                .await;
        let _ =
            raw_sql("CREATE INDEX IF NOT EXISTS idx_team_members_team ON team_members(team_id)")
                .execute(&self.pool)
                .await;
        // Team-scoped existing tables: nullable team_id FK columns.
        let _ = raw_sql(
            "ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS team_id TEXT REFERENCES teams(id) ON DELETE CASCADE",
        )
        .execute(&self.pool)
        .await;
        let _ = raw_sql(
            "ALTER TABLE routing_rules ADD COLUMN IF NOT EXISTS team_id TEXT REFERENCES teams(id) ON DELETE CASCADE",
        )
        .execute(&self.pool)
        .await;
        let _ = raw_sql(
            "ALTER TABLE wallet_transactions ADD COLUMN IF NOT EXISTS team_id TEXT REFERENCES teams(id) ON DELETE CASCADE",
        )
        .execute(&self.pool)
        .await;
        let _ = raw_sql(
            "ALTER TABLE wallet_transactions ADD COLUMN IF NOT EXISTS account_type TEXT NOT NULL DEFAULT 'user' CHECK (account_type IN ('user','team'))",
        )
        .execute(&self.pool)
        .await;
        let _ = raw_sql(
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS team_id TEXT REFERENCES teams(id) ON DELETE CASCADE",
        )
        .execute(&self.pool)
        .await;
        let _ = raw_sql(
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS account_type TEXT NOT NULL DEFAULT 'user' CHECK (account_type IN ('user','team'))",
        )
        .execute(&self.pool)
        .await;
        let _ = raw_sql("CREATE INDEX IF NOT EXISTS idx_api_keys_team ON api_keys(team_id)")
            .execute(&self.pool)
            .await;
        let _ =
            raw_sql("CREATE INDEX IF NOT EXISTS idx_routing_rules_team ON routing_rules(team_id)")
                .execute(&self.pool)
                .await;
        let _ = raw_sql(
            "CREATE INDEX IF NOT EXISTS idx_wallet_transactions_team ON wallet_transactions(team_id)",
        )
        .execute(&self.pool)
        .await;
        let _ = raw_sql(
            "CREATE INDEX IF NOT EXISTS idx_billing_events_team ON billing_events(team_id)",
        )
        .execute(&self.pool)
        .await;
        tracing::info!("teams tables ready");

        // ── SSO configs ─────────────────────────────────────────────────
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS sso_configs (
                id TEXT PRIMARY KEY,
                team_id TEXT REFERENCES teams(id) ON DELETE CASCADE,
                provider_name TEXT NOT NULL DEFAULT 'SSO',
                issuer_url TEXT NOT NULL,
                client_id TEXT NOT NULL,
                client_secret_encrypted TEXT NOT NULL,
                redirect_url TEXT NOT NULL,
                enabled BOOLEAN NOT NULL DEFAULT true,
                auto_create_user BOOLEAN NOT NULL DEFAULT true,
                domain_restrictions TEXT,
                default_role TEXT NOT NULL DEFAULT 'user',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create sso_configs: {e}")))?;
        tracing::info!("sso_configs table ready");

        // ── SSO user organizations (from IdP, e.g. Keycloak Organizations) ─
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS sso_user_orgs (
                user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
                orgs TEXT NOT NULL DEFAULT '[]',
                updated_at TEXT NOT NULL DEFAULT (now() AT TIME ZONE 'utc')
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create sso_user_orgs: {e}")))?;
        tracing::info!("sso_user_orgs table ready");

        Ok(())
    }

    // ── Readiness ────────────────────────────────────────────────────────

    async fn ping(&self) -> Result<(), DbError> {
        query_scalar::<_, i32>("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError(format!("pg ping: {e}")))?;
        Ok(())
    }

    // ── Users ────────────────────────────────────────────────────────────

    async fn list_users(&self, status: Option<&str>) -> Result<Vec<User>, DbError> {
        let rows = if let Some(status) = status {
            query(
                "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE status = $1 ORDER BY id",
            )
            .bind(status)
            .fetch_all(&self.pool)
            .await?
        } else {
            query(
                "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users ORDER BY id",
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .iter()
            .map(|r| {
                let mut idx = 0usize;
                Self::map_user_row(r, &mut idx)
            })
            .collect())
    }

    async fn get_user(&self, id: &str) -> Result<Option<User>, DbError> {
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1",
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
        let rows = query(
            "SELECT id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1",
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
        let tz = if user.timezone.is_empty() {
            "UTC"
        } else {
            &user.timezone
        };
        let role = if user.role.is_empty() {
            "user"
        } else {
            &user.role
        };
        query(
            "INSERT INTO users (id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
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
        .bind(&user.status)
        .bind(&user.suspended_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn create_initial_admin(&self, user: &User) -> Result<(), DbError> {
        let (rpm, tpm) = user
            .rate_limits
            .as_ref()
            .map(|r| (r.rpm.map(|v| v as i64), r.tpm.map(|v| v as i64)))
            .unwrap_or((None, None));
        let pw_hash = user.password_hash.as_deref().unwrap_or("");
        let tz = if user.timezone.is_empty() {
            "UTC"
        } else {
            &user.timezone
        };
        let role = if user.role.is_empty() {
            "user"
        } else {
            &user.role
        };

        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;

        let (admin_count,): (i64,) = query_as("SELECT COUNT(*) FROM users WHERE role = 'admin'")
            .fetch_one(&mut *tx)
            .await?;
        if admin_count > 0 {
            return Err(DbError("Setup already completed".to_string()));
        }

        query(
            "INSERT INTO users (id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
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
        .bind(&user.status)
        .bind(&user.suspended_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    async fn update_user(&self, user: &User) -> Result<(), DbError> {
        let (rpm, tpm) = user
            .rate_limits
            .as_ref()
            .map(|r| (r.rpm.map(|v| v as i64), r.tpm.map(|v| v as i64)))
            .unwrap_or((None, None));
        let tz = if user.timezone.is_empty() {
            "UTC"
        } else {
            &user.timezone
        };
        if let Some(ref pw) = user.password_hash {
            query(
                "UPDATE users SET name = $1, password_hash = $2, rpm = $3, tpm = $4, timezone = $5, token_version = $6, role = $7, concurrency_limit = $8, currency = $9, status = $10, suspended_at = $11 WHERE id = $12",
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
            .bind(&user.status)
            .bind(&user.suspended_at)
            .bind(&user.id)
            .execute(&self.pool)
            .await?;
        } else {
            query(
                "UPDATE users SET name = $1, rpm = $2, tpm = $3, timezone = $4, token_version = $5, role = $6, concurrency_limit = $7, currency = $8, status = $9, suspended_at = $10 WHERE id = $11",
            )
            .bind(&user.name)
            .bind(rpm)
            .bind(tpm)
            .bind(tz)
            .bind(user.token_version)
            .bind(&user.role)
            .bind(user.concurrency_limit as i64)
            .bind(&user.currency)
            .bind(&user.status)
            .bind(&user.suspended_at)
            .bind(&user.id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn bump_user_token_version(&self, id: &str) -> Result<(), DbError> {
        let result = query("UPDATE users SET token_version = token_version + 1 WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DbError("User not found".to_string()));
        }
        Ok(())
    }

    async fn update_user_admin_fields(
        &self,
        id: &str,
        name: Option<String>,
        password_hash: Option<String>,
        rate_limits: Option<crate::domain::user::RateLimit>,
        role: Option<String>,
        concurrency_limit: Option<u32>,
    ) -> Result<User, DbError> {
        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let existing = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let current_user = Self::map_user_row(existing, &mut idx);

        let next_name = name.unwrap_or(current_user.name.clone());
        let next_role = role.unwrap_or(current_user.role.clone());
        let next_rate_limits = rate_limits.or(current_user.rate_limits.clone());
        let next_concurrency_limit = concurrency_limit.unwrap_or(current_user.concurrency_limit);

        if current_user.role == "admin"
            && current_user.status == USER_STATUS_ACTIVE
            && next_role != current_user.role
        {
            let (active_admin_count,): (i64,) =
                query_as("SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = $1")
                    .bind(USER_STATUS_ACTIVE)
                    .fetch_one(&mut *tx)
                    .await?;
            if active_admin_count <= 1 {
                return Err(DbError("Cannot demote the last active admin".to_string()));
            }
        }
        let next_token_version = if next_role != current_user.role || password_hash.is_some() {
            current_user.token_version + 1
        } else {
            current_user.token_version
        };
        let (rpm, tpm) = next_rate_limits
            .as_ref()
            .map(|r| (r.rpm.map(|v| v as i64), r.tpm.map(|v| v as i64)))
            .unwrap_or((None, None));
        let tz = if current_user.timezone.is_empty() {
            "UTC"
        } else {
            current_user.timezone.as_str()
        };

        let updated = if let Some(ref pw) = password_hash {
            let row = query(
                "UPDATE users SET name = $1, password_hash = $2, rpm = $3, tpm = $4, timezone = $5, token_version = $6, role = $7, concurrency_limit = $8, currency = $9, status = $10, suspended_at = $11 WHERE id = $12 RETURNING id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at",
            )
            .bind(&next_name)
            .bind(pw)
            .bind(rpm)
            .bind(tpm)
            .bind(tz)
            .bind(next_token_version)
            .bind(&next_role)
            .bind(next_concurrency_limit as i64)
            .bind(&current_user.currency)
            .bind(&current_user.status)
            .bind(&current_user.suspended_at)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            let mut idx = 0usize;
            Self::map_user_row(&row, &mut idx)
        } else {
            let row = query(
                "UPDATE users SET name = $1, rpm = $2, tpm = $3, timezone = $4, token_version = $5, role = $6, concurrency_limit = $7, currency = $8, status = $9, suspended_at = $10 WHERE id = $11 RETURNING id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at",
            )
            .bind(&next_name)
            .bind(rpm)
            .bind(tpm)
            .bind(tz)
            .bind(next_token_version)
            .bind(&next_role)
            .bind(next_concurrency_limit as i64)
            .bind(&current_user.currency)
            .bind(&current_user.status)
            .bind(&current_user.suspended_at)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            let mut idx = 0usize;
            Self::map_user_row(&row, &mut idx)
        };

        tx.commit().await?;
        Ok(updated)
    }

    async fn suspend_user(
        &self,
        id: &str,
        suspended_at: &chrono::DateTime<chrono::Utc>,
    ) -> Result<User, DbError> {
        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let existing = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let current_user = Self::map_user_row(existing, &mut idx);

        if current_user.status == USER_STATUS_SUSPENDED {
            return Ok(current_user);
        }

        if current_user.role == "admin" && current_user.status == USER_STATUS_ACTIVE {
            let (active_admin_count,): (i64,) =
                query_as("SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = $1")
                    .bind(USER_STATUS_ACTIVE)
                    .fetch_one(&mut *tx)
                    .await?;
            if active_admin_count <= 1 {
                return Err(DbError("Cannot suspend the last active admin".to_string()));
            }
        }

        let suspended_at_str = suspended_at.to_rfc3339();
        query(
            "UPDATE users SET status = $1, suspended_at = $2, token_version = token_version + 1 WHERE id = $3",
        )
        .bind(USER_STATUS_SUSPENDED)
        .bind(&suspended_at_str)
        .bind(id)
        .execute(&mut *tx)
        .await?;

        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let updated = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let user = Self::map_user_row(updated, &mut idx);
        tx.commit().await?;
        Ok(user)
    }

    async fn restore_user(&self, id: &str) -> Result<User, DbError> {
        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let existing = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let current_user = Self::map_user_row(existing, &mut idx);

        if current_user.status == USER_STATUS_ACTIVE {
            return Ok(current_user);
        }

        query(
            "UPDATE users SET status = $1, suspended_at = NULL, token_version = token_version + 1 WHERE id = $2",
        )
        .bind(USER_STATUS_ACTIVE)
        .bind(id)
        .execute(&mut *tx)
        .await?;

        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let updated = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let user = Self::map_user_row(updated, &mut idx);
        tx.commit().await?;
        Ok(user)
    }

    async fn delete_user(&self, id: &str) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        query("LOCK TABLE users IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
        let rows = query(
            "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency, status, suspended_at FROM users WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let existing = rows
            .first()
            .ok_or_else(|| DbError("User not found".to_string()))?;
        let mut idx = 0usize;
        let current_user = Self::map_user_row(existing, &mut idx);

        if current_user.role == "admin" && current_user.status == USER_STATUS_ACTIVE {
            let (active_admin_count,): (i64,) =
                query_as("SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = $1")
                    .bind(USER_STATUS_ACTIVE)
                    .fetch_one(&mut *tx)
                    .await?;
            if active_admin_count <= 1 {
                return Err(DbError("Cannot delete the last active admin".to_string()));
            }
        }

        query("DELETE FROM users WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn count_admins(&self, status: Option<&str>) -> Result<i64, DbError> {
        let (count,): (i64,) = if let Some(status) = status {
            query_as("SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = $1")
                .bind(status)
                .fetch_one(&self.pool)
                .await?
        } else {
            query_as("SELECT COUNT(*) FROM users WHERE role = 'admin'")
                .fetch_one(&self.pool)
                .await?
        };
        Ok(count)
    }

    async fn get_user_timezone(&self, id: &str) -> Result<String, DbError> {
        let result: Option<(String,)> = query_as("SELECT timezone FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(result.map(|r| r.0).unwrap_or_else(|| "UTC".to_string()))
    }

    async fn update_user_timezone(&self, id: &str, timezone: &str) -> Result<(), DbError> {
        let tz = if timezone.is_empty() { "UTC" } else { timezone };
        query("UPDATE users SET timezone = $1 WHERE id = $2")
            .bind(tz)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_user_currency(&self, id: &str) -> Result<String, DbError> {
        let rows = query_as::<_, (String,)>("SELECT currency FROM users WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(rows.map(|r| r.0).unwrap_or_else(|| "usd".to_string()))
    }

    async fn update_user_currency(&self, id: &str, currency: &str) -> Result<(), DbError> {
        let cur = if currency.is_empty() { "usd" } else { currency };
        query("UPDATE users SET currency = $1 WHERE id = $2")
            .bind(cur)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Billing groups ────────────────────────────────────────────────

    async fn list_billing_groups(
        &self,
        active_only: bool,
    ) -> Result<Vec<BillingGroupRow>, DbError> {
        let rows = if active_only {
            query("SELECT id, name, payment_mode, status, is_default, created_by, created_at, updated_at, deleted_at, deleted_by FROM billing_groups WHERE status = 'active' ORDER BY is_default DESC, name, id")
                .fetch_all(&self.pool)
                .await?
        } else {
            query("SELECT id, name, payment_mode, status, is_default, created_by, created_at, updated_at, deleted_at, deleted_by FROM billing_groups ORDER BY is_default DESC, name, id")
                .fetch_all(&self.pool)
                .await?
        };
        rows.iter().map(map_billing_group_row).collect()
    }

    async fn get_billing_group(&self, id: &str) -> Result<Option<BillingGroupRow>, DbError> {
        let row = query("SELECT id, name, payment_mode, status, is_default, created_by, created_at, updated_at, deleted_at, deleted_by FROM billing_groups WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(map_billing_group_row).transpose()
    }

    async fn create_billing_group(
        &self,
        id: &str,
        name: &str,
        payment_mode: BillingPaymentMode,
        created_by: &str,
    ) -> Result<BillingGroupRow, DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let row = query("INSERT INTO billing_groups (id, name, payment_mode, status, is_default, created_by, created_at, updated_at) VALUES ($1, $2, $3, 'active', false, $4, $5, $5) RETURNING id, name, payment_mode, status, is_default, created_by, created_at, updated_at, deleted_at, deleted_by")
            .bind(id)
            .bind(name)
            .bind(payment_mode.as_str())
            .bind(created_by)
            .bind(now)
            .fetch_one(&self.pool)
            .await?;
        map_billing_group_row(&row)
    }

    async fn set_billing_group_status(&self, id: &str, status: &str) -> Result<(), DbError> {
        if !matches!(status, "active" | "inactive") {
            return Err(DbError("invalid billing group status".to_string()));
        }
        let result = query("UPDATE billing_groups SET status = $1, updated_at = $2 WHERE id = $3 AND is_default = false AND deleted_at IS NULL")
            .bind(status)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(DbError("billing group not found or protected".to_string()));
        }
        Ok(())
    }

    async fn delete_billing_group(
        &self,
        id: &str,
        actor_id: &str,
        reason: &str,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let row =
            query("SELECT is_default, deleted_at FROM billing_groups WHERE id = $1 FOR UPDATE")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| DbError("billing group not found".to_string()))?;
        if row.try_get::<bool, _>(0)? {
            return Err(DbError("billing group protected".to_string()));
        }
        if row.try_get::<Option<String>, _>(1)?.is_some() {
            return Err(DbError("billing group already deleted".to_string()));
        }
        // Deleting a group immediately unbinds every API key from it. The key
        // remains present for audit/UI purposes, but its null billing group
        // makes subsequent reservations fail closed until an administrator
        // explicitly assigns a new active group.
        let key_count: i64 =
            query_scalar("SELECT COUNT(*) FROM api_keys WHERE billing_group_id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        query("UPDATE api_keys SET billing_group_id = NULL, billing_payment_mode = NULL WHERE billing_group_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let reservation_count: i64 = query_scalar("SELECT COUNT(*) FROM token_request_reservations WHERE billing_group_id = $1 AND state = 'reserved'")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
        let now = chrono::Utc::now().to_rfc3339();
        query("UPDATE billing_groups SET status = 'inactive', deleted_at = $1, deleted_by = $2, deletion_reason = $3, updated_at = $1 WHERE id = $4 AND is_default = false AND deleted_at IS NULL")
            .bind(&now)
            .bind(actor_id)
            .bind(reason)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        query("CREATE TABLE IF NOT EXISTS billing_group_audit_log (id TEXT PRIMARY KEY, billing_group_id TEXT NOT NULL, action TEXT NOT NULL, actor_id TEXT NOT NULL, occurred_at TEXT NOT NULL, reason TEXT NOT NULL, affected_api_key_count BIGINT NOT NULL, affected_reservation_count BIGINT NOT NULL)")
            .execute(&mut *tx).await?;
        query("INSERT INTO billing_group_audit_log (id, billing_group_id, action, actor_id, occurred_at, reason, affected_api_key_count, affected_reservation_count) VALUES ($1,$2,'delete',$3,$4,$5,$6,$7)")
            .bind(uuid::Uuid::new_v4().to_string()).bind(id).bind(actor_id).bind(&now).bind(reason).bind(key_count).bind(reservation_count)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── API Keys ─────────────────────────────────────────────────────────

    async fn list_api_keys(&self, user_id: &str) -> Result<Vec<ApiKey>, DbError> {
        // Personal key list only: team-scoped keys (team_id NOT NULL) are
        // managed via the team endpoints (list_team_api_keys).
        let rows = query(
            "SELECT key, user_id, name, enabled, expires_at, spend_limit, allowed_models, team_id, billing_group_id, billing_payment_mode FROM api_keys WHERE user_id = $1 AND team_id IS NULL ORDER BY key",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        let mut keys: Vec<ApiKey> = rows
            .iter()
            .map(|r| {
                let allowed_models_str: Option<String> = r.get(6);
                ApiKey {
                    key: r.get(0),
                    user_id: r.get(1),
                    name: r.get(2),
                    enabled: r.get(3),
                    expires_at: r.get(4),
                    spend_limit: r
                        .get::<Option<f64>, _>(5)
                        .map(|v| Decimal::try_from(v).unwrap_or(Decimal::ZERO)),
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                    team_id: r.get(7),
                    scopes: None,
                    billing_group_id: r.get::<Option<String>, _>(8).unwrap_or_default(),
                    billing_payment_mode: r
                        .get::<Option<String>, _>(9)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(BillingPaymentMode::Prepaid),
                }
            })
            .collect();
        // 填充每个 key 的访问范围（资源类型，来自 api_key_scopes 表）
        let scope_rows = query(
            "SELECT api_key_id, resource_type FROM api_key_scopes \
             WHERE action='invoke' AND api_key_id = ANY($1)",
        )
        .bind(keys.iter().map(|k| k.key.clone()).collect::<Vec<String>>())
        .fetch_all(&self.pool)
        .await?;
        let mut scope_map: std::collections::HashMap<String, Vec<String>> = Default::default();
        for row in scope_rows.iter() {
            scope_map.entry(row.get(0)).or_default().push(row.get(1));
        }
        for k in &mut keys {
            k.scopes = scope_map.get(&k.key).cloned();
        }
        Ok(keys)
    }

    async fn create_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
        query(
            "INSERT INTO api_keys (key, user_id, name, enabled, expires_at, spend_limit, allowed_models, team_id, billing_group_id, billing_payment_mode) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(&key.key)
        .bind(&key.user_id)
        .bind(&key.name)
        .bind(key.enabled)
        .bind(&key.expires_at)
        .bind(key.spend_limit.map(|v| v.to_f64().unwrap_or(0.0)))
        .bind(allowed)
        .bind(&key.team_id)
        .bind(&key.billing_group_id)
        .bind(key.billing_payment_mode.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_api_key(&self, key: &str) -> Result<(), DbError> {
        query("DELETE FROM api_keys WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
        query(
            "UPDATE api_keys SET name = $1, enabled = $2, expires_at = $3, spend_limit = $4, allowed_models = $5, team_id = $6, billing_group_id = $7, billing_payment_mode = $8 WHERE key = $9",
        )
        .bind(&key.name)
        .bind(key.enabled)
        .bind(&key.expires_at)
        .bind(key.spend_limit.map(|v| v.to_f64().unwrap_or(0.0)))
        .bind(allowed)
        .bind(&key.team_id)
        .bind(&key.billing_group_id)
        .bind(key.billing_payment_mode.as_str())
        .bind(&key.key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn lookup_key(&self, key: &str) -> Result<Option<(User, ApiKey)>, DbError> {
        let rows = query(
            "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, u.token_version, u.role, u.concurrency_limit, u.currency, u.status, u.suspended_at, \
             a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models, a.team_id, a.billing_group_id, a.billing_payment_mode \
             FROM api_keys a JOIN users u ON u.id = a.user_id WHERE a.key = $1",
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.first().map(|r| {
            let allowed_models_str: Option<String> = r.get(17);
            let api_key = ApiKey {
                key: r.get(11),
                user_id: r.get(12),
                name: r.get(13),
                enabled: r.get(14),
                expires_at: r.get(15),
                spend_limit: r
                    .get::<Option<f64>, _>(16)
                    .map(|v| Decimal::try_from(v).unwrap_or(Decimal::ZERO)),
                allowed_models: allowed_models_str
                    .filter(|s| !s.is_empty())
                    .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                team_id: r.get(18),
                scopes: None,
                billing_group_id: r.get::<Option<String>, _>(19).unwrap_or_default(),
                billing_payment_mode: r
                    .get::<Option<String>, _>(20)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(BillingPaymentMode::Prepaid),
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
                    status: r
                        .get::<Option<String>, _>(9)
                        .unwrap_or_else(|| USER_STATUS_ACTIVE.to_string()),
                    suspended_at: r.get(10),
                }
            };
            (user, api_key)
        }))
    }

    async fn all_api_keys(&self) -> Result<Vec<(User, ApiKey)>, DbError> {
        let rows = query(
            "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, u.token_version, u.role, u.concurrency_limit, u.currency, u.status, u.suspended_at, \
             a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models, a.team_id, a.billing_group_id, a.billing_payment_mode \
             FROM api_keys a JOIN users u ON u.id = a.user_id ORDER BY a.key",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let allowed_models_str: Option<String> = r.get(17);
                let api_key = ApiKey {
                    key: r.get(11),
                    user_id: r.get(12),
                    name: r.get(13),
                    enabled: r.get(14),
                    expires_at: r.get(15),
                    spend_limit: r
                        .get::<Option<f64>, _>(16)
                        .map(|v| Decimal::try_from(v).unwrap_or(Decimal::ZERO)),
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                    team_id: r.get(18),
                    scopes: None,
                    billing_group_id: r.get::<Option<String>, _>(19).unwrap_or_default(),
                    billing_payment_mode: r
                        .get::<Option<String>, _>(20)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(BillingPaymentMode::Prepaid),
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
                        status: r
                            .get::<Option<String>, _>(9)
                            .unwrap_or_else(|| USER_STATUS_ACTIVE.to_string()),
                        suspended_at: r.get(10),
                    }
                };
                (user, api_key)
            })
            .collect())
    }

    // ── API Key Scopes（Platform API Key） ─────────────────────────────

    async fn add_api_key_scope(
        &self,
        api_key_id: &str,
        resource_type: &str,
        resource_id: &str,
        action: &str,
    ) -> Result<(), DbError> {
        query(
            "INSERT INTO api_key_scopes (id, api_key_id, resource_type, resource_id, action, created_at) \
             VALUES ($1,$2,$3,$4,$5,$6) \
             ON CONFLICT (api_key_id, resource_type, resource_id, action) DO NOTHING",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(api_key_id)
        .bind(resource_type)
        .bind(resource_id)
        .bind(action)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn api_key_has_resource_scope(
        &self,
        api_key_id: &str,
        resource_type: &str,
        resource_id: &str,
        action: &str,
    ) -> Result<bool, DbError> {
        let row = query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM api_key_scopes \
             WHERE api_key_id=$1 AND resource_type=$2 AND action=$3 \
             AND (resource_id='*' OR resource_id=$4)",
        )
        .bind(api_key_id)
        .bind(resource_type)
        .bind(action)
        .bind(resource_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row > 0)
    }

    // ── Channels & Endpoints ─────────────────────────────────────────────

    async fn list_channels(&self) -> Result<Vec<Channel>, DbError> {
        let ch_rows = query(
            "SELECT id, name, provider, priority, enabled, anthropic_compat FROM channels ORDER BY priority, id",
        )
        .fetch_all(&self.pool)
        .await?;

        let ep_rows = query(
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
        let rows = query(
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
            let eps = query(
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
        query(
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
            query(
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
        query(
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
        query("DELETE FROM endpoints WHERE channel_id = $1")
            .bind(&ch.id)
            .execute(&self.pool)
            .await?;
        for ep in &ch.endpoints {
            query(
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
        query("DELETE FROM channels WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_endpoint(&self, id: i64) -> Result<Option<Endpoint>, DbError> {
        let rows = query(
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
        query("UPDATE endpoints SET enabled = $1 WHERE id = $2")
            .bind(enabled)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn update_endpoint_api_key(&self, id: i64, api_key: &str) -> Result<(), DbError> {
        query("UPDATE endpoints SET api_key = $1 WHERE id = $2")
            .bind(api_key)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Models ───────────────────────────────────────────────────────────

    async fn list_models(&self) -> Result<Vec<Model>, DbError> {
        let m_rows = query(
            "SELECT id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category FROM models ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;

        let b_rows = query(
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
                    prompt_price: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                    completion_price: Decimal::try_from(r.get::<f64, _>(4))
                        .unwrap_or(Decimal::ZERO),
                    cache_read_price: Decimal::try_from(r.get::<f64, _>(5))
                        .unwrap_or(Decimal::ZERO),
                    cache_write_price: Decimal::try_from(r.get::<f64, _>(6))
                        .unwrap_or(Decimal::ZERO),
                    image_input_price: Decimal::try_from(r.get::<f64, _>(7))
                        .unwrap_or(Decimal::ZERO),
                    audio_input_price: Decimal::try_from(r.get::<f64, _>(8))
                        .unwrap_or(Decimal::ZERO),
                    audio_output_price: Decimal::try_from(r.get::<f64, _>(9))
                        .unwrap_or(Decimal::ZERO),
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
        let rows = query(
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
                    prompt_price: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                    completion_price: Decimal::try_from(r.get::<f64, _>(4))
                        .unwrap_or(Decimal::ZERO),
                    cache_read_price: Decimal::try_from(r.get::<f64, _>(5))
                        .unwrap_or(Decimal::ZERO),
                    cache_write_price: Decimal::try_from(r.get::<f64, _>(6))
                        .unwrap_or(Decimal::ZERO),
                    image_input_price: Decimal::try_from(r.get::<f64, _>(7))
                        .unwrap_or(Decimal::ZERO),
                    audio_input_price: Decimal::try_from(r.get::<f64, _>(8))
                        .unwrap_or(Decimal::ZERO),
                    audio_output_price: Decimal::try_from(r.get::<f64, _>(9))
                        .unwrap_or(Decimal::ZERO),
                },
                channels: Vec::new(),
                published: r.get::<bool, _>(10),
                context_length: r.get(11),
                category: r.get::<Option<String>, _>(12).unwrap_or_default(),
            };
            let bindings = query(
                "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, ''), mc.upstream_model \
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
        let mut tx = self.pool.begin().await?;
        query(
            "INSERT INTO models (id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(&m.id)
        .bind(&m.name)
        .bind(&m.model_pattern)
        .bind(m.pricing.prompt_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.completion_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.cache_read_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.cache_write_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.image_input_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.audio_input_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.audio_output_price.to_f64().unwrap_or(0.0))
        .bind(m.published)
        .bind(m.context_length)
        .bind(&m.category)
        .execute(&mut *tx)
        .await?;

        for binding in &m.channels {
            query(
                "INSERT INTO model_channels (model_id, channel_id, priority, upstream_model) \
                 VALUES ($1, $2, $3, $4) ON CONFLICT (model_id, channel_id) DO UPDATE SET priority = EXCLUDED.priority, upstream_model = EXCLUDED.upstream_model",
            )
            .bind(&m.id)
            .bind(&binding.channel_id)
            .bind(binding.priority)
            .bind(&binding.upstream_model)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn update_model(&self, old_id: &str, m: &Model) -> Result<(), DbError> {
        if old_id != m.id {
            return Err(DbError("Model ID cannot be changed".to_string()));
        }
        let mut tx = self.pool.begin().await?;
        query(
            "UPDATE models SET name=$1, model_pattern=$2, prompt_price=$3, completion_price=$4, \
             cache_read_price=$5, cache_write_price=$6, image_input_price=$7, audio_input_price=$8, \
             audio_output_price=$9, published=$10, context_length=$11, category=$12 WHERE id=$13",
        )
        .bind(&m.name)
        .bind(&m.model_pattern)
        .bind(m.pricing.prompt_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.completion_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.cache_read_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.cache_write_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.image_input_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.audio_input_price.to_f64().unwrap_or(0.0))
        .bind(m.pricing.audio_output_price.to_f64().unwrap_or(0.0))
        .bind(m.published)
        .bind(m.context_length)
        .bind(&m.category)
        .bind(old_id)
        .execute(&mut *tx)
        .await?;
        // Delete old bindings by old_id (model_channels FK references old model id)
        query("DELETE FROM model_channels WHERE model_id = $1")
            .bind(old_id)
            .execute(&mut *tx)
            .await?;
        for binding in &m.channels {
            query(
                "INSERT INTO model_channels (model_id, channel_id, priority, upstream_model) VALUES ($1, $2, $3, $4)",
            )
            .bind(&m.id)
            .bind(&binding.channel_id)
            .bind(binding.priority)
            .bind(&binding.upstream_model)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn delete_model(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM models WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_published_models(&self) -> Result<Vec<Model>, DbError> {
        let m_rows = query(
            "SELECT id, name, model_pattern, prompt_price, completion_price, \
             cache_read_price, cache_write_price, image_input_price, audio_input_price, \
             audio_output_price, published, context_length, category FROM models \
             WHERE published = true ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;

        let b_rows = query(
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
                    prompt_price: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                    completion_price: Decimal::try_from(r.get::<f64, _>(4))
                        .unwrap_or(Decimal::ZERO),
                    cache_read_price: Decimal::try_from(r.get::<f64, _>(5))
                        .unwrap_or(Decimal::ZERO),
                    cache_write_price: Decimal::try_from(r.get::<f64, _>(6))
                        .unwrap_or(Decimal::ZERO),
                    image_input_price: Decimal::try_from(r.get::<f64, _>(7))
                        .unwrap_or(Decimal::ZERO),
                    audio_input_price: Decimal::try_from(r.get::<f64, _>(8))
                        .unwrap_or(Decimal::ZERO),
                    audio_output_price: Decimal::try_from(r.get::<f64, _>(9))
                        .unwrap_or(Decimal::ZERO),
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
        query("UPDATE models SET published = $1 WHERE id = $2")
            .bind(published)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_model_pricing(&self, id: &str, pricing: &Pricing) -> Result<(), DbError> {
        query(
            "UPDATE models SET prompt_price=$1, completion_price=$2, cache_read_price=$3, \
             cache_write_price=$4, image_input_price=$5, audio_input_price=$6, \
             audio_output_price=$7 WHERE id=$8",
        )
        .bind(pricing.prompt_price.to_f64().unwrap_or(0.0))
        .bind(pricing.completion_price.to_f64().unwrap_or(0.0))
        .bind(pricing.cache_read_price.to_f64().unwrap_or(0.0))
        .bind(pricing.cache_write_price.to_f64().unwrap_or(0.0))
        .bind(pricing.image_input_price.to_f64().unwrap_or(0.0))
        .bind(pricing.audio_input_price.to_f64().unwrap_or(0.0))
        .bind(pricing.audio_output_price.to_f64().unwrap_or(0.0))
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn set_model_context_length(&self, id: &str, context_length: i64) -> Result<(), DbError> {
        query("UPDATE models SET context_length = $1 WHERE id = $2")
            .bind(context_length)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ── Routing Rules ────────────────────────────────────────────────────

    async fn list_rules(&self) -> Result<Vec<RoutingRule>, DbError> {
        let rows = query(
            "SELECT id, name, scope, user_id, source_model, target_model, \
             channel_id, upstream_model, priority, enabled, description, \
             created_at, updated_at, team_id \
             FROM routing_rules ORDER BY priority, name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RoutingRule {
                id: r.get(0),
                name: r.get(1),
                scope: r.get(2),
                user_id: r.get(3),
                source_model: r.get(4),
                target_model: r.get(5),
                channel_id: r.get(6),
                upstream_model: r.get(7),
                priority: r.get(8),
                enabled: r.get(9),
                description: r.get(10),
                created_at: r.get(11),
                updated_at: r.get(12),
                team_id: r.get(13),
            })
            .collect())
    }

    async fn create_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        let id = if r.id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            r.id.clone()
        };
        query(
            "INSERT INTO routing_rules \
             (id, name, scope, user_id, source_model, target_model, channel_id, \
              upstream_model, priority, enabled, description, created_at, updated_at, team_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        )
        .bind(&id)
        .bind(&r.name)
        .bind(&r.scope)
        .bind(&r.user_id)
        .bind(&r.source_model)
        .bind(&r.target_model)
        .bind(&r.channel_id)
        .bind(&r.upstream_model)
        .bind(r.priority)
        .bind(r.enabled)
        .bind(&r.description)
        .bind(&r.created_at)
        .bind(&r.updated_at)
        .bind(&r.team_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        query(
            "UPDATE routing_rules SET name=$1, scope=$2, user_id=$3, source_model=$4, \
             target_model=$5, channel_id=$6, upstream_model=$7, priority=$8, enabled=$9, \
             description=$10, updated_at=$11, team_id=$12 WHERE id=$13",
        )
        .bind(&r.name)
        .bind(&r.scope)
        .bind(&r.user_id)
        .bind(&r.source_model)
        .bind(&r.target_model)
        .bind(&r.channel_id)
        .bind(&r.upstream_model)
        .bind(r.priority)
        .bind(r.enabled)
        .bind(&r.description)
        .bind(&r.updated_at)
        .bind(&r.team_id)
        .bind(&r.id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_rule(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM routing_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_user_rules(&self, user_id: &str) -> Result<Vec<RoutingRule>, DbError> {
        let rows = query(
            "SELECT id, name, scope, user_id, source_model, target_model, \
             channel_id, upstream_model, priority, enabled, description, \
             created_at, updated_at, team_id \
             FROM routing_rules WHERE scope='user' AND user_id=$1 \
             ORDER BY priority, name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RoutingRule {
                id: r.get(0),
                name: r.get(1),
                scope: r.get(2),
                user_id: r.get(3),
                source_model: r.get(4),
                target_model: r.get(5),
                channel_id: r.get(6),
                upstream_model: r.get(7),
                priority: r.get(8),
                enabled: r.get(9),
                description: r.get(10),
                created_at: r.get(11),
                updated_at: r.get(12),
                team_id: r.get(13),
            })
            .collect())
    }

    // ── Usage Logs ───────────────────────────────────────────────────────

    async fn period_summary(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64), DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (cost, count, tokens): (f64, i64, i64) = if let Some(uid) = user_id {
            query_as(
                "SELECT COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_as(
                "SELECT COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
            )
            .bind(&start)
            .bind(&end)
            .fetch_one(&self.pool)
            .await?
        };
        Ok((
            Decimal::try_from(cost).unwrap_or(Decimal::ZERO),
            count as u64,
            tokens as u64,
        ))
    }

    async fn period_summary_since(
        &self,
        start: &str,
        user_id: Option<&str>,
    ) -> Result<Decimal, DbError> {
        let cost: f64 = if let Some(uid) = user_id {
            query_scalar("SELECT COALESCE(SUM(cost_amount), 0) FROM billing_events WHERE timestamp >= $1 AND user_id = $2")
                .bind(start)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
        } else {
            query_scalar(
                "SELECT COALESCE(SUM(cost_amount), 0) FROM billing_events WHERE timestamp >= $1",
            )
            .bind(start)
            .fetch_one(&self.pool)
            .await?
        };
        Ok(Decimal::try_from(cost).unwrap_or(Decimal::ZERO))
    }

    async fn billing_event_modes(
        &self,
        request_ids: &[String],
    ) -> Result<std::collections::HashMap<String, (String, Option<String>)>, DbError> {
        if request_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let rows = query("SELECT request_id, COALESCE(billing_payment_mode, 'prepaid'), billing_group_name FROM billing_events WHERE request_id = ANY($1)")
            .bind(request_ids)
            .fetch_all(&self.pool)
            .await?;
        let mut result = std::collections::HashMap::new();
        for row in rows {
            result.insert(row.try_get(0)?, (row.try_get(1)?, row.try_get(2)?));
        }
        Ok(result)
    }

    async fn list_billing_activities(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
        filter: &crate::db::BillingActivityFilter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<crate::db::BillingActivityRow>, DbError> {
        let mut builder = QueryBuilder::<Postgres>::new(
            r#"SELECT timestamp, request_id, user_id, user_name, model, channel_id,
             CASE WHEN status_code = 499 THEN 'interrupted' WHEN status_code >= 400 OR success = false THEN 'failed'
             WHEN activity_status IN ('unknown', '') THEN CASE WHEN cost_amount = 0
             AND package_units = 0 THEN 'zero_cost' ELSE 'success' END ELSE activity_status END,
             COALESCE(status_reason, ''), status_code, success, prompt_tokens, completion_tokens,
             cache_hit_input_tokens, cache_write_tokens, total_tokens, package_units, package_grant_id,
             COALESCE(wallet_amount, 0), COALESCE(NULLIF(priced_cost_amount, 0), cost_amount, 0),
             CASE WHEN billing_payment_mode = 'postpaid' AND package_units > 0 THEN 'postpaid_package'
             WHEN billing_payment_mode = 'postpaid' THEN 'postpaid'
             WHEN charge_source IN ('package_and_wallet', 'package', 'wallet', 'free_model', 'none') THEN charge_source
             WHEN package_units > 0 AND wallet_amount > 0 THEN 'package_and_wallet'
             WHEN package_units > 0 THEN 'package' WHEN wallet_amount > 0 THEN 'wallet'
             WHEN status_code >= 400 OR success = false THEN 'none' ELSE 'none' END,
             account_type, team_id, api_key_name, latency_ms, reservation_id,
             billing_group_id, billing_group_name, COALESCE(billing_payment_mode, 'prepaid')
             FROM billing_events WHERE timestamp >= "#,
        );
        builder
            .push_bind(start)
            .push(" AND timestamp < ")
            .push_bind(end);
        if let Some(uid) = user_id {
            builder.push(" AND user_id = ").push_bind(uid);
        }
        if let Some(search) = &filter.search {
            let pattern = format!("%{}%", search);
            builder
                .push(" AND (request_id ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR COALESCE(api_key_name, '未命名 Key') ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR model ILIKE ")
                .push_bind(pattern.clone())
                .push(")");
        }
        if let Some(api_key_name) = &filter.api_key_name {
            builder
                .push(" AND COALESCE(api_key_name, '未命名 Key') = ")
                .push_bind(api_key_name);
        }
        if let Some(model) = &filter.model {
            builder.push(" AND model = ").push_bind(model);
        }
        if let Some(source) = &filter.charge_source {
            builder.push(" AND (CASE WHEN billing_payment_mode = 'postpaid' AND package_units > 0 THEN 'postpaid_package' WHEN billing_payment_mode = 'postpaid' THEN 'postpaid' WHEN charge_source IN ('package_and_wallet', 'package', 'wallet', 'free_model', 'none') THEN charge_source WHEN package_units > 0 AND wallet_amount > 0 THEN 'package_and_wallet' WHEN package_units > 0 THEN 'package' WHEN wallet_amount > 0 THEN 'wallet' WHEN status_code >= 400 OR success = false THEN 'none' ELSE 'none' END) = ").push_bind(source);
        }
        builder
            .push(" ORDER BY timestamp DESC LIMIT ")
            .push_bind(limit as i64)
            .push(" OFFSET ")
            .push_bind(offset as i64);
        let rows = builder.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(crate::db::BillingActivityRow {
                    timestamp: row.try_get(0)?,
                    request_id: row.try_get(1)?,
                    user_id: row.try_get(2)?,
                    user_name: row.try_get(3)?,
                    model: row.try_get(4)?,
                    channel_id: row.try_get(5)?,
                    activity_status: row.try_get(6)?,
                    status_reason: row.try_get(7)?,
                    status_code: row.try_get::<i32, _>(8)? as u16,
                    success: row.try_get(9)?,
                    prompt_tokens: row.try_get::<i64, _>(10)?.max(0) as u64,
                    completion_tokens: row.try_get::<i64, _>(11)?.max(0) as u64,
                    cache_hit_input_tokens: row.try_get::<i64, _>(12)?.max(0) as u64,
                    cache_write_tokens: row.try_get::<i64, _>(13)?.max(0) as u64,
                    total_tokens: row.try_get::<i64, _>(14)?.max(0) as u64,
                    package_units: row.try_get::<i64, _>(15)?.max(0) as u64,
                    package_grant_id: row.try_get(16)?,
                    wallet_amount: Decimal::try_from(row.try_get::<f64, _>(17)?)
                        .unwrap_or(Decimal::ZERO),
                    priced_cost_amount: Decimal::try_from(row.try_get::<f64, _>(18)?)
                        .unwrap_or(Decimal::ZERO),
                    charge_source: row.try_get(19)?,
                    account_type: row.try_get(20)?,
                    team_id: row.try_get(21)?,
                    api_key_name: row.try_get(22)?,
                    latency_ms: row.try_get::<i64, _>(23)?.max(0) as u64,
                    reservation_id: row.try_get(24)?,
                    billing_group_id: row.try_get(25)?,
                    billing_group_name: row.try_get(26)?,
                    billing_payment_mode: row.try_get(27)?,
                })
            })
            .collect()
    }

    async fn count_billing_activities(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
        filter: &crate::db::BillingActivityFilter,
    ) -> Result<usize, DbError> {
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT COUNT(*)::bigint FROM billing_events WHERE timestamp >= ",
        );
        builder
            .push_bind(start)
            .push(" AND timestamp < ")
            .push_bind(end);
        if let Some(uid) = user_id {
            builder.push(" AND user_id = ").push_bind(uid);
        }
        if let Some(search) = &filter.search {
            let pattern = format!("%{}%", search);
            builder
                .push(" AND (request_id ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR COALESCE(api_key_name, '未命名 Key') ILIKE ")
                .push_bind(pattern.clone())
                .push(" OR model ILIKE ")
                .push_bind(pattern.clone())
                .push(")");
        }
        if let Some(api_key_name) = &filter.api_key_name {
            builder
                .push(" AND COALESCE(api_key_name, '未命名 Key') = ")
                .push_bind(api_key_name);
        }
        if let Some(model) = &filter.model {
            builder.push(" AND model = ").push_bind(model);
        }
        if let Some(source) = &filter.charge_source {
            builder.push(" AND (CASE WHEN billing_payment_mode = 'postpaid' AND package_units > 0 THEN 'postpaid_package' WHEN billing_payment_mode = 'postpaid' THEN 'postpaid' WHEN charge_source IN ('package_and_wallet', 'package', 'wallet', 'free_model', 'none') THEN charge_source WHEN package_units > 0 AND wallet_amount > 0 THEN 'package_and_wallet' WHEN package_units > 0 THEN 'package' WHEN wallet_amount > 0 THEN 'wallet' WHEN status_code >= 400 OR success = false THEN 'none' ELSE 'none' END) = ").push_bind(source);
        }
        let (count,) = builder
            .build_query_as::<(i64,)>()
            .fetch_one(&self.pool)
            .await?;
        Ok(count.max(0) as usize)
    }

    async fn billing_activity_summary(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
    ) -> Result<crate::db::BillingActivitySummary, DbError> {
        let predicate = if user_id.is_some() {
            " AND user_id = $3"
        } else {
            ""
        };
        let sql = format!(
            "WITH classified AS (\
                SELECT *, CASE \
                    WHEN status_code = 499 OR activity_status = 'interrupted' THEN 'interrupted' \
                    WHEN status_code >= 400 OR success = false THEN 'failed' \
                    WHEN COALESCE(package_units, 0) = 0 AND COALESCE(wallet_amount, 0) = 0 \
                         AND COALESCE(NULLIF(priced_cost_amount, 0), cost_amount, 0) = 0 THEN 'zero_cost' \
                    ELSE 'success' END AS derived_status \
                FROM billing_events WHERE timestamp >= $1 AND timestamp < $2{predicate}) \
             SELECT COUNT(*)::bigint, \
                    COUNT(*) FILTER (WHERE derived_status = 'success')::bigint, \
                    COUNT(*) FILTER (WHERE derived_status = 'failed')::bigint, \
                    COUNT(*) FILTER (WHERE derived_status = 'interrupted')::bigint, \
                    COUNT(*) FILTER (WHERE derived_status = 'zero_cost')::bigint, \
                    COALESCE(SUM(total_tokens), 0)::bigint, \
                    COALESCE(SUM(package_units), 0)::bigint, \
                    COALESCE(SUM(wallet_amount), 0), \
                    COALESCE(SUM(COALESCE(NULLIF(priced_cost_amount, 0), cost_amount, 0)), 0), \
                    COUNT(DISTINCT COALESCE(api_key_name, '未命名 Key'))::bigint, \
                    COUNT(DISTINCT model)::bigint FROM classified"
        );
        let mut query =
            query_as::<_, (i64, i64, i64, i64, i64, i64, i64, f64, f64, i64, i64)>(&sql)
                .bind(start)
                .bind(end);
        if let Some(uid) = user_id {
            query = query.bind(uid);
        }
        let row = query.fetch_one(&self.pool).await?;
        Ok(crate::db::BillingActivitySummary {
            activity_count: row.0.max(0) as u64,
            success_count: row.1.max(0) as u64,
            failed_count: row.2.max(0) as u64,
            interrupted_count: row.3.max(0) as u64,
            zero_cost_count: row.4.max(0) as u64,
            total_tokens: row.5.max(0) as u64,
            package_units: row.6.max(0) as u64,
            wallet_amount: Decimal::try_from(row.7).unwrap_or(Decimal::ZERO),
            priced_cost_amount: Decimal::try_from(row.8).unwrap_or(Decimal::ZERO),
            api_key_count: row.9.max(0) as u64,
            model_count: row.10.max(0) as u64,
        })
    }

    async fn billing_activity_dimensions(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
    ) -> Result<crate::db::BillingActivityDimensions, DbError> {
        let predicate = if user_id.is_some() {
            " AND user_id = $3"
        } else {
            ""
        };
        let source_expr = "CASE WHEN billing_payment_mode = 'postpaid' AND package_units > 0 THEN 'postpaid_package' WHEN billing_payment_mode = 'postpaid' THEN 'postpaid' WHEN charge_source IN ('package_and_wallet', 'package', 'wallet', 'free_model', 'none') THEN charge_source WHEN package_units > 0 AND wallet_amount > 0 THEN 'package_and_wallet' WHEN package_units > 0 THEN 'package' WHEN wallet_amount > 0 THEN 'wallet' WHEN status_code >= 400 OR success = false THEN 'none' ELSE 'none' END";
        let filtered = format!(
            "WITH filtered AS (SELECT COALESCE(api_key_name, '未命名 Key') AS api_key_name, model, package_units, COALESCE(wallet_amount, 0) AS wallet_amount, COALESCE(NULLIF(priced_cost_amount, 0), cost_amount, 0) AS priced_cost_amount, cost_amount, billing_payment_mode, {source_expr} AS charge_source, total_tokens FROM billing_events WHERE timestamp >= $1 AND timestamp < $2{predicate})"
        );

        let api_key_sql = format!(
            "{filtered} SELECT api_key_name, COUNT(*)::bigint, 1::bigint, COUNT(DISTINCT model)::bigint, ARRAY_AGG(DISTINCT model ORDER BY model), ARRAY_AGG(DISTINCT charge_source ORDER BY charge_source), COALESCE(SUM(total_tokens), 0)::bigint, COALESCE(SUM(package_units), 0)::bigint, COALESCE(SUM(wallet_amount), 0), COALESCE(SUM(priced_cost_amount), 0) FROM filtered GROUP BY api_key_name ORDER BY 2 DESC, 1"
        );
        let model_sql = format!(
            "{filtered} SELECT model, COUNT(*)::bigint, COUNT(DISTINCT api_key_name)::bigint, 1::bigint, ARRAY_AGG(DISTINCT api_key_name ORDER BY api_key_name), ARRAY_AGG(DISTINCT charge_source ORDER BY charge_source), COALESCE(SUM(total_tokens), 0)::bigint, COALESCE(SUM(package_units), 0)::bigint, COALESCE(SUM(wallet_amount), 0), COALESCE(SUM(priced_cost_amount), 0) FROM filtered GROUP BY model ORDER BY 2 DESC, 1"
        );
        let source_sql = format!(
            "{filtered} SELECT charge_source, COUNT(*)::bigint, COUNT(DISTINCT api_key_name)::bigint, COUNT(DISTINCT model)::bigint, ARRAY_AGG(DISTINCT api_key_name ORDER BY api_key_name), ARRAY_AGG(DISTINCT model ORDER BY model), COALESCE(SUM(total_tokens), 0)::bigint, COALESCE(SUM(package_units), 0)::bigint, COALESCE(SUM(wallet_amount), 0), COALESCE(SUM(priced_cost_amount), 0) FROM filtered GROUP BY charge_source ORDER BY 2 DESC, 1"
        );

        type DimensionTuple = (
            String,
            i64,
            i64,
            i64,
            Vec<String>,
            Vec<String>,
            i64,
            i64,
            f64,
            f64,
        );
        async fn fetch_dimension_rows(
            pool: &PgPool,
            sql: &str,
            start: &str,
            end: &str,
            user_id: Option<&str>,
        ) -> Result<Vec<DimensionTuple>, DbError> {
            let mut statement = query_as::<_, DimensionTuple>(sql).bind(start).bind(end);
            if let Some(uid) = user_id {
                statement = statement.bind(uid);
            }
            statement.fetch_all(pool).await.map_err(DbError::from)
        }

        let api_keys = fetch_dimension_rows(&self.pool, &api_key_sql, start, end, user_id).await?;
        let models = fetch_dimension_rows(&self.pool, &model_sql, start, end, user_id).await?;
        let sources = fetch_dimension_rows(&self.pool, &source_sql, start, end, user_id).await?;
        let map_row = |row: DimensionTuple| crate::db::BillingActivityDimensionRow {
            name: row.0,
            activity_count: row.1.max(0) as u64,
            key_count: row.2.max(0) as u64,
            model_count: row.3.max(0) as u64,
            related_names: row.4,
            source_names: row.5,
            total_tokens: row.6.max(0) as u64,
            package_units: row.7.max(0) as u64,
            wallet_amount: Decimal::try_from(row.8).unwrap_or(Decimal::ZERO),
            priced_cost_amount: Decimal::try_from(row.9).unwrap_or(Decimal::ZERO),
        };

        Ok(crate::db::BillingActivityDimensions {
            api_keys: api_keys.into_iter().map(&map_row).collect(),
            models: models.into_iter().map(&map_row).collect(),
            sources: sources.into_iter().map(map_row).collect(),
        })
    }

    async fn period_token_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, u64, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: (i64, i64, f64) = if let Some(uid) = user_id {
            query_as("SELECT COALESCE(SUM(prompt_tokens),0)::bigint, COALESCE(SUM(cache_hit_input_tokens),0)::bigint, COALESCE(SUM(completion_tokens),0)::double precision FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3")
                .bind(&start).bind(&end).bind(uid).fetch_one(&self.pool).await?
        } else {
            query_as("SELECT COALESCE(SUM(prompt_tokens),0)::bigint, COALESCE(SUM(cache_hit_input_tokens),0)::bigint, COALESCE(SUM(completion_tokens),0)::double precision FROM billing_events WHERE timestamp >= $1 AND timestamp < $2")
                .bind(&start).bind(&end).fetch_one(&self.pool).await?
        };
        Ok(vec![
            ("input".into(), rows.0.max(0) as u64, Decimal::ZERO),
            ("cache_hit".into(), rows.1.max(0) as u64, Decimal::ZERO),
            ("output".into(), rows.2.max(0.0) as u64, Decimal::ZERO),
        ])
    }

    async fn period_model_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, f64)> = if let Some(uid) = user_id {
            query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(cost_amount), 0) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(cost_amount), 0) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(m, c)| (m, Decimal::try_from(c).unwrap_or(Decimal::ZERO)))
            .collect())
    }

    async fn period_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, String, f64)> = if let Some(uid) = user_id {
            query_as::<_, (String, String, f64)>(
                "SELECT ul.channel_id, COALESCE(c.name, ul.channel_id), COALESCE(SUM(ul.cost_amount), 0) \
                 FROM billing_events ul LEFT JOIN channels c ON c.id = ul.channel_id \
                 WHERE ul.timestamp >= $1 AND ul.timestamp < $2 AND ul.user_id = $3 \
                 GROUP BY ul.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, String, f64)>(
                "SELECT ul.channel_id, COALESCE(c.name, ul.channel_id), COALESCE(SUM(ul.cost_amount), 0) \
                 FROM billing_events ul LEFT JOIN channels c ON c.id = ul.channel_id \
                 WHERE ul.timestamp >= $1 AND ul.timestamp < $2 \
                 GROUP BY ul.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(ch, n, c)| (ch, n, Decimal::try_from(c).unwrap_or(Decimal::ZERO)))
            .collect())
    }

    async fn daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                 GROUP BY day ORDER BY day DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(d, c, n)| (d, Decimal::try_from(c).unwrap_or(Decimal::ZERO), n as u64))
            .collect())
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
            query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
            )
            .bind(&start)
            .bind(&end)
            .bind(uid)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
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
    ) -> Result<Vec<(String, Decimal, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
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
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day DESC LIMIT $3 OFFSET $4",
            )
            .bind(&start)
            .bind(&end)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(d, c, n)| (d, Decimal::try_from(c).unwrap_or(Decimal::ZERO), n as u64))
            .collect())
    }

    async fn billing_months(&self) -> Result<Vec<String>, DbError> {
        let rows: Vec<(String,)> = query_as(
            "SELECT DISTINCT LEFT(timestamp::text, 7) AS month FROM billing_events ORDER BY month DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn billing_months_for_user(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        let rows: Vec<(String,)> = query_as(
            "SELECT DISTINCT LEFT(timestamp::text, 7) AS month FROM billing_events WHERE user_id = $1 ORDER BY month DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn period_summary_all(&self) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        let rows = query_as::<_, (String, f64, i64, i64)>(
            "SELECT LEFT(timestamp::text, 7) AS month, \
             COALESCE(SUM(cost_amount), 0), \
             COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
             FROM billing_events GROUP BY month ORDER BY month DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(m, c, n, t)| {
                (
                    m,
                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                    n as u64,
                    t as u64,
                )
            })
            .collect())
    }

    async fn period_summary_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        let rows = query_as::<_, (String, f64, i64, i64)>(
            "SELECT LEFT(timestamp::text, 7) AS month, \
             COALESCE(SUM(cost_amount), 0), \
             COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
             FROM billing_events WHERE user_id = $1 GROUP BY month ORDER BY month DESC",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(m, c, n, t)| {
                (
                    m,
                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                    n as u64,
                    t as u64,
                )
            })
            .collect())
    }

    async fn admin_billing_active_counts(
        &self,
        year: i32,
        month: u32,
    ) -> Result<(u64, u64), DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (active_teams, active_users): (i64, i64) = query_as(
            "SELECT \
             COUNT(DISTINCT CASE WHEN account_type = 'team' AND team_id IS NOT NULL THEN team_id END)::bigint, \
             COUNT(DISTINCT user_id)::bigint \
             FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
        )
        .bind(&start)
        .bind(&end)
        .fetch_one(&self.pool)
        .await?;
        Ok((active_teams as u64, active_users as u64))
    }

    async fn admin_billing_team_spend_ranking(
        &self,
        year: i32,
        month: u32,
        limit: usize,
    ) -> Result<Vec<(String, String, Decimal, u64, u64, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = query_as::<_, (String, String, f64, i64, i64, i64)>(
            "SELECT \
             be.team_id, \
             COALESCE(t.name, be.team_id) AS team_name, \
             COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
             COUNT(*)::bigint AS total_requests, \
             COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
             COUNT(DISTINCT be.user_id)::bigint AS active_users \
             FROM billing_events be \
             LEFT JOIN teams t ON t.id = be.team_id \
             WHERE be.timestamp >= $1 AND be.timestamp < $2 \
               AND be.account_type = 'team' AND be.team_id IS NOT NULL \
             GROUP BY be.team_id, t.name \
             ORDER BY total_cost DESC \
             LIMIT $3",
        )
        .bind(&start)
        .bind(&end)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(team_id, team_name, total_cost, total_requests, total_tokens, active_users)| {
                    (
                        team_id,
                        team_name,
                        Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                        total_requests as u64,
                        total_tokens as u64,
                        active_users as u64,
                    )
                },
            )
            .collect())
    }

    async fn admin_billing_teams_page(
        &self,
        year: i32,
        month: u32,
        search: Option<&str>,
        sort_by: Option<&str>,
        sort_order: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<(
                String,
                String,
                String,
                Decimal,
                u64,
                u64,
                u64,
                Option<String>,
            )>,
            usize,
        ),
        DbError,
    > {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let search_term = search
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("%{}%", value.trim()));
        let (sort_expr, sort_dir) = match sort_by.unwrap_or("total_cost") {
            "team_name" => (
                "team_name",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            "total_requests" => (
                "total_requests",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            "total_tokens" => (
                "total_tokens",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            "active_users" => (
                "active_users",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            "last_billed_at" => (
                "last_billed_at",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
            _ => (
                "total_cost",
                if sort_order == Some("asc") {
                    "ASC"
                } else {
                    "DESC"
                },
            ),
        };

        let base_cte = "WITH team_billing AS ( \
            SELECT \
            be.team_id, \
            COALESCE(t.name, be.team_id) AS team_name, \
            COALESCE(t.owner_id, '') AS owner_id, \
            COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
            COUNT(*)::bigint AS total_requests, \
            COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
            COUNT(DISTINCT be.user_id)::bigint AS active_users, \
            MAX(be.timestamp)::text AS last_billed_at \
            FROM billing_events be \
            LEFT JOIN teams t ON t.id = be.team_id \
            WHERE be.timestamp >= $1 AND be.timestamp < $2 \
              AND be.account_type = 'team' AND be.team_id IS NOT NULL \
            GROUP BY be.team_id, t.name, t.owner_id \
        ) ";

        let count_sql = if search_term.is_some() {
            format!("{} SELECT COUNT(*)::bigint FROM team_billing WHERE team_name ILIKE $3 OR team_id ILIKE $3 OR owner_id ILIKE $3", base_cte)
        } else {
            format!("{} SELECT COUNT(*)::bigint FROM team_billing", base_cte)
        };

        let total: i64 = if let Some(pattern) = &search_term {
            query_scalar::<_, i64>(&count_sql)
                .bind(&start)
                .bind(&end)
                .bind(pattern)
                .fetch_one(&self.pool)
                .await?
        } else {
            query_scalar::<_, i64>(&count_sql)
                .bind(&start)
                .bind(&end)
                .fetch_one(&self.pool)
                .await?
        };

        let page_sql = if search_term.is_some() {
            format!(
                "{} SELECT team_id, team_name, owner_id, total_cost, total_requests, total_tokens, active_users, last_billed_at \
                 FROM team_billing \
                 WHERE team_name ILIKE $3 OR team_id ILIKE $3 OR owner_id ILIKE $3 \
                 ORDER BY {} {} LIMIT $4 OFFSET $5",
                base_cte, sort_expr, sort_dir
            )
        } else {
            format!(
                "{} SELECT team_id, team_name, owner_id, total_cost, total_requests, total_tokens, active_users, last_billed_at \
                 FROM team_billing \
                 ORDER BY {} {} LIMIT $3 OFFSET $4",
                base_cte, sort_expr, sort_dir
            )
        };

        let rows = if let Some(pattern) = &search_term {
            query_as::<_, (String, String, String, f64, i64, i64, i64, Option<String>)>(&page_sql)
                .bind(&start)
                .bind(&end)
                .bind(pattern)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await?
        } else {
            query_as::<_, (String, String, String, f64, i64, i64, i64, Option<String>)>(&page_sql)
                .bind(&start)
                .bind(&end)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await?
        };

        Ok((
            rows.into_iter()
                .map(
                    |(
                        team_id,
                        team_name,
                        owner_id,
                        total_cost,
                        total_requests,
                        total_tokens,
                        active_users,
                        last_billed_at,
                    )| {
                        (
                            team_id,
                            team_name,
                            owner_id,
                            Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                            total_requests as u64,
                            total_tokens as u64,
                            active_users as u64,
                            last_billed_at,
                        )
                    },
                )
                .collect(),
            total as usize,
        ))
    }

    async fn admin_billing_team_users_page(
        &self,
        team_id: &str,
        year: i32,
        month: u32,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<(String, String, Decimal, u64, u64, Option<String>)>,
            usize,
        ),
        DbError,
    > {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let total: i64 = query_scalar(
            "SELECT COUNT(*)::bigint FROM ( \
             SELECT be.user_id \
             FROM billing_events be \
             WHERE be.timestamp >= $1 AND be.timestamp < $2 \
               AND be.team_id = $3 AND be.account_type = 'team' \
             GROUP BY be.user_id, be.user_name \
            ) users",
        )
        .bind(&start)
        .bind(&end)
        .bind(team_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = query_as::<_, (String, String, f64, i64, i64, Option<String>)>(
            "SELECT \
             be.user_id, \
             be.user_name, \
             COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
             COUNT(*)::bigint AS total_requests, \
             COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
             MAX(be.timestamp)::text AS last_billed_at \
             FROM billing_events be \
             WHERE be.timestamp >= $1 AND be.timestamp < $2 \
               AND be.team_id = $3 AND be.account_type = 'team' \
             GROUP BY be.user_id, be.user_name \
             ORDER BY total_cost DESC LIMIT $4 OFFSET $5",
        )
        .bind(&start)
        .bind(&end)
        .bind(team_id)
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;

        Ok((
            rows.into_iter()
                .map(
                    |(
                        user_id,
                        user_name,
                        total_cost,
                        total_requests,
                        total_tokens,
                        last_billed_at,
                    )| {
                        (
                            user_id,
                            user_name,
                            Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                            total_requests as u64,
                            total_tokens as u64,
                            last_billed_at,
                        )
                    },
                )
                .collect(),
            total as usize,
        ))
    }

    async fn admin_billing_scoped_period_summary(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64, Vec<(String, u64, Decimal)>), DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (
            cost,
            count,
            tokens,
            prompt_tokens,
            prompt_cost,
            cache_tokens,
            cache_cost,
            completion_tokens,
            completion_cost,
        ): (f64, i64, i64, i64, f64, i64, f64, i64, f64) = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as(
                    "SELECT COALESCE(SUM(cost_amount), 0), \
                     COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                     COALESCE(SUM(prompt_tokens),0)::bigint, \
                     COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price), 0), \
                     COALESCE(SUM(cache_hit_input_tokens),0)::bigint, \
                     COALESCE(SUM(cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                     COALESCE(SUM(completion_tokens),0)::bigint, \
                     COALESCE(SUM(completion_tokens / 1000000.0 * completion_price), 0) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team'",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
            } else {
                query_as(
                    "SELECT COALESCE(SUM(cost_amount), 0), \
                     COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                     COALESCE(SUM(prompt_tokens),0)::bigint, \
                     COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price), 0), \
                     COALESCE(SUM(cache_hit_input_tokens),0)::bigint, \
                     COALESCE(SUM(cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                     COALESCE(SUM(completion_tokens),0)::bigint, \
                     COALESCE(SUM(completion_tokens / 1000000.0 * completion_price), 0) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as(
                "SELECT COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                 COALESCE(SUM(prompt_tokens),0)::bigint, \
                 COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price), 0), \
                 COALESCE(SUM(cache_hit_input_tokens),0)::bigint, \
                 COALESCE(SUM(cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COALESCE(SUM(completion_tokens),0)::bigint, \
                 COALESCE(SUM(completion_tokens / 1000000.0 * completion_price), 0) \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team'",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_as(
                "SELECT COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint, \
                 COALESCE(SUM(prompt_tokens),0)::bigint, \
                 COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price), 0), \
                 COALESCE(SUM(cache_hit_input_tokens),0)::bigint, \
                 COALESCE(SUM(cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), \
                 COALESCE(SUM(completion_tokens),0)::bigint, \
                 COALESCE(SUM(completion_tokens / 1000000.0 * completion_price), 0) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
            )
            .bind(&start)
            .bind(&end)
            .fetch_one(&self.pool)
            .await?
        };
        Ok((
            Decimal::try_from(cost).unwrap_or(Decimal::ZERO),
            count as u64,
            tokens as u64,
            vec![
                (
                    "input".to_string(),
                    prompt_tokens as u64,
                    Decimal::try_from(prompt_cost).unwrap_or(Decimal::ZERO),
                ),
                (
                    "cache_hit".to_string(),
                    cache_tokens as u64,
                    Decimal::try_from(cache_cost).unwrap_or(Decimal::ZERO),
                ),
                (
                    "output".to_string(),
                    completion_tokens as u64,
                    Decimal::try_from(completion_cost).unwrap_or(Decimal::ZERO),
                ),
            ],
        ))
    }

    async fn admin_billing_scoped_model_breakdown(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, f64)> = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, f64)>(
                    "SELECT model, COALESCE(SUM(cost_amount), 0) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team' \
                     GROUP BY model ORDER BY 2 DESC",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, f64)>(
                    "SELECT model, COALESCE(SUM(cost_amount), 0) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                     GROUP BY model ORDER BY 2 DESC",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(cost_amount), 0) \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team' \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64)>(
                "SELECT model, COALESCE(SUM(cost_amount), 0) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY model ORDER BY 2 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(model, cost)| (model, Decimal::try_from(cost).unwrap_or(Decimal::ZERO)))
            .collect())
    }

    async fn admin_billing_scoped_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, Decimal)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, String, f64)> = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, String, f64)>(
                    "SELECT be.channel_id, COALESCE(c.name, be.channel_id), COALESCE(SUM(be.cost_amount), 0) \
                     FROM billing_events be LEFT JOIN channels c ON c.id = be.channel_id \
                     WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                       AND be.team_id = $3 AND be.user_id = $4 AND be.account_type = 'team' \
                     GROUP BY be.channel_id, c.name ORDER BY 3 DESC",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, String, f64)>(
                    "SELECT be.channel_id, COALESCE(c.name, be.channel_id), COALESCE(SUM(be.cost_amount), 0) \
                     FROM billing_events be LEFT JOIN channels c ON c.id = be.channel_id \
                     WHERE be.timestamp >= $1 AND be.timestamp < $2 AND be.user_id = $3 \
                     GROUP BY be.channel_id, c.name ORDER BY 3 DESC",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, String, f64)>(
                "SELECT be.channel_id, COALESCE(c.name, be.channel_id), COALESCE(SUM(be.cost_amount), 0) \
                 FROM billing_events be LEFT JOIN channels c ON c.id = be.channel_id \
                 WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                   AND be.team_id = $3 AND be.account_type = 'team' \
                 GROUP BY be.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, String, f64)>(
                "SELECT be.channel_id, COALESCE(c.name, be.channel_id), COALESCE(SUM(be.cost_amount), 0) \
                 FROM billing_events be LEFT JOIN channels c ON c.id = be.channel_id \
                 WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                 GROUP BY be.channel_id, c.name ORDER BY 3 DESC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(channel_id, name, cost)| {
                (
                    channel_id,
                    name,
                    Decimal::try_from(cost).unwrap_or(Decimal::ZERO),
                )
            })
            .collect())
    }

    async fn admin_billing_daily_trend(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows: Vec<(String, f64, i64, i64)> = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, f64, i64, i64)>(
                    "SELECT LEFT(timestamp::text, 10) as day, \
                     COALESCE(SUM(cost_amount), 0), \
                     COUNT(*)::bigint, \
                     COALESCE(SUM(total_tokens), 0)::bigint \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team' \
                     GROUP BY day ORDER BY day ASC",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, f64, i64, i64)>(
                    "SELECT LEFT(timestamp::text, 10) as day, \
                     COALESCE(SUM(cost_amount), 0), \
                     COUNT(*)::bigint, \
                     COALESCE(SUM(total_tokens), 0)::bigint \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                     GROUP BY day ORDER BY day ASC",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, f64, i64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint, \
                 COALESCE(SUM(total_tokens), 0)::bigint \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team' \
                 GROUP BY day ORDER BY day ASC",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64, i64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint, \
                 COALESCE(SUM(total_tokens), 0)::bigint \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day ASC",
            )
            .bind(&start)
            .bind(&end)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(date, total_cost, total_requests, total_tokens)| {
                (
                    date,
                    Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                    total_requests as u64,
                    total_tokens as u64,
                )
            })
            .collect())
    }

    async fn admin_billing_scoped_count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<usize, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let (count,): (i64,) = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as(
                    "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team'",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
            } else {
                query_as(
                    "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .fetch_one(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team'",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_as(
                "SELECT COUNT(DISTINCT LEFT(timestamp::text, 10)) \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2",
            )
            .bind(&start)
            .bind(&end)
            .fetch_one(&self.pool)
            .await?
        };
        Ok(count as usize)
    }

    async fn admin_billing_scoped_daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError> {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, f64, i64)>(
                    "SELECT LEFT(timestamp::text, 10) as day, \
                     COALESCE(SUM(cost_amount), 0), \
                     COUNT(*)::bigint \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 \
                       AND team_id = $3 AND user_id = $4 AND account_type = 'team' \
                     GROUP BY day ORDER BY day DESC LIMIT $5 OFFSET $6",
                )
                .bind(&start)
                .bind(&end)
                .bind(tid)
                .bind(uid)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, f64, i64)>(
                    "SELECT LEFT(timestamp::text, 10) as day, \
                     COALESCE(SUM(cost_amount), 0), \
                     COUNT(*)::bigint \
                     FROM billing_events \
                     WHERE timestamp >= $1 AND timestamp < $2 AND user_id = $3 \
                     GROUP BY day ORDER BY day DESC LIMIT $4 OFFSET $5",
                )
                .bind(&start)
                .bind(&end)
                .bind(uid)
                .bind(limit as i64)
                .bind(offset as i64)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events \
                 WHERE timestamp >= $1 AND timestamp < $2 \
                   AND team_id = $3 AND account_type = 'team' \
                 GROUP BY day ORDER BY day DESC LIMIT $4 OFFSET $5",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64, i64)>(
                "SELECT LEFT(timestamp::text, 10) as day, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint \
                 FROM billing_events WHERE timestamp >= $1 AND timestamp < $2 \
                 GROUP BY day ORDER BY day DESC LIMIT $3 OFFSET $4",
            )
            .bind(&start)
            .bind(&end)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(d, c, n)| (d, Decimal::try_from(c).unwrap_or(Decimal::ZERO), n as u64))
            .collect())
    }

    async fn admin_billing_scoped_period_summary_all(
        &self,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        let rows = if let Some(uid) = user_id {
            if let Some(tid) = team_id {
                query_as::<_, (String, f64, i64, i64)>(
                    "SELECT LEFT(timestamp::text, 7) AS month, \
                     COALESCE(SUM(cost_amount), 0), \
                     COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                     FROM billing_events \
                     WHERE team_id = $1 AND user_id = $2 AND account_type = 'team' \
                     GROUP BY month ORDER BY month DESC",
                )
                .bind(tid)
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            } else {
                query_as::<_, (String, f64, i64, i64)>(
                    "SELECT LEFT(timestamp::text, 7) AS month, \
                     COALESCE(SUM(cost_amount), 0), \
                     COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                     FROM billing_events WHERE user_id = $1 \
                     GROUP BY month ORDER BY month DESC",
                )
                .bind(uid)
                .fetch_all(&self.pool)
                .await?
            }
        } else if let Some(tid) = team_id {
            query_as::<_, (String, f64, i64, i64)>(
                "SELECT LEFT(timestamp::text, 7) AS month, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM billing_events \
                 WHERE team_id = $1 AND account_type = 'team' \
                 GROUP BY month ORDER BY month DESC",
            )
            .bind(tid)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (String, f64, i64, i64)>(
                "SELECT LEFT(timestamp::text, 7) AS month, \
                 COALESCE(SUM(cost_amount), 0), \
                 COUNT(*)::bigint, COALESCE(SUM(total_tokens),0)::bigint \
                 FROM billing_events GROUP BY month ORDER BY month DESC",
            )
            .fetch_all(&self.pool)
            .await?
        };
        Ok(rows
            .into_iter()
            .map(|(m, c, n, t)| {
                (
                    m,
                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                    n as u64,
                    t as u64,
                )
            })
            .collect())
    }

    async fn admin_billing_user_spend_ranking(
        &self,
        year: i32,
        month: u32,
        limit: usize,
    ) -> Result<
        Vec<(
            Option<String>,
            Option<String>,
            u64,
            bool,
            String,
            String,
            Decimal,
            u64,
            u64,
            u64,
            Option<String>,
        )>,
        DbError,
    > {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let rows = query_as::<_, (Option<String>, Option<String>, i64, bool, String, Option<String>, f64, i64, i64, i64, Option<String>)>(
            "WITH filtered AS ( \
                SELECT be.* \
                FROM billing_events be \
                WHERE be.timestamp >= $1 AND be.timestamp < $2 \
            ), user_totals AS ( \
                SELECT \
                    be.user_id, \
                    COALESCE(MAX(NULLIF(be.user_name, '')), be.user_id) AS user_name, \
                    COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
                    COUNT(*)::bigint AS total_requests, \
                    COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
                    COUNT(DISTINCT be.api_key_name)::bigint AS api_key_count, \
                    MAX(be.timestamp)::text AS last_billed_at, \
                    COUNT(DISTINCT CASE WHEN be.team_id IS NOT NULL THEN be.team_id END)::bigint AS team_count \
                FROM filtered be \
                GROUP BY be.user_id \
            ), user_team_rank AS ( \
                SELECT \
                    be.user_id, \
                    be.team_id, \
                    t.name AS team_name, \
                    ROW_NUMBER() OVER ( \
                        PARTITION BY be.user_id \
                        ORDER BY COALESCE(SUM(be.cost_amount), 0) DESC, COUNT(*) DESC, be.team_id ASC NULLS LAST \
                    ) AS rank_no \
                FROM filtered be \
                LEFT JOIN teams t ON t.id = be.team_id \
                WHERE be.team_id IS NOT NULL \
                GROUP BY be.user_id, be.team_id, t.name \
            ) \
            SELECT \
                utr.team_id, \
                utr.team_name, \
                ut.team_count, \
                (ut.team_count > 1) AS multi_team, \
                ut.user_id, \
                ut.user_name, \
                ut.total_cost, \
                ut.total_requests, \
                ut.total_tokens, \
                ut.api_key_count, \
                ut.last_billed_at \
            FROM user_totals ut \
            LEFT JOIN user_team_rank utr \
              ON utr.user_id = ut.user_id AND utr.rank_no = 1 \
            ORDER BY ut.total_cost DESC, ut.total_requests DESC, ut.user_id ASC \
            LIMIT $3",
        )
        .bind(&start)
        .bind(&end)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    team_id,
                    team_name,
                    team_count,
                    multi_team,
                    user_id,
                    user_name,
                    total_cost,
                    total_requests,
                    total_tokens,
                    api_key_count,
                    last_billed_at,
                )| {
                    let fallback_user_name = user_id.clone();
                    (
                        team_id,
                        team_name,
                        team_count as u64,
                        multi_team,
                        user_id,
                        user_name.unwrap_or(fallback_user_name),
                        Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                        total_requests as u64,
                        total_tokens as u64,
                        api_key_count as u64,
                        last_billed_at,
                    )
                },
            )
            .collect())
    }

    async fn admin_billing_user_api_keys_page(
        &self,
        team_id: Option<&str>,
        user_id: &str,
        year: i32,
        month: u32,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<(
                Option<String>,
                Decimal,
                u64,
                u64,
                u64,
                u64,
                u64,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<bool>,
                Option<String>,
            )>,
            usize,
        ),
        DbError,
    > {
        let start = format!("{}-{:02}-01T00:00:00", year, month);
        let end = if month == 12 {
            format!("{}-01-01T00:00:00", year + 1)
        } else {
            format!("{}-{:02}-01T00:00:00", year, month + 1)
        };
        let total: i64 = if let Some(tid) = team_id {
            query_scalar(
                "SELECT COUNT(*)::bigint FROM ( \
                 SELECT be.api_key_name \
                 FROM billing_events be \
                 WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                   AND be.team_id = $3 AND be.user_id = $4 AND be.account_type = 'team' \
                 GROUP BY be.api_key_name \
                ) api_keys",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?
        } else {
            query_scalar(
                "SELECT COUNT(*)::bigint FROM ( \
                 SELECT be.api_key_name \
                 FROM billing_events be \
                 WHERE be.timestamp >= $1 AND be.timestamp < $2 AND be.user_id = $3 \
                 GROUP BY be.api_key_name \
                ) api_keys",
            )
            .bind(&start)
            .bind(&end)
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?
        };

        let rows = if let Some(tid) = team_id {
            query_as::<_, (Option<String>, f64, i64, i64, i64, i64, i64, Option<String>, Option<String>, Option<String>, Option<bool>, Option<String>)>(
                "WITH key_stats AS ( \
                    SELECT \
                        be.api_key_name, \
                        COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
                        COUNT(*)::bigint AS total_requests, \
                        COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
                        COALESCE(SUM(be.prompt_tokens), 0)::bigint AS prompt_tokens, \
                        COALESCE(SUM(be.completion_tokens), 0)::bigint AS completion_tokens, \
                        COALESCE(SUM(be.cache_hit_input_tokens), 0)::bigint AS cache_hit_input_tokens, \
                        MAX(be.timestamp)::text AS last_request_at, \
                        MAX(be.team_id) AS team_id \
                    FROM billing_events be \
                    WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                      AND be.team_id = $3 AND be.user_id = $4 AND be.account_type = 'team' \
                    GROUP BY be.api_key_name \
                ), key_models AS ( \
                    SELECT \
                        be.api_key_name, \
                        be.model, \
                        ROW_NUMBER() OVER ( \
                            PARTITION BY be.api_key_name \
                            ORDER BY COALESCE(SUM(be.cost_amount), 0) DESC, COUNT(*) DESC, be.model ASC \
                        ) AS rank_no \
                    FROM billing_events be \
                    WHERE be.timestamp >= $1 AND be.timestamp < $2 \
                      AND be.team_id = $3 AND be.user_id = $4 AND be.account_type = 'team' \
                    GROUP BY be.api_key_name, be.model \
                ) \
                SELECT \
                    ks.api_key_name, \
                    ks.total_cost, \
                    ks.total_requests, \
                    ks.total_tokens, \
                    ks.prompt_tokens, \
                    ks.completion_tokens, \
                    ks.cache_hit_input_tokens, \
                    km.model AS primary_model, \
                    ks.last_request_at, \
                    ks.team_id, \
                    ak.enabled AS api_key_enabled, \
                    ak.key AS api_key \
                FROM key_stats ks \
                LEFT JOIN key_models km \
                  ON km.api_key_name IS NOT DISTINCT FROM ks.api_key_name AND km.rank_no = 1 \
                LEFT JOIN api_keys ak \
                  ON ak.name = ks.api_key_name AND ak.user_id = $4 AND ak.team_id = $3 \
                ORDER BY ks.total_cost DESC, ks.total_requests DESC, ks.api_key_name ASC NULLS LAST \
                LIMIT $5 OFFSET $6",
            )
            .bind(&start)
            .bind(&end)
            .bind(tid)
            .bind(user_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            query_as::<_, (Option<String>, f64, i64, i64, i64, i64, i64, Option<String>, Option<String>, Option<String>, Option<bool>, Option<String>)>(
                "WITH key_stats AS ( \
                    SELECT \
                        be.api_key_name, \
                        COALESCE(SUM(be.cost_amount), 0) AS total_cost, \
                        COUNT(*)::bigint AS total_requests, \
                        COALESCE(SUM(be.total_tokens), 0)::bigint AS total_tokens, \
                        COALESCE(SUM(be.prompt_tokens), 0)::bigint AS prompt_tokens, \
                        COALESCE(SUM(be.completion_tokens), 0)::bigint AS completion_tokens, \
                        COALESCE(SUM(be.cache_hit_input_tokens), 0)::bigint AS cache_hit_input_tokens, \
                        MAX(be.timestamp)::text AS last_request_at, \
                        MAX(be.team_id) AS team_id \
                    FROM billing_events be \
                    WHERE be.timestamp >= $1 AND be.timestamp < $2 AND be.user_id = $3 \
                    GROUP BY be.api_key_name \
                ), key_models AS ( \
                    SELECT \
                        be.api_key_name, \
                        be.model, \
                        ROW_NUMBER() OVER ( \
                            PARTITION BY be.api_key_name \
                            ORDER BY COALESCE(SUM(be.cost_amount), 0) DESC, COUNT(*) DESC, be.model ASC \
                        ) AS rank_no \
                    FROM billing_events be \
                    WHERE be.timestamp >= $1 AND be.timestamp < $2 AND be.user_id = $3 \
                    GROUP BY be.api_key_name, be.model \
                ) \
                SELECT \
                    ks.api_key_name, \
                    ks.total_cost, \
                    ks.total_requests, \
                    ks.total_tokens, \
                    ks.prompt_tokens, \
                    ks.completion_tokens, \
                    ks.cache_hit_input_tokens, \
                    km.model AS primary_model, \
                    ks.last_request_at, \
                    ks.team_id, \
                    ak.enabled AS api_key_enabled, \
                    ak.key AS api_key \
                FROM key_stats ks \
                LEFT JOIN key_models km \
                  ON km.api_key_name IS NOT DISTINCT FROM ks.api_key_name AND km.rank_no = 1 \
                LEFT JOIN api_keys ak \
                  ON ak.name = ks.api_key_name AND ak.user_id = $3 \
                ORDER BY ks.total_cost DESC, ks.total_requests DESC, ks.api_key_name ASC NULLS LAST \
                LIMIT $4 OFFSET $5",
            )
            .bind(&start)
            .bind(&end)
            .bind(user_id)
            .bind(limit as i64)
            .bind(offset as i64)
            .fetch_all(&self.pool)
            .await?
        };

        Ok((
            rows.into_iter()
                .map(
                    |(
                        api_key_name,
                        total_cost,
                        total_requests,
                        total_tokens,
                        prompt_tokens,
                        completion_tokens,
                        cache_hit_input_tokens,
                        primary_model,
                        last_request_at,
                        team_id,
                        api_key_enabled,
                        api_key,
                    )| {
                        (
                            api_key_name,
                            Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO),
                            total_requests as u64,
                            total_tokens as u64,
                            prompt_tokens as u64,
                            completion_tokens as u64,
                            cache_hit_input_tokens as u64,
                            primary_model,
                            last_request_at,
                            team_id,
                            api_key_enabled,
                            api_key,
                        )
                    },
                )
                .collect(),
            total as usize,
        ))
    }

    async fn lookup_model_pricing(
        &self,
        model_name: &str,
    ) -> Result<(Decimal, Decimal, Decimal, Decimal), DbError> {
        let (prompt_price, completion_price, cache_read_price, cache_write_price) =
            self.pricing_lookup(model_name).await;
        Ok((
            prompt_price,
            completion_price,
            cache_read_price,
            cache_write_price,
        ))
    }

    // ── Wallet ───────────────────────────────────────────────────────────

    async fn get_wallet_balance(&self, user_id: &str) -> Result<(Decimal, Decimal), DbError> {
        let row: (f64, f64) = query_as("SELECT balance, frozen FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;
        Ok((
            Decimal::try_from(row.0).unwrap_or(Decimal::ZERO),
            Decimal::try_from(row.1).unwrap_or(Decimal::ZERO),
        ))
    }

    async fn update_wallet_balance(&self, user_id: &str, balance: Decimal) -> Result<(), DbError> {
        query("UPDATE users SET balance = $1 WHERE id = $2")
            .bind(balance.to_f64().unwrap_or(0.0))
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
        amount: Decimal,
        balance_before: Decimal,
        balance_after: Decimal,
        method: &str,
        status: &str,
        note: &str,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        query(
            "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(id)
        .bind(user_id)
        .bind(tx_type)
        .bind(amount.to_f64().unwrap_or(0.0))
        .bind(balance_before.to_f64().unwrap_or(0.0))
        .bind(balance_after.to_f64().unwrap_or(0.0))
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
        let rows = query(
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
                amount: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                balance_before: Decimal::try_from(r.get::<f64, _>(4)).unwrap_or(Decimal::ZERO),
                balance_after: Decimal::try_from(r.get::<f64, _>(5)).unwrap_or(Decimal::ZERO),
                method: r.get(6),
                status: r.get(7),
                note: r.get(8),
                created_at: r.get(9),
            })
            .collect())
    }

    async fn count_wallet_transactions(&self, user_id: &str) -> Result<usize, DbError> {
        let (count,): (i64,) =
            query_as("SELECT COUNT(*) FROM wallet_transactions WHERE user_id = $1")
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

        let mut count_builder: QueryBuilder<'_, Postgres> = QueryBuilder::new(
            "SELECT COUNT(DISTINCT LEFT(created_at::text, 10)) FROM wallet_transactions WHERE 1=1",
        );

        let mut data_builder: QueryBuilder<'_, Postgres> =
            QueryBuilder::new(
                "SELECT id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at \
                 FROM wallet_transactions WHERE 1=1",
            );

        let mut date_builder: QueryBuilder<'_, Postgres> =
            QueryBuilder::new(
                "SELECT DISTINCT LEFT(created_at::text, 10) as tx_date FROM wallet_transactions WHERE 1=1",
            );

        add_filters!(count_builder);
        add_filters!(data_builder);
        add_filters!(date_builder);

        // Total distinct dates
        let (total_dates,): (i64,) = count_builder.build_query_as().fetch_one(&self.pool).await?;
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
                amount: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                balance_before: Decimal::try_from(r.get::<f64, _>(4)).unwrap_or(Decimal::ZERO),
                balance_after: Decimal::try_from(r.get::<f64, _>(5)).unwrap_or(Decimal::ZERO),
                method: r.get(6),
                status: r.get(7),
                note: r.get(8),
                created_at: r.get(9),
            })
            .collect();

        Ok((transactions, total_dates))
    }

    async fn get_total_consumed(&self, user_id: &str) -> Result<Decimal, DbError> {
        let (amount,): (f64,) = query_as(
            "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
             completion_tokens / 1000000.0 * completion_price + \
             cache_hit_input_tokens / 1000000.0 * cache_read_price), 0) \
             FROM billing_events WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(Decimal::try_from(amount).unwrap_or(Decimal::ZERO))
    }

    async fn get_total_recharged(&self, user_id: &str) -> Result<Decimal, DbError> {
        let (amount,): (f64,) = query_as(
            "SELECT COALESCE(SUM(amount), 0) FROM wallet_transactions \
             WHERE user_id = $1 AND type = 'recharge' AND status = 'completed'",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(Decimal::try_from(amount).unwrap_or(Decimal::ZERO))
    }

    async fn get_wallet_estimated_days(&self, user_id: &str) -> Result<Option<Decimal>, DbError> {
        let thirty_days_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
        let (total_cost,): (f64,) = query_as(
            "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + \
             completion_tokens / 1000000.0 * completion_price + \
             cache_hit_input_tokens / 1000000.0 * cache_read_price), 0) \
             FROM billing_events WHERE user_id = $1 AND timestamp >= $2",
        )
        .bind(user_id)
        .bind(&thirty_days_ago)
        .fetch_one(&self.pool)
        .await?;

        let (balance,): (f64,) = query_as("SELECT balance FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&self.pool)
            .await?;

        let daily_avg = Decimal::try_from(total_cost).unwrap_or(Decimal::ZERO) / Decimal::from(30);
        if daily_avg <= Decimal::ZERO {
            return Ok(None);
        }
        let bal = Decimal::try_from(balance).unwrap_or(Decimal::ZERO);
        Ok(Some(bal / daily_avg))
    }

    // ── Recharge Keys ────────────────────────────────────────────────────

    async fn create_recharge_key(
        &self,
        key: &str,
        amount: Decimal,
        created_by: &str,
        expires_at: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<(), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        query(
            "INSERT INTO recharge_keys (key, amount, created_by, created_at, expires_at, team_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(key)
        .bind(amount.to_f64().unwrap_or(0.0))
        .bind(created_by)
        .bind(&now)
        .bind(expires_at)
        .bind(team_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn redeem_recharge_key(
        &self,
        key: &str,
        user_id: &str,
    ) -> Result<(Decimal, Option<String>), DbError> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;

        // Atomically mark as used — only if not already used/revoked/expired
        let updated = query(
            "UPDATE recharge_keys SET used_by = $1, used_at = $2 \
             WHERE key = $3 AND used_by IS NULL \
             AND (revoked IS NULL OR revoked = false) \
             AND (expires_at IS NULL OR expires_at > $4)",
        )
        .bind(user_id)
        .bind(&now)
        .bind(key)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        if updated.rows_affected() == 0 {
            // Key doesn't exist or was already used/revoked — fetch details for error message
            let existing =
                query("SELECT used_by, revoked, expires_at FROM recharge_keys WHERE key = $1")
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

        // Get amount & team_id from the key
        let (amount, team_id): (f64, Option<String>) =
            query_as("SELECT amount, team_id FROM recharge_keys WHERE key = $1")
                .bind(key)
                .fetch_one(&mut *tx)
                .await?;

        if let Some(ref tid) = team_id {
            // ── Team wallet branch ──
            let (balance,): (f64,) =
                query_as("SELECT balance FROM team_wallets WHERE team_id = $1")
                    .bind(tid)
                    .fetch_one(&mut *tx)
                    .await?;
            let new_balance = balance + amount;
            query("UPDATE team_wallets SET balance = $1, updated_at = $2 WHERE team_id = $3")
                .bind(new_balance)
                .bind(&now)
                .bind(tid)
                .execute(&mut *tx)
                .await?;
            // Record team wallet transaction
            query(
                "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, \
                 balance_after, method, status, note, created_at, team_id, account_type) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
            )
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(user_id)
            .bind("recharge")
            .bind(amount)
            .bind(balance)
            .bind(new_balance)
            .bind("recharge_key")
            .bind("completed")
            .bind(format!("Key recharge: {} for team: {}", key, tid))
            .bind(&now)
            .bind(tid)
            .bind("team")
            .execute(&mut *tx)
            .await?;
        } else {
            // ── Personal wallet branch (existing behavior) ──
            let (balance,): (f64,) = query_as("SELECT balance FROM users WHERE id = $1")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| DbError("User not found".to_string()))?;
            let new_balance = balance + amount;
            query("UPDATE users SET balance = $1 WHERE id = $2")
                .bind(new_balance)
                .bind(user_id)
                .execute(&mut *tx)
                .await?;
            query(
                "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, \
                 balance_after, method, status, note, created_at, team_id, account_type) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
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
            .bind(Option::<String>::None)
            .bind("user")
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok((Decimal::try_from(amount).unwrap_or(Decimal::ZERO), team_id))
    }

    async fn revoke_recharge_key(&self, key: &str) -> Result<(), DbError> {
        let result = query(
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
        let rows = query(
            "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked, team_id \
             FROM recharge_keys ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RechargeKeyRow {
                key: r.get(0),
                amount: Decimal::try_from(r.get::<f64, _>(1)).unwrap_or(Decimal::ZERO),
                used_by: r.get(2),
                used_at: r.get(3),
                created_by: r.get(4),
                created_at: r.get(5),
                expires_at: r.get(6),
                revoked: r.get::<bool, _>(7),
                team_id: r.get(8),
            })
            .collect())
    }

    async fn list_recharge_keys_paginated(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<RechargeKeyRow>, DbError> {
        let rows = query(
            "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked, team_id \
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
                amount: Decimal::try_from(r.get::<f64, _>(1)).unwrap_or(Decimal::ZERO),
                used_by: r.get(2),
                used_at: r.get(3),
                created_by: r.get(4),
                created_at: r.get(5),
                expires_at: r.get(6),
                revoked: r.get::<bool, _>(7),
                team_id: r.get(8),
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
        let mut builder: QueryBuilder<'_, Postgres> =
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
        let mut builder: QueryBuilder<'_, Postgres> = QueryBuilder::new(
            "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked, team_id \
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
                amount: Decimal::try_from(r.get::<f64, _>(1)).unwrap_or(Decimal::ZERO),
                used_by: r.get(2),
                used_at: r.get(3),
                created_by: r.get(4),
                created_at: r.get(5),
                expires_at: r.get(6),
                revoked: r.get::<bool, _>(7),
                team_id: r.get(8),
            })
            .collect())
    }

    // ── Settings ─────────────────────────────────────────────────────────

    async fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let result: Option<(String,)> =
            query_as("SELECT value FROM balancer_settings WHERE key = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        Ok(result.map(|r| r.0))
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        query(
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

    async fn set_gateway_config(&self, config: &GatewayRuntimeConfig) -> Result<(), DbError> {
        let json = serde_json::to_string(config)
            .map_err(|e| DbError(format!("Failed to serialize gateway config: {}", e)))?;
        self.set_setting("gateway_config", &json).await
    }

    // ── Content Filter Rules ─────────────────────────────────────────

    async fn list_filter_rules(&self) -> Result<Vec<ContentFilterRule>, DbError> {
        let rows = query_as::<_, (String, String, String, String, String, String, Option<String>, Option<String>, bool, i32, String, String)>(
            "SELECT id, name, pattern_type, pattern, action, scope, channel_id, replacement, enabled, priority, created_at, updated_at FROM content_filter_rules ORDER BY priority ASC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to list filter rules: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    name,
                    pattern_type,
                    pattern,
                    action,
                    scope,
                    channel_id,
                    replacement,
                    enabled,
                    priority,
                    created_at,
                    updated_at,
                )| ContentFilterRule {
                    id,
                    name,
                    pattern_type,
                    pattern,
                    action,
                    scope,
                    channel_id,
                    replacement,
                    enabled,
                    priority,
                    created_at,
                    updated_at,
                },
            )
            .collect())
    }

    async fn create_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError> {
        query(
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
        query(
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
        query("DELETE FROM content_filter_rules WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("Failed to delete filter rule: {}", e)))?;
        Ok(())
    }

    // ── Health Probe Results ─────────────────────────────────────────

    async fn channel_usage_24h(
        &self,
    ) -> Result<Vec<(String, String, u64, u64, f64, f64)>, DbError> {
        let rows = query_as::<_, (String, String, i64, i64, f64, f64)>(
            "SELECT channel_id, model, COUNT(*)::bigint, SUM(CASE WHEN success THEN 1 ELSE 0 END)::bigint, COALESCE(AVG(latency_ms)::float8, 0), COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms)::float8, 0)
             FROM billing_events
             WHERE timestamp::timestamptz >= NOW() - INTERVAL '1 day'
             GROUP BY channel_id, model ORDER BY COUNT(*) DESC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to query channel usage: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|(ch, m, req, suc, avg, p95)| (ch, m, req as u64, suc as u64, avg, p95))
            .collect())
    }

    async fn recent_request_paths(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, String, Option<i64>, u64, bool)>, DbError> {
        let rows = query_as::<_, (String, String, String, Option<i64>, i64, bool)>(
            "SELECT timestamp, model, channel_id, endpoint_id, latency_ms, success FROM billing_events ORDER BY timestamp DESC LIMIT $1"
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to query recent paths: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|(ts, m, ch, eid, lat, suc)| (ts, m, ch, eid, lat as u64, suc))
            .collect())
    }

    async fn routing_flow_snapshot(
        &self,
        hours: u32,
    ) -> Result<Vec<(String, String, Option<i64>, u64)>, DbError> {
        use Row;
        let since = (chrono::Utc::now() - chrono::Duration::hours(hours as i64))
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string();
        let rows = query("SELECT model, channel_id, endpoint_id, COUNT(*)::bigint FROM billing_events WHERE \"timestamp\"::timestamp >= $1::timestamp GROUP BY model, channel_id, endpoint_id")
            .bind(&since).fetch_all(&self.pool).await.map_err(|e| DbError(format!("routing_flow_snapshot: {}", e)))?;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.try_get::<String, _>(0).unwrap_or_default(),
                    r.try_get::<String, _>(1).unwrap_or_default(),
                    r.try_get::<Option<i64>, _>(2).unwrap_or(None),
                    r.try_get::<i64, _>(3).unwrap_or(0) as u64,
                )
            })
            .collect())
    }

    async fn routing_history_buckets(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<super::RoutingHistoryBucket>, DbError> {
        use Row;
        let rows = query(
            "SELECT
                CASE WHEN (EXTRACT(EPOCH FROM $2::timestamp - $1::timestamp)) < 172800
                  THEN date_trunc('hour', \"timestamp\"::timestamp)::text
                  ELSE date_trunc('day',  \"timestamp\"::timestamp)::text
                END AS bucket,
                channel_id,
                COUNT(*)::bigint AS requests,
                SUM(CASE WHEN success THEN 1 ELSE 0 END)::bigint AS successes,
                AVG(latency_ms)::float8 AS avg_latency
             FROM billing_events
             WHERE \"timestamp\"::timestamp >= $1::timestamp
               AND \"timestamp\"::timestamp <= $2::timestamp
               AND ($3::text IS NULL OR model = $3)
             GROUP BY bucket, channel_id
             ORDER BY bucket ASC",
        )
        .bind(start)
        .bind(end)
        .bind(model)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("routing_history_buckets: {}", e)))?;
        Ok(rows
            .iter()
            .map(|r| super::RoutingHistoryBucket {
                bucket: r.try_get::<String, _>(0).unwrap_or_default(),
                channel_id: r.try_get::<String, _>(1).unwrap_or_default(),
                endpoint_id: None,
                requests: r.try_get::<i64, _>(2).unwrap_or(0) as u64,
                successes: r.try_get::<i64, _>(3).unwrap_or(0) as u64,
                avg_latency: r.try_get::<f64, _>(4).unwrap_or(0.0),
            })
            .collect())
    }

    async fn routing_history_endpoint_stats(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<super::RoutingEndpointStat>, DbError> {
        use Row;
        let rows = query(
            "SELECT channel_id,
                    COUNT(*)::bigint AS requests,
                    SUM(CASE WHEN success THEN 1 ELSE 0 END)::bigint AS successes,
                    AVG(latency_ms)::float8 AS avg_latency,
                    COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms), 0)::float8 AS p95_latency
             FROM billing_events
             WHERE \"timestamp\"::timestamp >= $1::timestamp
               AND \"timestamp\"::timestamp <= $2::timestamp
               AND ($3::text IS NULL OR model = $3)
             GROUP BY channel_id
             ORDER BY requests DESC",
        )
        .bind(start).bind(end).bind(model)
        .fetch_all(&self.pool).await
        .map_err(|e| DbError(format!("routing_history_endpoint_stats: {}", e)))?;
        Ok(rows
            .iter()
            .map(|r| super::RoutingEndpointStat {
                channel_id: r.try_get::<String, _>(0).unwrap_or_default(),
                endpoint_id: None,
                requests: r.try_get::<i64, _>(1).unwrap_or(0) as u64,
                successes: r.try_get::<i64, _>(2).unwrap_or(0) as u64,
                avg_latency: r.try_get::<f64, _>(3).unwrap_or(0.0),
                p95_latency: r.try_get::<f64, _>(4).unwrap_or(0.0),
            })
            .collect())
    }

    async fn routing_history_endpoint_details(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<(String, Option<i64>, Option<String>, u64, u64, f64, f64)>, DbError> {
        use Row;
        let rows = query(
            "SELECT ul.channel_id, ul.endpoint_id, e.url,
                    COUNT(*)::bigint, SUM(CASE WHEN ul.success THEN 1 ELSE 0 END)::bigint,
                    AVG(ul.latency_ms)::float8,
                    COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY ul.latency_ms),0)::float8
             FROM billing_events ul LEFT JOIN endpoints e ON e.id=ul.endpoint_id
             WHERE \"ul\".\"timestamp\"::timestamp>=$1::timestamp AND \"ul\".\"timestamp\"::timestamp<=$2::timestamp
               AND ($3::text IS NULL OR ul.model=$3)
             GROUP BY ul.channel_id, ul.endpoint_id, e.url ORDER BY ul.channel_id, COUNT(*) DESC",
        ).bind(start).bind(end).bind(model).fetch_all(&self.pool).await
        .map_err(|e| DbError(format!("routing_history_endpoint_details: {}", e)))?;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.try_get::<String, _>(0).unwrap_or_default(),
                    r.try_get::<Option<i64>, _>(1).unwrap_or(None),
                    r.try_get::<Option<String>, _>(2).unwrap_or(None),
                    r.try_get::<i64, _>(3).unwrap_or(0) as u64,
                    r.try_get::<i64, _>(4).unwrap_or(0) as u64,
                    r.try_get::<f64, _>(5).unwrap_or(0.0),
                    r.try_get::<f64, _>(6).unwrap_or(0.0),
                )
            })
            .collect())
    }

    // ── Announcements ──────────────────────────────────────────────────

    async fn list_announcements(&self) -> Result<Vec<AnnouncementRow>, DbError> {
        let rows = query(
            "SELECT id, title, content, created_by, created_at, updated_at, published \
             FROM announcements ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_announcements: {e}")))?;
        Ok(rows.iter().map(map_announcement_row).collect())
    }

    async fn list_published_announcements(&self) -> Result<Vec<AnnouncementRow>, DbError> {
        let rows = query(
            "SELECT id, title, content, created_by, created_at, updated_at, published \
             FROM announcements WHERE published = true ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_published_announcements: {e}")))?;
        Ok(rows.iter().map(map_announcement_row).collect())
    }

    async fn get_announcement(&self, id: &str) -> Result<Option<AnnouncementRow>, DbError> {
        let row = query(
            "SELECT id, title, content, created_by, created_at, updated_at, published \
             FROM announcements WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError(format!("get_announcement: {e}")))?;
        Ok(row.as_ref().map(map_announcement_row))
    }

    async fn create_announcement(&self, a: &AnnouncementRow) -> Result<(), DbError> {
        query(
            "INSERT INTO announcements (id, title, content, created_by, created_at, updated_at, published) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&a.id)
        .bind(&a.title)
        .bind(&a.content)
        .bind(&a.created_by)
        .bind(&a.created_at)
        .bind(&a.updated_at)
        .bind(a.published)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("create_announcement: {e}")))?;
        Ok(())
    }

    async fn update_announcement(&self, a: &AnnouncementRow) -> Result<(), DbError> {
        query(
            "UPDATE announcements SET title = $1, content = $2, updated_at = $3, published = $4 \
             WHERE id = $5",
        )
        .bind(&a.title)
        .bind(&a.content)
        .bind(&a.updated_at)
        .bind(a.published)
        .bind(&a.id)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("update_announcement: {e}")))?;
        Ok(())
    }

    async fn delete_announcement(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM announcements WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("delete_announcement: {e}")))?;
        Ok(())
    }

    // ── Casbin Policies ──────────────────────────────────────────────────

    async fn casbin_list_policies(
        &self,
    ) -> Result<Vec<(String, String, String, String, String, String, String)>, DbError> {
        let rows = query("SELECT ptype, v0, v1, v2, v3, v4, v5 FROM casbin_policies ORDER BY id")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError(format!("casbin_list_policies: {e}")))?;
        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.try_get::<String, _>(0).unwrap_or_default(),
                    r.try_get::<String, _>(1).unwrap_or_default(),
                    r.try_get::<String, _>(2).unwrap_or_default(),
                    r.try_get::<String, _>(3).unwrap_or_default(),
                    r.try_get::<String, _>(4).unwrap_or_default(),
                    r.try_get::<String, _>(5).unwrap_or_default(),
                    r.try_get::<String, _>(6).unwrap_or_default(),
                )
            })
            .collect())
    }

    async fn casbin_add_policy(
        &self,
        ptype: &str,
        v0: &str,
        v1: &str,
        v2: &str,
        v3: &str,
        v4: &str,
        v5: &str,
    ) -> Result<(), DbError> {
        query(
            "INSERT INTO casbin_policies (ptype, v0, v1, v2, v3, v4, v5) VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (ptype, v0, v1, v2, v3, v4, v5) DO NOTHING",
        )
        .bind(ptype)
        .bind(v0)
        .bind(v1)
        .bind(v2)
        .bind(v3)
        .bind(v4)
        .bind(v5)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("casbin_add_policy: {e}")))?;
        Ok(())
    }

    async fn casbin_remove_policy(&self, ptype: &str, v0: &str, v1: &str) -> Result<(), DbError> {
        query("DELETE FROM casbin_policies WHERE ptype = $1 AND v0 = $2 AND v1 = $3")
            .bind(ptype)
            .bind(v0)
            .bind(v1)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("casbin_remove_policy: {e}")))?;
        Ok(())
    }

    async fn get_balances_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, Decimal)>, DbError> {
        let rows = query_as::<_, (String, f64, f64)>(
            "SELECT id, balance, frozen FROM users LIMIT $1 OFFSET $2",
        )
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, b, f)| {
                (
                    id,
                    Decimal::try_from(b).unwrap_or(Decimal::ZERO),
                    Decimal::try_from(f).unwrap_or(Decimal::ZERO),
                )
            })
            .collect())
    }

    // ── Batch Operations ────────────────────────────────────────────────

    async fn batch_insert_usage_with_billing(
        &self,
        batch: &[UsageRecord],
        billing_enabled: bool,
    ) -> Result<Vec<(String, Decimal, Decimal)>, DbError> {
        let mut tx = self.pool.begin().await?;
        let mut deductions: Vec<(String, Decimal, Decimal)> = Vec::new();

        for record in batch {
            let (prompt_price, completion_price, cache_read_price, cache_write_price) = {
                // Lookup pricing within transaction
                let result = query_as::<_, (f64, f64, f64, f64)>(
                    "SELECT prompt_price, completion_price, cache_read_price, cache_write_price FROM models WHERE name = $1",
                )
                .bind(&record.model)
                .fetch_optional(&mut *tx)
                .await;

                match result {
                    Ok(Some(p)) => p,
                    _ => {
                        // Fallback to pattern matching
                        let rows = query_as::<_, (f64, f64, f64, f64, String)>(
                            "SELECT prompt_price, completion_price, cache_read_price, cache_write_price, model_pattern FROM models",
                        )
                        .fetch_all(&mut *tx)
                        .await
                        .unwrap_or_default();

                        let mut found = (0.0, 0.0, 0.0, 0.0);
                        for (p, c, cr, cw, pattern) in rows {
                            if pattern.ends_with('*') {
                                let prefix = &pattern[..pattern.len() - 1];
                                if record.model.starts_with(prefix) {
                                    found = (p, c, cr, cw);
                                    break;
                                }
                            }
                            if pattern == record.model {
                                found = (p, c, cr, cw);
                                break;
                            }
                        }
                        found
                    }
                }
            };

            // Compute cost_amount (always — used for observability even when billing is off)
            let cost_amount = compute_cost_amount(
                record.prompt_tokens,
                record.completion_tokens,
                record.cache_hit_input_tokens,
                prompt_price,
                completion_price,
                cache_read_price,
                cache_write_price,
            );

            // Insert only billing metadata into PostgreSQL.
            let account_type = record
                .account_type
                .clone()
                .unwrap_or_else(|| "user".to_string());
            let billing_inserted = query(
                "INSERT INTO billing_events (\
                 timestamp, request_id, user_id, user_name, channel_id, model, \
                 prompt_tokens, completion_tokens, total_tokens, latency_ms, cache_hit_input_tokens, \
                 prompt_price, completion_price, cache_read_price, cost_amount, \
                 api_key_name, api_format, stream, client_ip, endpoint_id, \
                 request_body, response_body, reasoning_body, original_model, \
                 success, status_code, team_id, account_type, activity_status, charge_source, priced_cost_amount) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, \
                 $12, $13, $14, $15, $16, $17, $18, $19, $20, \
                 $21, $22, $23, $24, $25, $26, $27, $28, $29, $30, $31) \
                 ON CONFLICT (request_id) DO NOTHING",
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
            .bind(record.cache_hit_input_tokens as i64)
            .bind(prompt_price)
            .bind(completion_price)
            .bind(cache_read_price)
            .bind(cost_amount)
            .bind(&record.api_key_name)
            .bind(&record.api_format)
            .bind(record.stream)
            .bind(&record.client_ip)
            .bind(record.endpoint_id)
            .bind(Option::<String>::None)
            .bind(Option::<String>::None)
            .bind(Option::<String>::None)
            .bind(&record.original_model)
            .bind(record.success)
            .bind(record.status_code as i32)
            .bind(&record.team_id)
            .bind(&account_type)
            .bind(if record.status_code == 499 { "interrupted" } else if record.success { "success" } else { "failed" })
            .bind(if cost_amount > 0.0 { "wallet" } else { "unknown" })
            .bind(cost_amount)
            .execute(&mut *tx)
            .await?;
            if billing_inserted.rows_affected() == 0 {
                // A replay is never chargeable, but it may still carry metadata
                // missing from an earlier partial write. Keep that repair
                // separate from the insert result so it cannot re-enter billing.
                query(
                    "UPDATE billing_events
                     SET user_name = COALESCE(NULLIF($1, ''), user_name),
                         api_key_name = COALESCE(NULLIF($2, ''), api_key_name),
                         priced_cost_amount = CASE WHEN priced_cost_amount = 0 THEN $3 ELSE priced_cost_amount END
                     WHERE request_id = $4",
                )
                .bind(&record.user_name)
                .bind(&record.api_key_name)
                .bind(cost_amount)
                .bind(&record.request_id)
                .execute(&mut *tx)
                .await?;
                continue;
            }

            // Requests using reserve+settle already performed their authoritative
            // package/wallet charge synchronously. The background usage writer only
            // persists the billing fact and must never debit the wallet again.
            // It also rewrites cost_amount to the actual wallet fallback amount so
            // package-covered requests display zero monetary usage in billing views.
            let has_token_reservation: bool = query_scalar(
                "SELECT EXISTS(SELECT 1 FROM token_request_reservations WHERE request_id = $1)",
            )
            .bind(&record.request_id)
            .fetch_one(&mut *tx)
            .await?;
            if has_token_reservation {
                query(
                    "UPDATE billing_events be
                     SET reservation_id = r.id,
                         package_grant_id = r.package_grant_id,
                         accounting_mode = r.accounting_mode,
                         package_units = COALESCE(r.actual_package_units, 0),
                         wallet_amount = COALESCE(r.actual_wallet_amount, 0),
                         cost_amount = COALESCE(r.actual_wallet_amount, 0),
                         activity_status = CASE WHEN be.status_code = 499 THEN 'interrupted' WHEN be.success = false THEN 'failed' WHEN r.actual_package_units > 0 OR r.actual_wallet_amount > 0 THEN 'success' ELSE be.activity_status END,
                         status_reason = COALESCE(r.reason, be.status_reason),
                         charge_source = CASE WHEN r.billing_payment_mode = 'postpaid' AND r.actual_package_units > 0 THEN 'postpaid_package' WHEN r.billing_payment_mode = 'postpaid' THEN 'postpaid' WHEN r.actual_package_units > 0 AND r.actual_wallet_amount > 0 THEN 'package_and_wallet' WHEN r.actual_package_units > 0 THEN 'package' WHEN r.actual_wallet_amount > 0 THEN 'wallet' ELSE be.charge_source END,
                         api_key_name = COALESCE(NULLIF(r.api_key_name, ''), be.api_key_name),
                         billing_group_id = COALESCE(NULLIF(r.billing_group_id, ''), be.billing_group_id),
                         billing_group_name = COALESCE(NULLIF(r.billing_group_name, ''), be.billing_group_name),
                         billing_payment_mode = COALESCE(NULLIF(r.billing_payment_mode, ''), be.billing_payment_mode),
                         priced_cost_amount = COALESCE(NULLIF(be.priced_cost_amount, 0), r.estimated_priced_cost_amount, be.cost_amount)
                     FROM token_request_reservations r
                     WHERE be.request_id = r.request_id AND be.request_id = $1",
                )
                .bind(&record.request_id)
                .execute(&mut *tx)
                .await?;
                continue;
            }

            if billing_enabled && cost_amount > 0.0 {
                // Team-scoped records charge the team wallet; personal records
                // charge the user wallet. The personal path is preserved
                // verbatim so existing behavior is unchanged.
                if let Some(team_id) = &record.team_id {
                    let (balance, frozen): (f64, f64) = query_as(
                        "SELECT balance, frozen FROM team_wallets WHERE team_id = $1 FOR UPDATE",
                    )
                    .bind(team_id)
                    .fetch_one(&mut *tx)
                    .await?;

                    let reserved: f64 = query_scalar(
                        "SELECT token_wallet_reserved FROM team_wallets WHERE team_id = $1",
                    )
                    .bind(team_id)
                    .fetch_one(&mut *tx)
                    .await?;
                    let spendable = balance - frozen - reserved;
                    if spendable < cost_amount {
                        return Err(DbError("insufficient team wallet balance".to_string()));
                    }

                    let new_balance = balance - cost_amount;
                    query(
                        "UPDATE team_wallets SET balance = $1, updated_at = $2 WHERE team_id = $3",
                    )
                    .bind(new_balance)
                    .bind(chrono::Utc::now().to_rfc3339())
                    .bind(team_id)
                    .execute(&mut *tx)
                    .await?;

                    let now = chrono::Utc::now().to_rfc3339();
                    query(
                        "INSERT INTO wallet_transactions (id, user_id, type, amount, \
                         balance_before, balance_after, method, status, note, created_at, \
                         team_id, account_type) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                    )
                    .bind(uuid::Uuid::new_v4().to_string())
                    .bind(&record.user_id)
                    .bind("deduction")
                    .bind(-cost_amount)
                    .bind(balance)
                    .bind(new_balance)
                    .bind("usage")
                    .bind("completed")
                    .bind(format!("Usage: {}", record.model))
                    .bind(&now)
                    .bind(team_id)
                    .bind("team")
                    .execute(&mut *tx)
                    .await?;

                    deductions.push((
                        team_id.clone(),
                        Decimal::try_from(new_balance).unwrap_or(Decimal::ZERO),
                        Decimal::try_from(frozen).unwrap_or(Decimal::ZERO),
                    ));
                } else {
                    let (balance, frozen, reserved): (f64, f64, f64) =
                        query_as("SELECT balance, frozen, token_wallet_reserved FROM users WHERE id = $1 FOR UPDATE")
                            .bind(&record.user_id)
                            .fetch_one(&mut *tx)
                            .await?;
                    let spendable = balance - frozen - reserved;
                    if spendable < cost_amount {
                        return Err(DbError("insufficient wallet balance".to_string()));
                    }
                    if spendable < cost_amount {
                        tracing::warn!(
                            user_id = &record.user_id,
                            balance,
                            frozen,
                            cost_amount,
                            "Insufficient balance — skipping deduction"
                        );
                        continue;
                    }

                    let new_balance = balance - cost_amount;
                    query("UPDATE users SET balance = $1 WHERE id = $2")
                        .bind(new_balance)
                        .bind(&record.user_id)
                        .execute(&mut *tx)
                        .await?;

                    let now = chrono::Utc::now().to_rfc3339();
                    query(
                        "INSERT INTO wallet_transactions (id, user_id, type, amount, \
                         balance_before, balance_after, method, status, note, created_at, \
                         team_id, account_type) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                    )
                    .bind(uuid::Uuid::new_v4().to_string())
                    .bind(&record.user_id)
                    .bind("deduction")
                    .bind(-cost_amount)
                    .bind(balance)
                    .bind(new_balance)
                    .bind("usage")
                    .bind("completed")
                    .bind(format!("Usage: {}", record.model))
                    .bind(&now)
                    .bind(Option::<String>::None)
                    .bind("user")
                    .execute(&mut *tx)
                    .await?;

                    deductions.push((
                        record.user_id.clone(),
                        Decimal::try_from(new_balance).unwrap_or(Decimal::ZERO),
                        Decimal::try_from(frozen).unwrap_or(Decimal::ZERO),
                    ));
                }
            }
        }

        tx.commit().await?;
        Ok(deductions)
    }

    // ── Token resource packages ──────────────────────────────────────────

    async fn create_token_package_plan(
        &self,
        id: &str,
        code: &str,
        name: &str,
        accounting_mode: &str,
        display_token_amount: i64,
        total_units: i64,
        input_credit_factor: Decimal,
        output_credit_factor: Decimal,
        cache_credit_factor: Decimal,
        exhaustion_policy: &str,
        priority: i32,
        validity_days: Option<i32>,
        created_by: &str,
    ) -> Result<crate::domain::token_package::TokenPackagePlanRow, DbError> {
        if display_token_amount <= 0 || total_units <= 0 {
            return Err(DbError("package amounts must be positive".to_string()));
        }
        if !matches!(accounting_mode, "raw_tokens" | "standardized_credits") {
            return Err(DbError("unsupported accounting mode".to_string()));
        }
        if !matches!(exhaustion_policy, "package_then_wallet" | "package_only") {
            return Err(DbError("unsupported exhaustion policy".to_string()));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let row = query(
            "INSERT INTO token_package_plans
             (id, code, name, accounting_mode, display_token_amount, total_units,
              input_credit_factor, output_credit_factor, cache_credit_factor,
              exhaustion_policy, priority, validity_days, status, created_by, created_at, updated_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'active',$13,$14,$14)
             RETURNING id, code, name, accounting_mode, display_token_amount, total_units,
                       input_credit_factor::text, output_credit_factor::text, cache_credit_factor::text,
                       exhaustion_policy, priority, validity_days, status, created_at, updated_at",
        )
        .bind(id).bind(code).bind(name).bind(accounting_mode).bind(display_token_amount)
        .bind(total_units)
        .bind(input_credit_factor.to_f64().unwrap_or(1.0))
        .bind(output_credit_factor.to_f64().unwrap_or(1.0))
        .bind(cache_credit_factor.to_f64().unwrap_or(0.0))
        .bind(exhaustion_policy).bind(priority).bind(validity_days).bind(created_by).bind(&now)
        .fetch_one(&self.pool)
        .await?;
        Ok(crate::domain::token_package::TokenPackagePlanRow {
            id: row.try_get(0)?,
            code: row.try_get(1)?,
            name: row.try_get(2)?,
            accounting_mode: row.try_get::<String, _>(3)?.parse().map_err(DbError)?,
            display_token_amount: row.try_get::<i64, _>(4)?.max(0) as u64,
            total_units: row.try_get::<i64, _>(5)?.max(0) as u64,
            input_credit_factor: Decimal::try_from(row.try_get::<f64, _>(6)?)
                .unwrap_or(Decimal::ONE),
            output_credit_factor: Decimal::try_from(row.try_get::<f64, _>(7)?)
                .unwrap_or(Decimal::ONE),
            cache_credit_factor: Decimal::try_from(row.try_get::<f64, _>(8)?)
                .unwrap_or(Decimal::ZERO),
            exhaustion_policy: row.try_get::<String, _>(9)?.parse().map_err(DbError)?,
            priority: row.try_get(10)?,
            validity_days: row.try_get(11)?,
            status: row.try_get(12)?,
            created_at: row.try_get(13)?,
            updated_at: row.try_get(14)?,
        })
    }

    async fn delete_token_package_plan(&self, plan_id: &str) -> Result<(), DbError> {
        let active_grants: i64 = query_scalar(
            "SELECT COUNT(*) FROM token_package_grants WHERE plan_id = $1 AND status = 'active'",
        )
        .bind(plan_id)
        .fetch_one(&self.pool)
        .await?;
        if active_grants > 0 {
            return Err(DbError(
                "cannot delete a plan with active grants".to_string(),
            ));
        }
        let deleted = query("DELETE FROM token_package_plans WHERE id = $1")
            .bind(plan_id)
            .execute(&self.pool)
            .await?;
        if deleted.rows_affected() == 0 {
            return Err(DbError("token package plan not found".to_string()));
        }
        Ok(())
    }

    async fn revoke_token_package_grant(&self, grant_id: &str) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let status = query_scalar::<_, String>(
            "SELECT status FROM token_package_grants WHERE id = $1 FOR UPDATE",
        )
        .bind(grant_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError("token package grant not found".to_string()))?;
        if status != "active" {
            return Err(DbError(format!(
                "token package grant cannot be revoked from status '{status}'"
            )));
        }
        let updated = query(
            "UPDATE token_package_grants
             SET status = 'revoked', updated_at = $1
             WHERE id = $2 AND status = 'active'",
        )
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(grant_id)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(DbError(
                "token package grant was changed concurrently".to_string(),
            ));
        }
        tx.commit().await?;
        Ok(())
    }

    async fn list_token_package_plans(
        &self,
    ) -> Result<Vec<crate::domain::token_package::TokenPackagePlanRow>, DbError> {
        let rows = query(
            "SELECT id, code, name, accounting_mode, display_token_amount, total_units,
                    input_credit_factor::text, output_credit_factor::text, cache_credit_factor::text,
                    exhaustion_policy, priority, validity_days, status, created_at, updated_at
             FROM token_package_plans ORDER BY priority DESC, created_at DESC, id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(crate::domain::token_package::TokenPackagePlanRow {
                    id: row.try_get(0)?,
                    code: row.try_get(1)?,
                    name: row.try_get(2)?,
                    accounting_mode: row.try_get::<String, _>(3)?.parse().map_err(DbError)?,
                    display_token_amount: row.try_get::<i64, _>(4)?.max(0) as u64,
                    total_units: row.try_get::<i64, _>(5)?.max(0) as u64,
                    input_credit_factor: row
                        .try_get::<String, _>(6)?
                        .parse()
                        .unwrap_or(Decimal::ONE),
                    output_credit_factor: row
                        .try_get::<String, _>(7)?
                        .parse()
                        .unwrap_or(Decimal::ONE),
                    cache_credit_factor: row
                        .try_get::<String, _>(8)?
                        .parse()
                        .unwrap_or(Decimal::ZERO),
                    exhaustion_policy: row.try_get::<String, _>(9)?.parse().map_err(DbError)?,
                    priority: row.try_get(10)?,
                    validity_days: row.try_get(11)?,
                    status: row.try_get(12)?,
                    created_at: row.try_get(13)?,
                    updated_at: row.try_get(14)?,
                })
            })
            .collect()
    }

    async fn list_token_package_grants(
        &self,
        user_id: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<Vec<crate::domain::token_package::TokenPackageGrantRow>, DbError> {
        let rows = match (user_id, team_id) {
            (Some(user_id), None) => query(
                "SELECT g.id, g.plan_id, p.code, p.name, g.user_id, g.team_id, g.accounting_mode, \
                 g.display_token_amount, g.total_units, g.consumed_units, g.reserved_units, \
                 g.priority, g.exhaustion_policy, g.status, g.expires_at, g.created_at \
                 FROM token_package_grants g JOIN token_package_plans p ON p.id = g.plan_id WHERE g.user_id = $1 ORDER BY g.priority DESC, g.expires_at NULLS LAST, g.created_at, g.id",
            )
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?,
            (None, Some(team_id)) => query(
                "SELECT g.id, g.plan_id, p.code, p.name, g.user_id, g.team_id, g.accounting_mode, \
                 g.display_token_amount, g.total_units, g.consumed_units, g.reserved_units, \
                 g.priority, g.exhaustion_policy, g.status, g.expires_at, g.created_at \
                 FROM token_package_grants g JOIN token_package_plans p ON p.id = g.plan_id WHERE g.team_id = $1 ORDER BY g.priority DESC, g.expires_at NULLS LAST, g.created_at, g.id",
            )
            .bind(team_id)
            .fetch_all(&self.pool)
            .await?,
            (None, None) => query(
                "SELECT g.id, g.plan_id, p.code, p.name, g.user_id, g.team_id, g.accounting_mode, \
                 g.display_token_amount, g.total_units, g.consumed_units, g.reserved_units, \
                 g.priority, g.exhaustion_policy, g.status, g.expires_at, g.created_at \
                 FROM token_package_grants g JOIN token_package_plans p ON p.id = g.plan_id \
                 ORDER BY g.created_at DESC, g.id",
            )
            .fetch_all(&self.pool)
            .await?,
            _ => return Ok(Vec::new()),
        };
        rows.into_iter()
            .map(|row| {
                let mode = row
                    .try_get::<String, _>(6)
                    .unwrap_or_else(|_| "raw_tokens".to_string())
                    .parse()
                    .map_err(|e: String| DbError(e))?;
                let policy = row
                    .try_get::<String, _>(12)
                    .unwrap_or_else(|_| "package_then_wallet".to_string())
                    .parse()
                    .map_err(|e: String| DbError(e))?;
                Ok(crate::domain::token_package::TokenPackageGrantRow {
                    id: row.try_get(0)?,
                    plan_id: row.try_get(1)?,
                    plan_code: row.try_get(2)?,
                    plan_name: row.try_get(3)?,
                    user_id: row.try_get(4)?,
                    team_id: row.try_get(5)?,
                    accounting_mode: mode,
                    display_token_amount: row.try_get::<i64, _>(7)?.max(0) as u64,
                    total_units: row.try_get::<i64, _>(8)?.max(0) as u64,
                    consumed_units: row.try_get::<i64, _>(9)?.max(0) as u64,
                    reserved_units: row.try_get::<i64, _>(10)?.max(0) as u64,
                    priority: row.try_get(11)?,
                    exhaustion_policy: policy,
                    status: row.try_get(13)?,
                    expires_at: row.try_get(14)?,
                    created_at: row.try_get(15)?,
                })
            })
            .collect()
    }

    async fn create_token_package_grant(
        &self,
        grant_id: &str,
        plan_id: &str,
        user_id: Option<&str>,
        team_id: Option<&str>,
        source: &str,
        note: &str,
        expires_at: Option<&str>,
    ) -> Result<crate::domain::token_package::TokenPackageGrantRow, DbError> {
        if user_id.is_none() == team_id.is_none() {
            return Err(DbError(
                "exactly one of user_id or team_id is required".to_string(),
            ));
        }
        let now = chrono::Utc::now().to_rfc3339();
        let row = query(
            "INSERT INTO token_package_grants
             (id, plan_id, user_id, team_id, accounting_mode, display_token_amount,
              total_units, priority, exhaustion_policy, source, note, expires_at, created_at, updated_at)
             SELECT $1, p.id, $3, $4, p.accounting_mode, p.display_token_amount,
                    p.total_units, p.priority, p.exhaustion_policy, $5, $6, $7, $8, $8
             FROM token_package_plans p WHERE p.id = $2 AND p.status = 'active'
             RETURNING id, plan_id, user_id, team_id, accounting_mode, display_token_amount,
                       total_units, consumed_units, reserved_units, priority, exhaustion_policy,
                       status, expires_at, created_at",
        )
        .bind(grant_id)
        .bind(plan_id)
        .bind(user_id)
        .bind(team_id)
        .bind(source)
        .bind(note)
        .bind(expires_at)
        .bind(&now)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError("token package plan not found or inactive".to_string()))?;
        let mode = row.try_get::<String, _>(4)?.parse().map_err(DbError)?;
        let policy = row.try_get::<String, _>(10)?.parse().map_err(DbError)?;
        let (plan_code, plan_name): (String, String) =
            query_as("SELECT code, name FROM token_package_plans WHERE id = $1")
                .bind(plan_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(crate::domain::token_package::TokenPackageGrantRow {
            id: row.try_get(0)?,
            plan_id: row.try_get(1)?,
            plan_code,
            plan_name,
            user_id: row.try_get(2)?,
            team_id: row.try_get(3)?,
            accounting_mode: mode,
            display_token_amount: row.try_get::<i64, _>(5)?.max(0) as u64,
            total_units: row.try_get::<i64, _>(6)?.max(0) as u64,
            consumed_units: row.try_get::<i64, _>(7)?.max(0) as u64,
            reserved_units: row.try_get::<i64, _>(8)?.max(0) as u64,
            priority: row.try_get(9)?,
            exhaustion_policy: policy,
            status: row.try_get(11)?,
            expires_at: row.try_get(12)?,
            created_at: row.try_get(13)?,
        })
    }

    async fn reserve_token_request(
        &self,
        request: &crate::domain::token_package::TokenReservationRequest,
    ) -> Result<crate::domain::token_package::TokenReservationHandle, DbError> {
        let mut tx = self.pool.begin().await?;
        if let Some(row) = query(
            "SELECT id, package_grant_id, accounting_mode, reserved_package_units, reserved_total_units, reserved_wallet_amount, factor_snapshot, request_fingerprint, state, prompt_price, completion_price, cache_read_price, cache_write_price \
             FROM token_request_reservations WHERE request_id = $1 FOR UPDATE",
        )
        .bind(&request.request_id)
        .fetch_optional(&mut *tx)
        .await?
        {
            let stored_fingerprint = row.try_get::<String, _>(7).unwrap_or_default();
            if !stored_fingerprint.is_empty() && stored_fingerprint != request.request_fingerprint {
                return Err(DbError("request_id already belongs to different request".to_string()));
            }
            if row.try_get::<String, _>(8).unwrap_or_default() != "reserved" {
                return Err(DbError("request_id has already been finalized".to_string()));
            }
            let mode = row
                .try_get::<Option<String>, _>(2)?
                .and_then(|v| v.parse().ok());
            return Ok(crate::domain::token_package::TokenReservationHandle {
                reservation_id: row.try_get(0)?,
                request_id: request.request_id.clone(),
                package_grant_id: row.try_get(1)?,
                accounting_mode: mode,
                input_factor: serde_json::from_str::<serde_json::Value>(&row.try_get::<String, _>(6).unwrap_or_default())
                    .ok().and_then(|v| v["input_factor"].as_f64()).and_then(|v| Decimal::try_from(v).ok()).unwrap_or(Decimal::ONE),
                output_factor: serde_json::from_str::<serde_json::Value>(&row.try_get::<String, _>(6).unwrap_or_default())
                    .ok().and_then(|v| v["output_factor"].as_f64()).and_then(|v| Decimal::try_from(v).ok()).unwrap_or(Decimal::ONE),
                cache_factor: serde_json::from_str::<serde_json::Value>(&row.try_get::<String, _>(6).unwrap_or_default())
                    .ok().and_then(|v| v["cache_factor"].as_f64()).and_then(|v| Decimal::try_from(v).ok()).unwrap_or(Decimal::ZERO),
                reserved_package_units: row.try_get::<i64, _>(3)?.max(0) as u64,
                reserved_total_units: row.try_get::<i64, _>(4)?.max(0) as u64,
                reserved_wallet_amount: Decimal::try_from(row.try_get::<f64, _>(5)?)
                    .unwrap_or(Decimal::ZERO),
                prompt_price: Decimal::try_from(row.try_get::<f64, _>(9).unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                completion_price: Decimal::try_from(row.try_get::<f64, _>(10).unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                cache_read_price: Decimal::try_from(row.try_get::<f64, _>(11).unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                cache_write_price: Decimal::try_from(row.try_get::<f64, _>(12).unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                billing_group_id: request.billing_group_id.clone(),
                billing_group_name: request.billing_group_name.clone(),
                billing_payment_mode: request.billing_payment_mode,
            });
        }

        let requested_raw = request
            .prompt_tokens
            .saturating_add(request.completion_tokens);
        let grant = if let Some(team_id) = request.team_id.as_deref() {
            query(
                "SELECT id, accounting_mode, exhaustion_policy, total_units, consumed_units, reserved_units \
                 FROM token_package_grants \
                 WHERE team_id = $1 AND status = 'active' AND (expires_at IS NULL OR expires_at > $2) \
                   AND total_units - consumed_units - reserved_units > 0 \
                 ORDER BY priority DESC, expires_at NULLS LAST, created_at, id LIMIT 1 FOR UPDATE",
            )
            .bind(team_id)
            .bind(&request.expires_at)
            .fetch_optional(&mut *tx)
            .await?
        } else {
            query(
                "SELECT id, accounting_mode, exhaustion_policy, total_units, consumed_units, reserved_units \
                 FROM token_package_grants \
                 WHERE user_id = $1 AND status = 'active' AND (expires_at IS NULL OR expires_at > $2) \
                   AND total_units - consumed_units - reserved_units > 0 \
                 ORDER BY priority DESC, expires_at NULLS LAST, created_at, id LIMIT 1 FOR UPDATE",
            )
            .bind(&request.user_id)
            .bind(&request.expires_at)
            .fetch_optional(&mut *tx)
            .await?
        };
        let available = grant
            .as_ref()
            .map(|row| {
                row.try_get::<i64, _>(3).unwrap_or(0)
                    - row.try_get::<i64, _>(4).unwrap_or(0)
                    - row.try_get::<i64, _>(5).unwrap_or(0)
            })
            .unwrap_or(0)
            .max(0) as u64;
        let grant_id = grant
            .as_ref()
            .map(|r| r.try_get::<String, _>(0))
            .transpose()?;
        let mode_text = grant.as_ref().and_then(|r| r.try_get::<String, _>(1).ok());
        let (input_factor, output_factor, cache_factor) = if let Some(id) = grant_id.as_deref() {
            query_as::<_, (f64, f64, f64)>(
                "SELECT input_factor, output_factor, cache_factor
                 FROM token_package_model_factors
                 WHERE plan_id = (SELECT plan_id FROM token_package_grants WHERE id = $1)
                   AND ($2 = model_pattern OR ($2 LIKE REPLACE(model_pattern, '*', '%')))
                 ORDER BY CASE WHEN $2 = model_pattern THEN 0 ELSE 1 END, model_pattern
                 LIMIT 1",
            )
            .bind(id)
            .bind(&request.model)
            .fetch_optional(&mut *tx)
            .await?
            .map(|(i, o, c)| {
                (
                    Decimal::try_from(i).unwrap_or(Decimal::ONE),
                    Decimal::try_from(o).unwrap_or(Decimal::ONE),
                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                )
            })
            .unwrap_or((Decimal::ONE, Decimal::ONE, Decimal::ZERO))
        } else {
            (Decimal::ONE, Decimal::ONE, Decimal::ZERO)
        };
        let mode = mode_text.as_deref().and_then(|v| v.parse().ok());
        let requested = if mode
            == Some(crate::domain::token_package::TokenAccountingMode::StandardizedCredits)
        {
            let usage = crate::domain::token_package::TokenUsage {
                prompt_tokens: request.prompt_tokens,
                completion_tokens: request.completion_tokens,
                cache_hit_input_tokens: request.cache_hit_input_tokens,
            };
            usage.standardized_credits(input_factor, output_factor, cache_factor)
        } else {
            requested_raw
        };
        let package_units = requested.min(available);
        let wallet_units = requested.saturating_sub(package_units);
        let policy = grant
            .as_ref()
            .and_then(|r| r.try_get::<String, _>(2).ok())
            .unwrap_or_else(|| "package_then_wallet".to_string());
        if wallet_units > 0
            && policy == "package_only"
            && request.billing_payment_mode == BillingPaymentMode::Prepaid
        {
            return Err(DbError("token package quota exceeded".to_string()));
        }
        let wallet_hold = if request.billing_payment_mode == BillingPaymentMode::Postpaid {
            Decimal::ZERO
        } else if wallet_units > 0 {
            request.estimated_wallet_amount.max(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };
        if wallet_hold > Decimal::ZERO {
            let amount = wallet_hold.to_f64().unwrap_or(f64::MAX);
            if let Some(team_id) = request.team_id.as_deref() {
                let updated = query(
                    "UPDATE team_wallets SET token_wallet_reserved = token_wallet_reserved + $1, updated_at = $2
                     WHERE team_id = $3 AND balance - frozen - token_wallet_reserved >= $1",
                )
                .bind(amount)
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(team_id)
                .execute(&mut *tx)
                .await?;
                if updated.rows_affected() != 1 {
                    return Err(DbError("insufficient team wallet balance".to_string()));
                }
            } else {
                let updated = query(
                    "UPDATE users SET token_wallet_reserved = token_wallet_reserved + $1
                     WHERE id = $2 AND balance - frozen - token_wallet_reserved >= $1",
                )
                .bind(amount)
                .bind(&request.user_id)
                .execute(&mut *tx)
                .await?;
                if updated.rows_affected() != 1 {
                    return Err(DbError("insufficient wallet balance".to_string()));
                }
            }
        }
        if let Some(id) = grant_id.as_deref() {
            query("UPDATE token_package_grants SET reserved_units = reserved_units + $1, updated_at = $2 WHERE id = $3")
                .bind(package_units as i64)
                .bind(&request.expires_at)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        let reservation_id = uuid::Uuid::new_v4().to_string();
        query(
            "INSERT INTO token_request_reservations \
             (id, request_id, request_fingerprint, user_id, user_name, api_key_name, team_id, account_type, package_grant_id, model, accounting_mode, \
              reserved_prompt_tokens, reserved_completion_tokens, reserved_package_units, reserved_total_units, reserved_wallet_amount, \
              factor_snapshot, prompt_price, completion_price, cache_read_price, cache_write_price, estimated_priced_cost_amount, billing_group_id, billing_group_name, billing_payment_mode, expires_at, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)",

        )
        .bind(&reservation_id)
        .bind(&request.request_id)
        .bind(&request.request_fingerprint)
        .bind(&request.user_id)
        .bind(&request.user_name)
        .bind(&request.api_key_name)
        .bind(&request.team_id)
        .bind(if request.team_id.is_some() { "team" } else { "user" })
        .bind(&grant_id)
        .bind(&request.model)
        .bind(&mode_text)
        .bind(request.prompt_tokens as i64)
        .bind(request.completion_tokens as i64)
        .bind(package_units as i64)
        .bind(requested as i64)
        .bind(wallet_hold.to_f64().unwrap_or(f64::MAX))
        .bind(serde_json::json!({
            "input_factor": input_factor,
            "output_factor": output_factor,
            "cache_factor": cache_factor,
        }).to_string())
        .bind(request.estimated_priced_cost_amount.to_f64().unwrap_or(0.0))
        .bind(&request.billing_group_id)
        .bind(&request.billing_group_name)
        .bind(request.billing_payment_mode.as_str())
        .bind(&request.expires_at)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(crate::domain::token_package::TokenReservationHandle {
            reservation_id,
            request_id: request.request_id.clone(),
            package_grant_id: grant_id,
            accounting_mode: mode_text.and_then(|v| v.parse().ok()),
            input_factor,
            output_factor,
            cache_factor,
            reserved_package_units: package_units,
            reserved_total_units: requested,
            reserved_wallet_amount: wallet_hold,
            prompt_price: request.prompt_price,
            completion_price: request.completion_price,
            cache_read_price: request.cache_read_price,
            cache_write_price: request.cache_write_price,
            billing_group_id: request.billing_group_id.clone(),
            billing_group_name: request.billing_group_name.clone(),
            billing_payment_mode: request.billing_payment_mode,
        })
    }

    async fn settle_token_request(
        &self,
        settlement: &crate::domain::token_package::TokenSettlementRequest,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let row = query("SELECT state, package_grant_id, reserved_package_units, reserved_total_units, reserved_prompt_tokens, reserved_completion_tokens, user_id, team_id, reserved_wallet_amount, accounting_mode, factor_snapshot, billing_payment_mode, billing_group_id, billing_group_name, request_id FROM token_request_reservations WHERE id = $1 FOR UPDATE")
            .bind(&settlement.reservation_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| DbError("token reservation not found".to_string()))?;
        if row.try_get::<String, _>(0)? != "reserved" {
            return Ok(());
        }
        let grant_id = row.try_get::<Option<String>, _>(1)?;
        let reserved = row.try_get::<i64, _>(2)?.max(0) as u64;
        let reserved_total = row.try_get::<i64, _>(3)?.max(0) as u64;
        let user_id = row.try_get::<String, _>(6)?;
        let team_id = row.try_get::<Option<String>, _>(7)?;
        let reserved_wallet = row.try_get::<f64, _>(8)?;
        let request_id = row.try_get::<String, _>(14)?;
        let billing_payment_mode = row
            .try_get::<String, _>(11)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(BillingPaymentMode::Prepaid);
        let accounting_mode = row.try_get::<Option<String>, _>(9)?;
        let factor_snapshot = row
            .try_get::<String, _>(10)
            .unwrap_or_else(|_| "{}".to_string());
        let (input_factor, output_factor, cache_factor) =
            serde_json::from_str::<serde_json::Value>(&factor_snapshot)
                .ok()
                .map(|v| {
                    (
                        v["input_factor"]
                            .as_f64()
                            .and_then(|value| Decimal::try_from(value).ok())
                            .unwrap_or(Decimal::ONE),
                        v["output_factor"]
                            .as_f64()
                            .and_then(|value| Decimal::try_from(value).ok())
                            .unwrap_or(Decimal::ONE),
                        v["cache_factor"]
                            .as_f64()
                            .and_then(|value| Decimal::try_from(value).ok())
                            .unwrap_or(Decimal::ZERO),
                    )
                })
                .unwrap_or((Decimal::ONE, Decimal::ONE, Decimal::ZERO));
        let actual_usage = crate::domain::token_package::TokenUsage {
            prompt_tokens: settlement.actual_prompt_tokens,
            completion_tokens: settlement.actual_completion_tokens,
            cache_hit_input_tokens: settlement.actual_cache_hit_input_tokens,
        };
        let actual_units = if accounting_mode.as_deref() == Some("standardized_credits") {
            actual_usage.standardized_credits(input_factor, output_factor, cache_factor)
        } else {
            actual_usage.raw_units()
        };
        let consumed = actual_units.min(reserved);
        if let Some(id) = grant_id.as_deref() {
            query("UPDATE token_package_grants SET consumed_units = consumed_units + $1, reserved_units = reserved_units - $2, updated_at = $3 WHERE id = $4")
                .bind(consumed as i64)
                .bind(reserved as i64)
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(id)
                .execute(&mut *tx)
                .await?;
            query("INSERT INTO token_package_ledger (id, package_grant_id, reservation_id, request_id, entry_type, units, created_at) VALUES ($1, $2, $3, $4, 'consume', $5, $6) ON CONFLICT DO NOTHING")
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(id)
                .bind(&settlement.reservation_id)
                .bind(&request_id)
                .bind(consumed as i64)
                .bind(chrono::Utc::now().to_rfc3339())
                .execute(&mut *tx)
                .await?;
        }
        let wallet_units = actual_units.saturating_sub(reserved);
        let wallet_amount = if billing_payment_mode == BillingPaymentMode::Postpaid
            || wallet_units == 0
            || reserved_total == 0
        {
            0.0
        } else {
            reserved_wallet * wallet_units.min(reserved_total) as f64 / reserved_total as f64
        };
        let now = chrono::Utc::now().to_rfc3339();
        let release_wallet = (reserved_wallet - wallet_amount).max(0.0);
        if release_wallet > 0.0 {
            if let Some(team_id) = team_id.as_deref() {
                query("UPDATE team_wallets SET token_wallet_reserved = GREATEST(0, token_wallet_reserved - $1), updated_at = $2 WHERE team_id = $3")
                    .bind(release_wallet)
                    .bind(chrono::Utc::now().to_rfc3339())
                    .bind(team_id)
                    .execute(&mut *tx)
                    .await?;
            } else {
                query("UPDATE users SET token_wallet_reserved = GREATEST(0, token_wallet_reserved - $1) WHERE id = $2")
                    .bind(release_wallet)
                    .bind(&user_id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        if billing_payment_mode == BillingPaymentMode::Prepaid && wallet_amount > 0.0 {
            let deduction = -wallet_amount;
            if let Some(team_id) = team_id.as_deref() {
                let row = query("SELECT balance FROM team_wallets WHERE team_id = $1 FOR UPDATE")
                    .bind(team_id)
                    .fetch_one(&mut *tx)
                    .await?;
                let balance = row.try_get::<f64, _>(0)?;
                query("UPDATE team_wallets SET balance = balance + $1, token_wallet_reserved = GREATEST(0, token_wallet_reserved - $2), updated_at = $3 WHERE team_id = $4")
                    .bind(deduction).bind(wallet_amount).bind(chrono::Utc::now().to_rfc3339()).bind(team_id)
                    .execute(&mut *tx).await?;
                query("INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at, team_id, account_type) VALUES ($1,$2,'deduction',$3,$4,$5,'token_package','completed',$6,$7,$8,'team')")
                    .bind(uuid::Uuid::new_v4().to_string()).bind(&user_id).bind(deduction).bind(balance).bind(balance + deduction)
                    .bind("Token package wallet fallback").bind(&now).bind(team_id).execute(&mut *tx).await?;
            } else {
                let row = query("SELECT balance FROM users WHERE id = $1 FOR UPDATE")
                    .bind(&user_id)
                    .fetch_one(&mut *tx)
                    .await?;
                let balance = row.try_get::<f64, _>(0)?;
                query("UPDATE users SET balance = balance + $1, token_wallet_reserved = GREATEST(0, token_wallet_reserved - $2) WHERE id = $3")
                    .bind(deduction).bind(wallet_amount).bind(&user_id)
                    .execute(&mut *tx).await?;
                query("INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at, team_id, account_type) VALUES ($1,$2,'deduction',$3,$4,$5,'token_package','completed',$6,$7,NULL,'user')")
                    .bind(uuid::Uuid::new_v4().to_string()).bind(&user_id).bind(deduction).bind(balance).bind(balance + deduction)
                    .bind("Token package wallet fallback").bind(&now).execute(&mut *tx).await?;
            }
        }
        query("UPDATE token_request_reservations SET actual_prompt_tokens = $1, actual_completion_tokens = $2, actual_package_units = $3, actual_wallet_amount = $4, state = $5, reason = $6, settled_at = $7 WHERE id = $8")
            .bind(settlement.actual_prompt_tokens as i64)
            .bind(settlement.actual_completion_tokens as i64)
            .bind(consumed as i64)
            .bind(wallet_amount)
            .bind(if actual_units > 0 { "settled" } else { "released" })
            .bind(&settlement.reason)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&settlement.reservation_id)
            .execute(&mut *tx)
            .await?;
        query(
            "INSERT INTO billing_events (request_id, user_id, user_name, channel_id, model,
             prompt_tokens, completion_tokens, total_tokens, cache_hit_input_tokens,
             cost_amount, success, status_code, timestamp, reservation_id, package_grant_id,
             accounting_mode, package_units, wallet_amount, team_id, account_type,
             api_key_name, billing_group_id, billing_group_name, billing_payment_mode,
             activity_status, charge_source, priced_cost_amount)
             SELECT r.request_id, r.user_id, COALESCE(NULLIF(r.user_name, ''), u.name), '', r.model,
                    COALESCE(r.actual_prompt_tokens, 0), COALESCE(r.actual_completion_tokens, 0),
                    COALESCE(r.actual_prompt_tokens, 0) + COALESCE(r.actual_completion_tokens, 0),
                    COALESCE($1, 0), COALESCE(r.actual_wallet_amount, 0), $2, $3, COALESCE(r.created_at, $4),
                    r.id, r.package_grant_id, r.accounting_mode, COALESCE(r.actual_package_units, 0),
                    COALESCE(r.actual_wallet_amount, 0), r.team_id, r.account_type, NULLIF(r.api_key_name, ''),
                    r.billing_group_id, r.billing_group_name, r.billing_payment_mode,
                    CASE WHEN $3 = 499 THEN 'interrupted' WHEN $3 >= 400 OR $2 = false THEN 'failed' ELSE 'success' END,
                    CASE WHEN r.billing_payment_mode = 'postpaid' AND COALESCE(r.actual_package_units, 0) > 0 THEN 'postpaid_package'
                         WHEN r.billing_payment_mode = 'postpaid' THEN 'postpaid'
                         WHEN COALESCE(r.actual_package_units, 0) > 0 AND COALESCE(r.actual_wallet_amount, 0) > 0 THEN 'package_and_wallet'
                         WHEN COALESCE(r.actual_package_units, 0) > 0 THEN 'package'
                         WHEN COALESCE(r.actual_wallet_amount, 0) > 0 THEN 'wallet'
                         ELSE 'none' END,
                    COALESCE(r.estimated_priced_cost_amount, 0)
             FROM token_request_reservations r JOIN users u ON u.id = r.user_id
             WHERE r.id = $5
             ON CONFLICT (request_id) DO UPDATE SET
               prompt_tokens = EXCLUDED.prompt_tokens,
               completion_tokens = EXCLUDED.completion_tokens,
               total_tokens = EXCLUDED.total_tokens,
               cache_hit_input_tokens = EXCLUDED.cache_hit_input_tokens,
               cost_amount = EXCLUDED.cost_amount,
               success = EXCLUDED.success,
               status_code = EXCLUDED.status_code,
               reservation_id = EXCLUDED.reservation_id,
               package_grant_id = EXCLUDED.package_grant_id,
               accounting_mode = EXCLUDED.accounting_mode,
               package_units = EXCLUDED.package_units,
               wallet_amount = EXCLUDED.wallet_amount,
               activity_status = EXCLUDED.activity_status,
               charge_source = EXCLUDED.charge_source,
               billing_group_id = COALESCE(EXCLUDED.billing_group_id, billing_events.billing_group_id),
               billing_group_name = COALESCE(EXCLUDED.billing_group_name, billing_events.billing_group_name),
               billing_payment_mode = COALESCE(EXCLUDED.billing_payment_mode, billing_events.billing_payment_mode),
               priced_cost_amount = EXCLUDED.priced_cost_amount,
               user_name = COALESCE(NULLIF(EXCLUDED.user_name, ''), billing_events.user_name),
               api_key_name = COALESCE(NULLIF(EXCLUDED.api_key_name, ''), billing_events.api_key_name)",
        )
        .bind(settlement.actual_cache_hit_input_tokens as i64)
        .bind(settlement.success)
        .bind(settlement.status_code as i32)
        .bind(now)
        .bind(&settlement.reservation_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn settle_released_token_request(
        &self,
        request_id: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        cache_hit_input_tokens: u64,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let row = query_as::<_, (String, String, i64, Option<String>)>(
            "SELECT id, state, reserved_package_units, package_grant_id
             FROM token_request_reservations WHERE request_id = $1 FOR UPDATE",
        )
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((reservation_id, state, reserved, package_grant_id)) = row else {
            return Ok(());
        };
        if state != "released" || package_grant_id.is_none() || reserved <= 0 {
            return Ok(());
        }
        let actual = prompt_tokens.saturating_add(completion_tokens);
        let consume = (actual as i64).min(reserved).max(0);
        if consume == 0 {
            return Ok(());
        }
        let grant_id = package_grant_id.unwrap();
        query("UPDATE token_package_grants SET consumed_units = consumed_units + $1, reserved_units = GREATEST(0, reserved_units - $1), updated_at = $2 WHERE id = $3")
            .bind(consume)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&grant_id)
            .execute(&mut *tx)
            .await?;
        query("UPDATE token_request_reservations SET actual_prompt_tokens = $1, actual_completion_tokens = $2, actual_package_units = $3, state = 'settled', reason = 'client disconnected after partial output', settled_at = $4 WHERE id = $5 AND state = 'released'")
            .bind(prompt_tokens as i64)
            .bind(completion_tokens as i64)
            .bind(consume)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&reservation_id)
            .execute(&mut *tx)
            .await?;
        query("INSERT INTO token_package_ledger (id, package_grant_id, reservation_id, request_id, entry_type, units, display_tokens, prompt_tokens, completion_tokens, credits, created_at, note) VALUES ($1,$2,$3,$4,'consume',$5,$6,$7,$8,$9,$10,$11) ON CONFLICT DO NOTHING")
            .bind(uuid::Uuid::new_v4().to_string()).bind(&grant_id).bind(&reservation_id).bind(request_id)
            .bind(consume).bind(prompt_tokens.saturating_add(completion_tokens) as i64).bind(prompt_tokens as i64).bind(completion_tokens as i64).bind(consume)
            .bind(chrono::Utc::now().to_rfc3339()).bind("partial stream output")
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn token_request_billing_amount(
        &self,
        request_id: &str,
    ) -> Result<Option<(bool, Decimal, String)>, DbError> {
        let row = query_as::<_, (Option<String>, Option<f64>, Option<String>)>(
            "SELECT package_grant_id, actual_wallet_amount, billing_payment_mode
             FROM token_request_reservations
             WHERE request_id = $1 AND state IN ('reserved', 'settled', 'released', 'expired')
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(request_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(package_id, amount, mode)| {
            (
                package_id.is_some(),
                Decimal::try_from(amount.unwrap_or(0.0)).unwrap_or(Decimal::ZERO),
                mode.unwrap_or_else(|| "prepaid".to_string()),
            )
        }))
    }

    async fn reclaim_expired_token_reservations(&self, limit: usize) -> Result<usize, DbError> {
        if limit == 0 {
            return Ok(0);
        }

        // Selection is deliberately narrow. `release_token_request` performs
        // the state transition under a row lock, so concurrent settlement or
        // another instance can only make a candidate a harmless no-op.
        let reservation_ids = query_scalar::<_, String>(
            "SELECT id
             FROM token_request_reservations
             WHERE state = 'reserved'
               AND expires_at::timestamptz <= NOW()
             ORDER BY expires_at, id
             LIMIT $1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut reclaimed = 0;
        for reservation_id in reservation_ids {
            self.release_token_request(&reservation_id, "reservation expired")
                .await?;
            reclaimed += 1;
        }
        Ok(reclaimed)
    }

    async fn release_token_request(
        &self,
        reservation_id: &str,
        reason: &str,
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        let row = query("SELECT state, package_grant_id, reserved_package_units, reserved_prompt_tokens, reserved_completion_tokens, user_id, team_id, reserved_wallet_amount, request_id FROM token_request_reservations WHERE id = $1 FOR UPDATE")
            .bind(reservation_id)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            return Ok(());
        };
        if row.try_get::<String, _>(0)? != "reserved" {
            return Ok(());
        }
        let reserved_prompt = row.try_get::<i64, _>(3)?.max(0) as u64;
        let reserved_completion = row.try_get::<i64, _>(4)?.max(0) as u64;
        let user_id = row.try_get::<String, _>(5)?;
        let team_id = row.try_get::<Option<String>, _>(6)?;
        let reserved_wallet = row.try_get::<f64, _>(7)?;
        let request_id = row.try_get::<String, _>(8)?;
        if reserved_wallet > 0.0 {
            if let Some(team_id) = team_id.as_deref() {
                query("UPDATE team_wallets SET token_wallet_reserved = GREATEST(0, token_wallet_reserved - $1), updated_at = $2 WHERE team_id = $3")
                    .bind(reserved_wallet).bind(chrono::Utc::now().to_rfc3339()).bind(team_id)
                    .execute(&mut *tx).await?;
            } else {
                query("UPDATE users SET token_wallet_reserved = GREATEST(0, token_wallet_reserved - $1) WHERE id = $2")
                    .bind(reserved_wallet).bind(&user_id).execute(&mut *tx).await?;
            }
        }
        if let Some(grant_id) = row.try_get::<Option<String>, _>(1)? {
            let reserved = row.try_get::<i64, _>(2)?.max(0);
            query("UPDATE token_package_grants SET reserved_units = GREATEST(0, reserved_units - $1), updated_at = $2 WHERE id = $3")
                .bind(reserved)
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(&grant_id)
                .execute(&mut *tx)
                .await?;
            query("INSERT INTO token_package_ledger (id, package_grant_id, reservation_id, request_id, entry_type, units, created_at, note) VALUES ($1, $2, $3, $4, 'release', $5, $6, $7) ON CONFLICT DO NOTHING")
                .bind(uuid::Uuid::new_v4().to_string())
                .bind(&grant_id)
                .bind(reservation_id)
                .bind(&request_id)
                .bind(-reserved)
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(reason)
                .execute(&mut *tx)
                .await?;
        }
        query("UPDATE token_request_reservations SET state = 'released', reason = $1, settled_at = $2 WHERE id = $3")
            .bind(reason)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(reservation_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    // ── Teams ─────────────────────────────────────────────────────────────

    async fn create_team(&self, team: &Team, owner_id: &str) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        query(
            "INSERT INTO teams (id, name, owner_id, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&team.id)
        .bind(&team.name)
        .bind(&team.owner_id)
        .bind(team.created_at.to_rfc3339())
        .bind(team.updated_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        // Owner is a member with role 'owner'.
        query(
            "INSERT INTO team_members (team_id, user_id, role, joined_at) VALUES ($1, $2, 'owner', $3)",
        )
        .bind(&team.id)
        .bind(owner_id)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
        // Initial zero-balance team wallet.
        query(
            "INSERT INTO team_wallets (team_id, balance, frozen, updated_at) \
             VALUES ($1, 0.0, 0.0, $2)",
        )
        .bind(&team.id)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn get_team(&self, team_id: &str) -> Result<Option<Team>, DbError> {
        let rows = query_as::<_, (String, String, String, String, String)>(
            "SELECT id, name, owner_id, created_at, updated_at FROM teams WHERE id = $1",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("get_team: {e}")))?;
        Ok(rows
            .first()
            .map(|(id, name, owner_id, created_at, updated_at)| Team {
                id: id.clone(),
                name: name.clone(),
                owner_id: owner_id.clone(),
                created_at: chrono::DateTime::parse_from_rfc3339(created_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(updated_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            }))
    }

    async fn list_teams_for_user(&self, user_id: &str) -> Result<Vec<Team>, DbError> {
        let rows = query_as::<_, (String, String, String, String, String)>(
            "SELECT t.id, t.name, t.owner_id, t.created_at, t.updated_at \
             FROM teams t \
             JOIN team_members m ON m.team_id = t.id \
             WHERE m.user_id = $1 \
             ORDER BY t.created_at",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_teams_for_user: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(id, name, owner_id, created_at, updated_at)| Team {
                id,
                name,
                owner_id,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }

    async fn list_all_teams(&self) -> Result<Vec<Team>, DbError> {
        let rows = query_as::<_, (String, String, String, String, String)>(
            "SELECT id, name, owner_id, created_at, updated_at FROM teams ORDER BY created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_all_teams: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(id, name, owner_id, created_at, updated_at)| Team {
                id,
                name,
                owner_id,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }

    async fn update_team(&self, team_id: &str, name: &str) -> Result<(), DbError> {
        query("UPDATE teams SET name = $1, updated_at = $2 WHERE id = $3")
            .bind(name)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(team_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("update_team: {e}")))?;
        Ok(())
    }

    async fn delete_team(&self, team_id: &str) -> Result<(), DbError> {
        query("DELETE FROM teams WHERE id = $1")
            .bind(team_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("delete_team: {e}")))?;
        Ok(())
    }

    async fn add_team_member(
        &self,
        team_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<(), DbError> {
        query(
            "INSERT INTO team_members (team_id, user_id, role, joined_at) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (team_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        )
        .bind(team_id)
        .bind(user_id)
        .bind(role)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("add_team_member: {e}")))?;
        Ok(())
    }

    async fn remove_team_member(&self, team_id: &str, user_id: &str) -> Result<(), DbError> {
        // Refuse to remove the owner.
        let (current_role,): (String,) =
            query_as("SELECT role FROM team_members WHERE team_id = $1 AND user_id = $2")
                .bind(team_id)
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError(format!("remove_team_member: {e}")))?;
        if current_role == "owner" {
            return Err(DbError("Cannot remove the team owner".to_string()));
        }
        query("DELETE FROM team_members WHERE team_id = $1 AND user_id = $2")
            .bind(team_id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("remove_team_member: {e}")))?;
        Ok(())
    }

    async fn set_team_member_role(
        &self,
        team_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<(), DbError> {
        // Forbid changing the owner's role away from owner.
        let (current_role,): (String,) =
            query_as("SELECT role FROM team_members WHERE team_id = $1 AND user_id = $2")
                .bind(team_id)
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .map_err(|e| DbError(format!("set_team_member_role: {e}")))?;
        if current_role == "owner" && role != "owner" {
            return Err(DbError("Cannot demote the team owner".to_string()));
        }
        query("UPDATE team_members SET role = $1 WHERE team_id = $2 AND user_id = $3")
            .bind(role)
            .bind(team_id)
            .bind(user_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("set_team_member_role: {e}")))?;
        Ok(())
    }

    async fn list_team_members(&self, team_id: &str) -> Result<Vec<TeamMember>, DbError> {
        let rows = query_as::<_, (String, String, String, String)>(
            "SELECT team_id, user_id, role, joined_at FROM team_members \
             WHERE team_id = $1 ORDER BY joined_at",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("list_team_members: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(team_id, user_id, role, joined_at)| TeamMember {
                team_id,
                user_id,
                role,
                joined_at: chrono::DateTime::parse_from_rfc3339(&joined_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }

    async fn get_team_member(
        &self,
        team_id: &str,
        user_id: &str,
    ) -> Result<Option<TeamMember>, DbError> {
        let rows = query_as::<_, (String, String, String, String)>(
            "SELECT team_id, user_id, role, joined_at FROM team_members \
             WHERE team_id = $1 AND user_id = $2",
        )
        .bind(team_id)
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("get_team_member: {e}")))?;
        Ok(rows
            .first()
            .map(|(team_id, user_id, role, joined_at)| TeamMember {
                team_id: team_id.clone(),
                user_id: user_id.clone(),
                role: role.clone(),
                joined_at: chrono::DateTime::parse_from_rfc3339(joined_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            }))
    }

    async fn get_team_wallet(&self, team_id: &str) -> Result<Option<(f64, f64)>, DbError> {
        let rows = query_as::<_, (f64, f64)>(
            "SELECT balance, frozen FROM team_wallets WHERE team_id = $1",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("get_team_wallet: {e}")))?;
        Ok(rows.first().map(|(balance, frozen)| (*balance, *frozen)))
    }

    async fn all_team_members(&self) -> Result<Vec<TeamMember>, DbError> {
        let rows = query_as::<_, (String, String, String, String)>(
            "SELECT team_id, user_id, role, joined_at FROM team_members ORDER BY team_id, user_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("all_team_members: {e}")))?;
        Ok(rows
            .into_iter()
            .map(|(team_id, user_id, role, joined_at)| TeamMember {
                team_id,
                user_id,
                role,
                joined_at: chrono::DateTime::parse_from_rfc3339(&joined_at)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            })
            .collect())
    }

    async fn add_team_wallet_balance(&self, team_id: &str, amount: f64) -> Result<(), DbError> {
        query("UPDATE team_wallets SET balance = balance + $1, updated_at = $2 WHERE team_id = $3")
            .bind(amount)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(team_id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("add_team_wallet_balance: {e}")))?;
        Ok(())
    }

    async fn list_team_wallet_transactions(
        &self,
        team_id: &str,
        page: usize,
        size: usize,
    ) -> Result<(Vec<WalletTransactionRow>, usize), DbError> {
        let offset = (page.saturating_sub(1)) * size;
        let rows = query(
            "SELECT id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at \
             FROM wallet_transactions WHERE team_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
        )
        .bind(team_id)
        .bind(size as i64)
        .bind(offset as i64)
        .fetch_all(&self.pool)
        .await?;
        let items = rows
            .iter()
            .map(|r| WalletTransactionRow {
                id: r.get(0),
                user_id: r.get(1),
                tx_type: r.get(2),
                amount: Decimal::try_from(r.get::<f64, _>(3)).unwrap_or(Decimal::ZERO),
                balance_before: Decimal::try_from(r.get::<f64, _>(4)).unwrap_or(Decimal::ZERO),
                balance_after: Decimal::try_from(r.get::<f64, _>(5)).unwrap_or(Decimal::ZERO),
                method: r.get(6),
                status: r.get(7),
                note: r.get(8),
                created_at: r.get(9),
            })
            .collect::<Vec<_>>();
        let (count,): (i64,) =
            query_as("SELECT COUNT(*) FROM wallet_transactions WHERE team_id = $1")
                .bind(team_id)
                .fetch_one(&self.pool)
                .await?;
        Ok((items, count as usize))
    }

    async fn list_team_api_keys(&self, team_id: &str) -> Result<Vec<ApiKey>, DbError> {
        let rows = query(
            "SELECT key, user_id, name, enabled, expires_at, spend_limit, allowed_models, team_id, billing_group_id, billing_payment_mode \
             FROM api_keys WHERE team_id = $1 ORDER BY key",
        )
        .bind(team_id)
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
                    spend_limit: r
                        .get::<Option<f64>, _>(5)
                        .map(|v| Decimal::try_from(v).unwrap_or(Decimal::ZERO)),
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                    team_id: r.get(7),
                    scopes: None,
                    billing_group_id: r.get::<Option<String>, _>(8).unwrap_or_default(),
                    billing_payment_mode: r
                        .get::<Option<String>, _>(9)
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(BillingPaymentMode::Prepaid),
                }
            })
            .collect())
    }

    async fn list_team_rules(&self, team_id: &str) -> Result<Vec<RoutingRule>, DbError> {
        let rows = query(
            "SELECT id, name, scope, user_id, source_model, target_model, \
             channel_id, upstream_model, priority, enabled, description, \
             created_at, updated_at, team_id \
             FROM routing_rules WHERE scope='user' AND team_id=$1 \
             ORDER BY priority, name",
        )
        .bind(team_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| RoutingRule {
                id: r.get(0),
                name: r.get(1),
                scope: r.get(2),
                user_id: r.get(3),
                source_model: r.get(4),
                target_model: r.get(5),
                channel_id: r.get(6),
                upstream_model: r.get(7),
                priority: r.get(8),
                enabled: r.get(9),
                description: r.get(10),
                created_at: r.get(11),
                updated_at: r.get(12),
                team_id: r.get(13),
            })
            .collect())
    }

    // ── SSO Configs ─────────────────────────────────────────────────────────

    async fn list_sso_configs(&self) -> Result<Vec<SsoConfigRow>, DbError> {
        let rows = query_as::<
            _,
            (
                String,
                Option<String>,
                String,
                String,
                String,
                String,
                String,
                bool,
                bool,
                Option<String>,
                String,
                String,
                String,
            ),
        >(
            "SELECT id, team_id, provider_name, issuer_url, client_id, \
             client_secret_encrypted, redirect_url, enabled, auto_create_user, \
             domain_restrictions, default_role, created_at, updated_at \
             FROM sso_configs ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to list sso configs: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    team_id,
                    provider_name,
                    issuer_url,
                    client_id,
                    client_secret_encrypted,
                    redirect_url,
                    enabled,
                    auto_create_user,
                    domain_restrictions,
                    default_role,
                    created_at,
                    updated_at,
                )| SsoConfigRow {
                    id,
                    team_id,
                    provider_name,
                    issuer_url,
                    client_id,
                    client_secret_encrypted: Some(client_secret_encrypted),
                    redirect_url,
                    enabled,
                    auto_create_user,
                    domain_restrictions,
                    default_role,
                    created_at,
                    updated_at,
                },
            )
            .collect())
    }

    async fn get_sso_config(&self, id: &str) -> Result<Option<SsoConfigRow>, DbError> {
        let row: Option<(
            String,
            Option<String>,
            String,
            String,
            String,
            String,
            String,
            bool,
            bool,
            Option<String>,
            String,
            String,
            String,
        )> = query_as(
            "SELECT id, team_id, provider_name, issuer_url, client_id, \
             client_secret_encrypted, redirect_url, enabled, auto_create_user, \
             domain_restrictions, default_role, created_at, updated_at \
             FROM sso_configs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to get sso config: {}", e)))?;

        Ok(row.map(
            |(
                id,
                team_id,
                provider_name,
                issuer_url,
                client_id,
                client_secret_encrypted,
                redirect_url,
                enabled,
                auto_create_user,
                domain_restrictions,
                default_role,
                created_at,
                updated_at,
            )| SsoConfigRow {
                id,
                team_id,
                provider_name,
                issuer_url,
                client_id,
                client_secret_encrypted: Some(client_secret_encrypted),
                redirect_url,
                enabled,
                auto_create_user,
                domain_restrictions,
                default_role,
                created_at,
                updated_at,
            },
        ))
    }

    async fn get_sso_config_by_team(&self, team_id: &str) -> Result<Option<SsoConfigRow>, DbError> {
        let row: Option<(
            String,
            Option<String>,
            String,
            String,
            String,
            String,
            String,
            bool,
            bool,
            Option<String>,
            String,
            String,
            String,
        )> = query_as(
            "SELECT id, team_id, provider_name, issuer_url, client_id, \
             client_secret_encrypted, redirect_url, enabled, auto_create_user, \
             domain_restrictions, default_role, created_at, updated_at \
             FROM sso_configs WHERE team_id = $1",
        )
        .bind(team_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to get sso config by team: {}", e)))?;

        Ok(row.map(
            |(
                id,
                team_id,
                provider_name,
                issuer_url,
                client_id,
                client_secret_encrypted,
                redirect_url,
                enabled,
                auto_create_user,
                domain_restrictions,
                default_role,
                created_at,
                updated_at,
            )| SsoConfigRow {
                id,
                team_id,
                provider_name,
                issuer_url,
                client_id,
                client_secret_encrypted: Some(client_secret_encrypted),
                redirect_url,
                enabled,
                auto_create_user,
                domain_restrictions,
                default_role,
                created_at,
                updated_at,
            },
        ))
    }

    async fn create_sso_config(&self, config: &SsoConfigRow) -> Result<(), DbError> {
        query(
            "INSERT INTO sso_configs (id, team_id, provider_name, issuer_url, client_id, \
             client_secret_encrypted, redirect_url, enabled, auto_create_user, \
             domain_restrictions, default_role, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(&config.id)
        .bind(&config.team_id)
        .bind(&config.provider_name)
        .bind(&config.issuer_url)
        .bind(&config.client_id)
        .bind(&config.client_secret_encrypted)
        .bind(&config.redirect_url)
        .bind(config.enabled)
        .bind(config.auto_create_user)
        .bind(&config.domain_restrictions)
        .bind(&config.default_role)
        .bind(&config.created_at)
        .bind(&config.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to create sso config: {}", e)))?;
        Ok(())
    }

    async fn update_sso_config(&self, config: &SsoConfigRow) -> Result<(), DbError> {
        query(
            "UPDATE sso_configs SET team_id=$2, provider_name=$3, issuer_url=$4, \
             client_id=$5, client_secret_encrypted=$6, redirect_url=$7, \
             enabled=$8, auto_create_user=$9, domain_restrictions=$10, \
             default_role=$11, updated_at=$12 \
             WHERE id=$1",
        )
        .bind(&config.id)
        .bind(&config.team_id)
        .bind(&config.provider_name)
        .bind(&config.issuer_url)
        .bind(&config.client_id)
        .bind(&config.client_secret_encrypted)
        .bind(&config.redirect_url)
        .bind(config.enabled)
        .bind(config.auto_create_user)
        .bind(&config.domain_restrictions)
        .bind(&config.default_role)
        .bind(&config.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to update sso config: {}", e)))?;
        Ok(())
    }

    async fn delete_sso_config(&self, id: &str) -> Result<(), DbError> {
        query("DELETE FROM sso_configs WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError(format!("Failed to delete sso config: {}", e)))?;
        Ok(())
    }

    async fn list_sso_user_orgs(&self) -> Result<Vec<(String, String)>, DbError> {
        let rows = query_as::<_, (String, String)>(
            "SELECT user_id, orgs FROM sso_user_orgs ORDER BY user_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to list sso user orgs: {}", e)))?;
        Ok(rows)
    }

    async fn upsert_sso_user_orgs(&self, user_id: &str, orgs_json: &str) -> Result<(), DbError> {
        query(
            "INSERT INTO sso_user_orgs (user_id, orgs, updated_at) VALUES ($1, $2, now() AT TIME ZONE 'utc') \
             ON CONFLICT (user_id) DO UPDATE SET orgs = EXCLUDED.orgs, updated_at = now() AT TIME ZONE 'utc'",
        )
        .bind(user_id)
        .bind(orgs_json)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Failed to upsert sso user orgs: {}", e)))?;
        Ok(())
    }
}

impl PgBackend {
    fn apply_recharge_key_filters<'a>(
        builder: &mut QueryBuilder<'a, Postgres>,
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

/// Compute usage cost from token counts and per-1M prices.
/// Pure helper extracted from the billing path so it is unit-testable.
fn compute_cost_amount(
    prompt_tokens: u64,
    completion_tokens: u64,
    cache_hit_input_tokens: u64,
    prompt_price: f64,
    completion_price: f64,
    cache_read_price: f64,
    cache_write_price: f64,
) -> f64 {
    prompt_tokens as f64 / 1000000.0 * prompt_price
        + completion_tokens as f64 / 1000000.0 * completion_price
        + cache_hit_input_tokens as f64 / 1000000.0 * cache_read_price
}

/// Resolve which account a usage record charges.
/// Returns (account_id, account_type) where account_type is "user" or "team".
/// Personal records charge the user's wallet; team records charge the team wallet.
fn usage_account(record: &UsageRecord) -> (String, &'static str) {
    match &record.team_id {
        Some(team_id) => (team_id.clone(), "team"),
        None => (record.user_id.clone(), "user"),
    }
}

#[cfg(test)]
mod billing_tests {
    use super::{compute_cost_amount, usage_account};
    use crate::domain::usage::UsageRecord;

    fn record(user_id: &str, team_id: Option<&str>) -> UsageRecord {
        let mut r = UsageRecord {
            timestamp: String::new(),
            request_id: String::new(),
            user_id: user_id.to_string(),
            user_name: String::new(),
            channel_id: String::new(),
            model: String::new(),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            latency_ms: 0,
            status_code: 0,
            success: true,
            request_body: None,
            response_body: None,
            reasoning_body: None,
            api_key_name: None,
            api_format: String::new(),
            stream: false,
            cache_hit_input_tokens: 0,
            cache_write_tokens: 0,
            prompt_price: rust_decimal::Decimal::ZERO,
            completion_price: rust_decimal::Decimal::ZERO,
            cache_read_price: rust_decimal::Decimal::ZERO,
            cache_write_price: rust_decimal::Decimal::ZERO,
            client_ip: None,
            endpoint_id: None,
            endpoint_url: None,
            original_model: String::new(),
            team_id: team_id.map(|s| s.to_string()),
            ttft_ms: None,
            account_type: None,
            billing_payment_mode: None,
        };
        r.prompt_tokens = 1_000_000; // $1 at $1/1M
        r.prompt_price = rust_decimal::Decimal::ONE;
        r.completion_tokens = 2_000_000;
        r.completion_price = rust_decimal::Decimal::from(2);
        r.cache_hit_input_tokens = 500_000;
        r.cache_read_price = rust_decimal::Decimal::from(1);
        r
    }

    #[test]
    fn cost_amount_matches_tokens_times_prices() {
        let r = record("user-1", None);
        // 1M*$1 + 2M*$2 + 0.5M*$1 = 1 + 4 + 0.5
        let cost = compute_cost_amount(
            r.prompt_tokens,
            r.completion_tokens,
            r.cache_hit_input_tokens,
            1.0,
            2.0,
            1.0,
            0.0,
        );
        assert!((cost - 5.5).abs() < 1e-9, "expected 5.5, got {}", cost);
    }

    #[test]
    fn zero_cost_when_no_tokens() {
        let r = record("user-1", None);
        let cost = compute_cost_amount(0, 0, 0, 1.0, 1.0, 1.0, 0.0);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn personal_record_charges_user_wallet() {
        let r = record("user-1", None);
        let (account, ty) = usage_account(&r);
        assert_eq!(account, "user-1");
        assert_eq!(ty, "user");
    }

    #[test]
    fn team_record_charges_team_wallet() {
        let r = record("user-1", Some("team-9"));
        let (account, ty) = usage_account(&r);
        assert_eq!(account, "team-9");
        assert_eq!(ty, "team");
    }
}
