use super::*;
use async_trait::async_trait;

#[async_trait]
impl CoreBackend for PgBackend {
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
                expires_at TEXT,
                key_kind TEXT NOT NULL DEFAULT 'user' CHECK (key_kind IN ('user', 'platform'))
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

            CREATE TABLE IF NOT EXISTS management_api_keys (
                id TEXT PRIMARY KEY,
                key_hash TEXT NOT NULL UNIQUE,
                key_prefix TEXT NOT NULL,
                name TEXT NOT NULL DEFAULT '',
                enabled BOOLEAN NOT NULL DEFAULT true,
                created_by TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL,
                expires_at TEXT,
                last_used_at TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_management_api_keys_enabled
                ON management_api_keys(enabled, expires_at);
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
        add_col!(
            "ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS key_kind TEXT NOT NULL DEFAULT 'user'"
        );
        add_col!("DELETE FROM balancer_settings WHERE key = 'management_api_enabled'");
        add_col!("UPDATE api_keys SET key_kind = 'user' WHERE key_kind IS NULL OR key_kind NOT IN ('user', 'platform')");
        add_col!("ALTER TABLE api_keys DROP CONSTRAINT IF EXISTS api_keys_key_kind_check");
        add_col!("ALTER TABLE api_keys ADD CONSTRAINT api_keys_key_kind_check CHECK (key_kind IN ('user', 'platform'))");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS concurrency_limit BIGINT NOT NULL DEFAULT 2000");
        add_col!("ALTER TABLE users ADD COLUMN IF NOT EXISTS currency TEXT NOT NULL DEFAULT 'usd'");
        add_col!(
            "ALTER TABLE endpoints ADD COLUMN IF NOT EXISTS enabled BOOLEAN NOT NULL DEFAULT true"
        );
        add_col!(
            "ALTER TABLE endpoints ADD COLUMN IF NOT EXISTS full_url BOOLEAN NOT NULL DEFAULT false"
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
                payment_mode TEXT NOT NULL CHECK (payment_mode IN ('metered','prepaid')),\
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
        // The legacy database may still have the old check constraint. Drop it
        // before the one-time protocol migration below, then re-add the new
        // constraint after all billing tables and columns exist.
        let _ = raw_sql("ALTER TABLE billing_groups DROP CONSTRAINT IF EXISTS billing_groups_payment_mode_check")
            .execute(&self.pool)
            .await;
        raw_sql(
            "INSERT INTO billing_groups (id, name, payment_mode, status, is_default, created_by, created_at, updated_at)\
             VALUES ('billing-group-default-prepaid', '默认按量计费', 'metered', 'active', true, '', now()::text, now()::text)\
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration seed default billing group: {e}")))?;
        add_col!("ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS billing_group_id TEXT");
        add_col!("ALTER TABLE api_keys ADD COLUMN IF NOT EXISTS billing_payment_mode TEXT");
        add_col!("UPDATE api_keys SET billing_group_id = 'billing-group-default-prepaid' WHERE billing_group_id IS NULL OR billing_group_id = ''");
        add_col!("UPDATE api_keys SET billing_payment_mode = 'metered' WHERE billing_payment_mode IS NULL OR billing_payment_mode = ''");
        // Keep the built-in group name aligned with its actual metered behavior.
        add_col!("UPDATE billing_groups SET name = '默认按量计费' WHERE id = 'billing-group-default-prepaid' AND payment_mode = 'metered' AND name IN ('默认预付费', '默认按量计费')");
        // Repair current API-key bindings from the authoritative group mode.
        add_col!("UPDATE api_keys ak SET billing_payment_mode = g.payment_mode FROM billing_groups g WHERE ak.billing_group_id = g.id AND g.payment_mode IN ('metered', 'prepaid')");
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
                cache_write_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
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
                cache_write_price DOUBLE PRECISION NOT NULL DEFAULT 0.0,\
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
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS cache_write_price DOUBLE PRECISION NOT NULL DEFAULT 0.0",
        ] {
            let _ = raw_sql(alter).execute(&self.pool).await;
        }
        let _ = raw_sql(
            "INSERT INTO billing_events (\
                request_id, user_id, user_name, channel_id, model, \
                prompt_tokens, completion_tokens, total_tokens, latency_ms, cache_hit_input_tokens, \
                prompt_price, completion_price, cache_read_price, cache_write_price, cost_amount, \
                api_key_name, api_format, stream, client_ip, endpoint_id, \
                request_body, response_body, reasoning_body, original_model, \
                success, status_code, timestamp\
             ) \
             SELECT request_id, user_id, user_name, channel_id, model, \
                prompt_tokens, completion_tokens, total_tokens, latency_ms, cache_hit_input_tokens, \
                prompt_price, completion_price, cache_read_price, cache_write_price, cost_amount, \
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
                actual_cache_write_tokens BIGINT,
                actual_package_units BIGINT,
                actual_wallet_amount DOUBLE PRECISION,
                wallet_shortfall_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
                actual_priced_cost_amount DOUBLE PRECISION,
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
            "CREATE TABLE IF NOT EXISTS token_package_reservation_allocations (
                id TEXT PRIMARY KEY,
                reservation_id TEXT NOT NULL REFERENCES token_request_reservations(id) ON DELETE CASCADE,
                package_grant_id TEXT NOT NULL REFERENCES token_package_grants(id),
                reserved_units BIGINT NOT NULL DEFAULT 0 CHECK (reserved_units >= 0),
                consumed_units BIGINT NOT NULL DEFAULT 0 CHECK (consumed_units >= 0),
                created_at TEXT NOT NULL,
                UNIQUE (reservation_id, package_grant_id)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create token_package_reservation_allocations: {e}")))?;
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
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS token_settlement_receivables (
                id TEXT PRIMARY KEY,
                reservation_id TEXT NOT NULL UNIQUE REFERENCES token_request_reservations(id) ON DELETE CASCADE,
                request_id TEXT NOT NULL UNIQUE,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                team_id TEXT,
                account_type TEXT NOT NULL DEFAULT 'user',
                actual_prompt_tokens BIGINT,
                actual_completion_tokens BIGINT,
                actual_cache_hit_input_tokens BIGINT,
                actual_cache_write_tokens BIGINT,
                status_code INTEGER,
                success BOOLEAN,
                reason TEXT NOT NULL DEFAULT '',
                actual_priced_cost_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
                package_priced_cost_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
                wallet_due_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
                settled_wallet_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
                outstanding_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
                writeoff_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
                other_adjustment_amount DOUBLE PRECISION NOT NULL DEFAULT 0,
                state TEXT NOT NULL DEFAULT 'awaiting_actuals',
                attempts INTEGER NOT NULL DEFAULT 0,
                next_attempt_at TEXT,
                lease_until TEXT,
                last_error TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                settled_at TEXT
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create token_settlement_receivables: {e}")))?;
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS token_settlement_payments (
                id TEXT PRIMARY KEY,
                receivable_id TEXT NOT NULL REFERENCES token_settlement_receivables(id) ON DELETE CASCADE,
                reservation_id TEXT NOT NULL,
                request_id TEXT NOT NULL,
                payment_sequence BIGINT NOT NULL,
                payment_type TEXT NOT NULL DEFAULT 'recovery',
                idempotency_key TEXT NOT NULL,
                amount DOUBLE PRECISION NOT NULL,
                account_type TEXT NOT NULL,
                wallet_transaction_id TEXT,
                created_at TEXT NOT NULL,
                UNIQUE (receivable_id, payment_sequence),
                UNIQUE (idempotency_key)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create token_settlement_payments: {e}")))?;
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
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS actual_cache_write_tokens BIGINT",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS billing_group_id TEXT NOT NULL DEFAULT 'billing-group-default-prepaid'",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS billing_group_name TEXT NOT NULL DEFAULT '默认按量计费'",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS billing_payment_mode TEXT NOT NULL DEFAULT 'metered'",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS estimated_priced_cost_amount DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS actual_priced_cost_amount DOUBLE PRECISION",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS wallet_shortfall_amount DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE token_settlement_receivables ADD COLUMN IF NOT EXISTS attempts INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE token_settlement_receivables ADD COLUMN IF NOT EXISTS next_attempt_at TEXT",
            "ALTER TABLE token_settlement_receivables ADD COLUMN IF NOT EXISTS lease_until TEXT",
            "ALTER TABLE token_settlement_receivables ADD COLUMN IF NOT EXISTS last_error TEXT NOT NULL DEFAULT ''",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS settlement_state TEXT NOT NULL DEFAULT 'reserved'",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS settlement_state TEXT NOT NULL DEFAULT 'settled'",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS settled_amount DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS outstanding_amount DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS writeoff_amount DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS token_settlement_id TEXT",
            "ALTER TABLE token_settlement_payments ADD COLUMN IF NOT EXISTS payment_type TEXT NOT NULL DEFAULT 'recovery'",
            "UPDATE wallet_transactions SET method = 'token_settlement' WHERE method = 'token_package' AND note = 'Token package wallet fallback'",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS prompt_price DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS completion_price DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS cache_read_price DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE token_request_reservations ADD COLUMN IF NOT EXISTS cache_write_price DOUBLE PRECISION NOT NULL DEFAULT 0",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS billing_group_id TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS billing_group_name TEXT",
            "ALTER TABLE billing_events ADD COLUMN IF NOT EXISTS billing_payment_mode TEXT NOT NULL DEFAULT 'metered'",
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
            "CREATE INDEX IF NOT EXISTS idx_token_settlement_receivables_state ON token_settlement_receivables(state, next_attempt_at)",
            "CREATE INDEX IF NOT EXISTS idx_token_settlement_receivables_user ON token_settlement_receivables(user_id, state)",
            "CREATE INDEX IF NOT EXISTS idx_token_settlement_payments_receivable ON token_settlement_payments(receivable_id, payment_sequence)",
            "CREATE INDEX IF NOT EXISTS idx_token_ledger_grant ON token_package_ledger(package_grant_id, created_at)",
            "CREATE INDEX IF NOT EXISTS idx_billing_events_reservation ON billing_events(reservation_id)",
            "CREATE INDEX IF NOT EXISTS idx_billing_events_activity ON billing_events(user_id, timestamp DESC, activity_status, charge_source)",
        ] {
            let _ = raw_sql(index).execute(&self.pool).await;
        }
        // Convert legacy snapshots exactly once. The marker makes this
        // idempotent across restarts and prevents prepaid from drifting into
        // metered again.
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS billing_mode_migration_marker (
                id TEXT PRIMARY KEY,
                migrated_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await;
        let marker_inserted = raw_sql(
            "INSERT INTO billing_mode_migration_marker (id, migrated_at)
             VALUES ('legacy-prepaid-postpaid-v1', now()::text)
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&self.pool)
        .await
        .map(|r| r.rows_affected() == 1)
        .unwrap_or(false);
        if marker_inserted {
            let _ = raw_sql("UPDATE token_request_reservations SET billing_payment_mode = CASE WHEN billing_payment_mode = 'postpaid' THEN 'prepaid' WHEN billing_payment_mode = 'prepaid' THEN 'metered' ELSE billing_payment_mode END")
                .execute(&self.pool).await;
            let _ = raw_sql("UPDATE billing_events SET billing_payment_mode = CASE WHEN billing_payment_mode = 'postpaid' THEN 'prepaid' WHEN billing_payment_mode = 'prepaid' THEN 'metered' ELSE billing_payment_mode END")
                .execute(&self.pool).await;
        }
        let _ = raw_sql("ALTER TABLE billing_groups ADD CONSTRAINT billing_groups_payment_mode_check CHECK (payment_mode IN ('metered','prepaid'))")
            .execute(&self.pool)
            .await;
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

        // ── API Gateway routes（纯 API 网关业务配置，类似 Kong/APISIX）────
        // 数据面访问入口：/apigw/{path_prefix 剩余路径}。upstream_headers 存
        // 加密 JSON（代理时注入上游请求头），upstream_url 不允许含凭据。
        let _ = raw_sql(
            "CREATE TABLE IF NOT EXISTS gateway_routes (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL DEFAULT '',
                path_prefix TEXT NOT NULL,
                upstream_url TEXT NOT NULL,
                methods TEXT NOT NULL DEFAULT 'GET,POST,PUT,PATCH,DELETE',
                timeout_ms BIGINT NOT NULL DEFAULT 30000,
                enabled BOOLEAN NOT NULL DEFAULT true,
                preserve_query BOOLEAN NOT NULL DEFAULT true,
                strip_prefix BOOLEAN NOT NULL DEFAULT true,
                upstream_headers TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL DEFAULT '',
                UNIQUE (path_prefix)
            )",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError(format!("Migration create gateway_routes: {e}")))?;
        tracing::info!("gateway_routes table ready");

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
}
