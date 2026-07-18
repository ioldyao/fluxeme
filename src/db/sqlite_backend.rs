use async_trait::async_trait;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};

use crate::config::types::GatewayRuntimeConfig;
use crate::db::backend::DbBackend;
use crate::db::{DbError, ProbeResultRow, RechargeKeyRow, WalletTransactionRow};
use crate::domain::channel::{Channel, Endpoint};
use crate::domain::model::{Model, ModelChannel, Pricing};
use crate::domain::moderation::ContentFilterRule;
use crate::domain::routing::RoutingRule;
use crate::domain::usage::{UsageFilter, UsageRecord};
use crate::domain::user::{ApiKey, User};

pub struct SqliteBackend {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteBackend {
    pub fn new(path: &str) -> Result<Self, DbError> {
        let path = path.to_string();
        let exists = Path::new(&path).exists();
        let conn = Connection::open(&path)
            .map_err(|e| DbError(format!("Failed to open database at {}: {}", path, e)))?;
        if !exists {
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
                .map_err(|e| DbError(format!("Failed to set pragmas: {}", e)))?;
            Self::migrate_inner(&conn)?;
            tracing::info!("Database created at {}", path);
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Execute a blocking function on the SQLite connection within an async context.
    /// Uses `block_in_place` to avoid blocking the tokio reactor.
    async fn exec<T>(&self, f: impl FnOnce(&Connection) -> Result<T, DbError>) -> Result<T, DbError> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| DbError("Database mutex poisoned".into()))?;
        tokio::task::block_in_place(move || f(&*guard))
    }

    fn migrate_inner(conn: &Connection) -> Result<(), DbError> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                password_hash TEXT NOT NULL DEFAULT '',
                rpm INTEGER,
                tpm INTEGER,
                concurrency_limit INTEGER NOT NULL DEFAULT 2000,
                currency TEXT NOT NULL DEFAULT 'usd'
            );

            CREATE TABLE IF NOT EXISTS api_keys (
                key TEXT PRIMARY KEY,
                user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                name TEXT DEFAULT '',
                enabled INTEGER NOT NULL DEFAULT 1,
                expires_at TEXT
            );

            CREATE TABLE IF NOT EXISTS channels (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL DEFAULT '',
                provider TEXT NOT NULL,
                priority INTEGER NOT NULL DEFAULT 1,
                enabled INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS endpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                channel_id TEXT NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
                url TEXT NOT NULL,
                api_key TEXT DEFAULT '',
                weight INTEGER NOT NULL DEFAULT 1,
                timeout_secs INTEGER
            );

            CREATE TABLE IF NOT EXISTS models (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                model_pattern TEXT NOT NULL,
                prompt_price REAL NOT NULL DEFAULT 0.0,
                completion_price REAL NOT NULL DEFAULT 0.0
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
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                request_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                user_name TEXT NOT NULL,
                channel_id TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                total_tokens INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                status_code INTEGER NOT NULL,
                success INTEGER NOT NULL,
                request_body TEXT,
                response_body TEXT,
                reasoning_body TEXT,
                api_key_name TEXT,
                api_format TEXT NOT NULL DEFAULT '',
                stream INTEGER NOT NULL DEFAULT 0,
                cache_hit_input_tokens INTEGER NOT NULL DEFAULT 0,
                prompt_price REAL NOT NULL DEFAULT 0.0,
                completion_price REAL NOT NULL DEFAULT 0.0,
                client_ip TEXT
            );
            ",
        )?;

        // Backward compat: add columns that may not exist in older schemas
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN password_hash TEXT NOT NULL DEFAULT '';");
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN request_body TEXT;");
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN response_body TEXT;");
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN reasoning_body TEXT;");
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN api_key_name TEXT;");
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN client_ip TEXT;");
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN cache_read_price REAL NOT NULL DEFAULT 0.0;");
        let _ = conn.execute_batch("ALTER TABLE models ADD COLUMN published INTEGER NOT NULL DEFAULT 0;");
        let _ = conn.execute_batch("ALTER TABLE models ADD COLUMN context_length INTEGER;");
        let _ = conn.execute_batch("ALTER TABLE models ADD COLUMN cache_read_price REAL NOT NULL DEFAULT 0.0;");
        let _ = conn.execute_batch("ALTER TABLE models ADD COLUMN cache_write_price REAL NOT NULL DEFAULT 0.0;");
        let _ = conn.execute_batch("ALTER TABLE models ADD COLUMN image_input_price REAL NOT NULL DEFAULT 0.0;");
        let _ = conn.execute_batch("ALTER TABLE models ADD COLUMN audio_input_price REAL NOT NULL DEFAULT 0.0;");
        let _ = conn.execute_batch("ALTER TABLE models ADD COLUMN audio_output_price REAL NOT NULL DEFAULT 0.0;");
        let _ = conn.execute_batch("ALTER TABLE channels ADD COLUMN name TEXT NOT NULL DEFAULT '';");
        let _ = conn.execute_batch("ALTER TABLE api_keys ADD COLUMN spend_limit REAL;");
        let _ = conn.execute_batch("ALTER TABLE api_keys ADD COLUMN allowed_models TEXT;");
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN concurrency_limit INTEGER NOT NULL DEFAULT 2000;");
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN currency TEXT NOT NULL DEFAULT 'usd';");
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS user_subscriptions (
                user_id TEXT NOT NULL,
                model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL,
                PRIMARY KEY (user_id, model_id)
            );",
        );
        let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_usage_user_id ON usage_logs(user_id)");
        let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_logs(timestamp)");
        let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_usage_user_timestamp ON usage_logs(user_id, timestamp)");
        let _ = conn.execute_batch("ALTER TABLE endpoints ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;");
        let _ = conn.execute_batch("ALTER TABLE models ADD COLUMN category TEXT NOT NULL DEFAULT '';");
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC';");
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN balance REAL NOT NULL DEFAULT 0.0;");
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN frozen REAL NOT NULL DEFAULT 0.0;");
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN token_version INTEGER NOT NULL DEFAULT 0;");
        let _ = conn.execute_batch("ALTER TABLE users ADD COLUMN role TEXT NOT NULL DEFAULT 'user';");
        let _ = conn.execute_batch("UPDATE users SET role='admin' WHERE id='admin' AND role='user';");
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS wallet_transactions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                type TEXT NOT NULL,
                amount REAL NOT NULL,
                balance_before REAL NOT NULL DEFAULT 0.0,
                balance_after REAL NOT NULL DEFAULT 0.0,
                method TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'completed',
                note TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL
            );",
        );
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS recharge_keys (
                key TEXT PRIMARY KEY,
                amount REAL NOT NULL,
                used_by TEXT,
                used_at TEXT,
                created_by TEXT NOT NULL,
                created_at TEXT NOT NULL
            );",
        );
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS balancer_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        );
        let _ = conn.execute_batch("ALTER TABLE recharge_keys ADD COLUMN expires_at TEXT;");
        let _ = conn.execute_batch("ALTER TABLE recharge_keys ADD COLUMN revoked INTEGER NOT NULL DEFAULT 0;");
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS content_filter_rules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL DEFAULT '',
                pattern_type TEXT NOT NULL DEFAULT 'keyword',
                pattern TEXT NOT NULL,
                action TEXT NOT NULL DEFAULT 'block',
                scope TEXT NOT NULL DEFAULT 'both',
                channel_id TEXT,
                replacement TEXT DEFAULT '[REDACTED]',
                enabled INTEGER NOT NULL DEFAULT 1,
                priority INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        );
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS probe_results (
                id TEXT PRIMARY KEY,
                channel_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                success INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                error TEXT,
                probed_at TEXT NOT NULL
            );",
        );
        let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_probe_channel ON probe_results(channel_id)");
        let _ = conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_probe_model ON probe_results(model_id)");
        Ok(())
    }
}

// ── Private helpers ──────────────────────────────────────────────────────────

impl SqliteBackend {
    fn pricing_lookup(conn: &Connection, model_name: &str) -> (f64, f64, f64) {
        let result = conn.query_row(
            "SELECT prompt_price, completion_price, cache_read_price FROM models WHERE name = ?1",
            params![model_name],
            |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?, row.get::<_, f64>(2)?)),
        );
        match result {
            Ok(p) => p,
            Err(_) => {
                let mut stmt = conn
                    .prepare("SELECT prompt_price, completion_price, cache_read_price, model_pattern FROM models")
                    .ok();
                if let Some(ref mut stmt) = stmt {
                    if let Ok(rows) = stmt.query_map([], |row| {
                        Ok((
                            row.get::<_, f64>(0)?,
                            row.get::<_, f64>(1)?,
                            row.get::<_, f64>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    }) {
                        for row in rows.flatten() {
                            let (p, c, cr, pattern) = row;
                            if pattern.ends_with('*') {
                                let prefix = &pattern[..pattern.len() - 1];
                                if model_name.starts_with(prefix) {
                                    return (p, c, cr);
                                }
                            }
                            if pattern == model_name {
                                return (p, c, cr);
                            }
                        }
                    }
                }
                (0.0, 0.0, 0.0)
            }
        }
    }

    fn build_recharge_key_filter(
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
        now: &str,
    ) -> (String, Vec<String>) {
        let mut conditions: Vec<String> = Vec::new();
        let mut params: Vec<String> = Vec::new();

        if let Some(s) = search.filter(|s| !s.is_empty()) {
            params.push(format!("%{}%", s));
            conditions.push(format!("key LIKE ?{}", params.len()));
        }
        if let Some(u) = user_search.filter(|u| !u.is_empty()) {
            params.push(format!("%{}%", u));
            let idx = params.len();
            conditions.push(format!(
                "(used_by LIKE ?{0} OR created_by LIKE ?{0})",
                idx
            ));
        }
        match status {
            Some("active") => {
                params.push(now.to_string());
                let idx = params.len();
                conditions.push("used_by IS NULL".into());
                conditions.push("(revoked IS NULL OR revoked = 0)".into());
                conditions.push(format!("(expires_at IS NULL OR expires_at > ?{})", idx));
            }
            Some("used") => {
                conditions.push("used_by IS NOT NULL".into());
            }
            Some("expired") => {
                params.push(now.to_string());
                let idx = params.len();
                conditions.push("used_by IS NULL".into());
                conditions.push("(revoked IS NULL OR revoked = 0)".into());
                conditions.push("expires_at IS NOT NULL".into());
                conditions.push(format!("expires_at < ?{}", idx));
            }
            Some("revoked") => {
                conditions.push("revoked = 1".into());
            }
            _ => {}
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        (where_clause, params)
    }

    fn map_user_row(row: &rusqlite::Row) -> rusqlite::Result<User> {
        Ok(User {
            id: row.get(0)?,
            name: row.get(1)?,
            password_hash: None,
            rate_limits: {
                let rpm: Option<u64> = row.get(2)?;
                let tpm: Option<u64> = row.get(3)?;
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            token_version: row.get::<_, i64>(5).unwrap_or(0),
            role: row.get::<_, String>(6).unwrap_or_default(),
            concurrency_limit: row.get::<_, u32>(7).unwrap_or(2000),
            currency: row.get::<_, String>(8).unwrap_or_default(),
        })
    }

    fn map_user_with_pw_row(row: &rusqlite::Row) -> rusqlite::Result<User> {
        Ok(User {
            id: row.get(0)?,
            name: row.get(1)?,
            password_hash: Some(row.get::<_, String>(2)?),
            rate_limits: {
                let rpm: Option<u64> = row.get(3)?;
                let tpm: Option<u64> = row.get(4)?;
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            token_version: row.get::<_, i64>(6).unwrap_or(0),
            role: row.get::<_, String>(7).unwrap_or_default(),
            concurrency_limit: row.get::<_, u32>(8).unwrap_or(2000),
            currency: row.get::<_, String>(9).unwrap_or_default(),
        })
    }
}

// ── DbBackend implementation ─────────────────────────────────────────────────

#[async_trait]
impl DbBackend for SqliteBackend {
    async fn migrate(&self) -> Result<(), DbError> {
        self.exec(|conn| Self::migrate_inner(conn)).await
    }

    // ── Users ────────────────────────────────────────────────────────────

    async fn list_users(&self) -> Result<Vec<User>, DbError> {
        self.exec(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency FROM users ORDER BY id",
            )?;
            let rows = stmt.query_map([], Self::map_user_row)?;
            let mut users = Vec::new();
            for row in rows {
                users.push(row?);
            }
            Ok(users)
        })
        .await
    }

    async fn get_user(&self, id: &str) -> Result<Option<User>, DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, rpm, tpm, timezone, token_version, role, concurrency_limit, currency FROM users WHERE id = ?1",
            )?;
            let mut rows = stmt.query_map(params![id], Self::map_user_row)?;
            match rows.next() {
                Some(Ok(u)) => Ok(Some(u)),
                _ => Ok(None),
            }
        })
        .await
    }

    async fn get_user_with_password(&self, id: &str) -> Result<Option<User>, DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency FROM users WHERE id = ?1",
            )?;
            let mut rows = stmt.query_map(params![id], Self::map_user_with_pw_row)?;
            match rows.next() {
                Some(Ok(u)) => Ok(Some(u)),
                _ => Ok(None),
            }
        })
        .await
    }

    async fn create_user(&self, user: &User) -> Result<(), DbError> {
        let user = user.clone();
        self.exec(move |conn| {
            let (rpm, tpm) = user
                .rate_limits
                .as_ref()
                .map(|r| (r.rpm, r.tpm))
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
            conn.execute(
                "INSERT INTO users (id, name, password_hash, rpm, tpm, timezone, token_version, role, concurrency_limit, currency) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![user.id, user.name, pw_hash, rpm, tpm, tz, user.token_version, role, user.concurrency_limit, user.currency],
            )?;
            Ok(())
        })
        .await
    }

    async fn update_user(&self, user: &User) -> Result<(), DbError> {
        let user = user.clone();
        self.exec(move |conn| {
            let (rpm, tpm) = user
                .rate_limits
                .as_ref()
                .map(|r| (r.rpm, r.tpm))
                .unwrap_or((None, None));
            let tz = if user.timezone.is_empty() {
                "UTC"
            } else {
                &user.timezone
            };
            if let Some(ref pw) = user.password_hash {
                conn.execute(
                    "UPDATE users SET name = ?1, password_hash = ?2, rpm = ?3, tpm = ?4, timezone = ?5, token_version = ?6, role = ?7, concurrency_limit = ?8, currency = ?9 WHERE id = ?10",
                    params![user.name, pw, rpm, tpm, tz, user.token_version, user.role, user.concurrency_limit, user.currency, user.id],
                )?;
            } else {
                conn.execute(
                    "UPDATE users SET name = ?1, rpm = ?2, tpm = ?3, timezone = ?4, token_version = ?5, role = ?6, concurrency_limit = ?7, currency = ?8 WHERE id = ?9",
                    params![user.name, rpm, tpm, tz, user.token_version, user.role, user.concurrency_limit, user.currency, user.id],
                )?;
            }
            Ok(())
        })
        .await
    }

    async fn delete_user(&self, id: &str) -> Result<(), DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            conn.execute("DELETE FROM users WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    async fn count_admins(&self) -> Result<i64, DbError> {
        self.exec(|conn| {
            conn.query_row("SELECT COUNT(*) FROM users WHERE role = 'admin'", [], |row| {
                row.get(0)
            })
            .map_err(|e| DbError(e.to_string()))
        })
        .await
    }

    async fn get_user_timezone(&self, id: &str) -> Result<String, DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare("SELECT timezone FROM users WHERE id = ?1")?;
            let tz: Option<String> = stmt.query_row(params![id], |row| row.get(0)).ok();
            Ok(tz.unwrap_or_else(|| "UTC".to_string()))
        })
        .await
    }

    async fn update_user_timezone(&self, id: &str, timezone: &str) -> Result<(), DbError> {
        let id = id.to_string();
        let tz = if timezone.is_empty() {
            "UTC".to_string()
        } else {
            timezone.to_string()
        };
        self.exec(move |conn| {
            conn.execute(
                "UPDATE users SET timezone = ?1 WHERE id = ?2",
                params![tz, id],
            )?;
            Ok(())
        })
        .await
    }

    async fn get_user_currency(&self, id: &str) -> Result<String, DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            let cur: String = conn.query_row(
                "SELECT currency FROM users WHERE id = ?1",
                params![id],
                |row| row.get(0),
            ).unwrap_or_default();
            Ok(cur)
        })
        .await
    }

    async fn update_user_currency(&self, id: &str, currency: &str) -> Result<(), DbError> {
        let id = id.to_string();
        let cur = if currency.is_empty() { "usd".to_string() } else { currency.to_string() };
        self.exec(move |conn| {
            conn.execute(
                "UPDATE users SET currency = ?1 WHERE id = ?2",
                params![cur, id],
            )?;
            Ok(())
        })
        .await
    }

    // ── API Keys ─────────────────────────────────────────────────────────

    async fn list_api_keys(&self, user_id: &str) -> Result<Vec<ApiKey>, DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT key, user_id, name, enabled, expires_at, spend_limit, allowed_models FROM api_keys WHERE user_id = ?1 ORDER BY key",
            )?;
            let rows = stmt.query_map(params![user_id], |row| {
                let allowed_models_str: Option<String> = row.get(6)?;
                Ok(ApiKey {
                    key: row.get(0)?,
                    user_id: row.get(1)?,
                    name: row.get(2)?,
                    enabled: row.get::<_, i32>(3)? != 0,
                    expires_at: row.get(4)?,
                    spend_limit: row.get(5)?,
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                })
            })?;
            let mut keys = Vec::new();
            for row in rows {
                keys.push(row?);
            }
            Ok(keys)
        })
        .await
    }

    async fn create_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        let key = key.clone();
        self.exec(move |conn| {
            let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
            conn.execute(
                "INSERT INTO api_keys (key, user_id, name, enabled, expires_at, spend_limit, allowed_models) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![key.key, key.user_id, key.name, key.enabled as i32, key.expires_at, key.spend_limit, allowed],
            )?;
            Ok(())
        })
        .await
    }

    async fn delete_api_key(&self, key: &str) -> Result<(), DbError> {
        let key = key.to_string();
        self.exec(move |conn| {
            conn.execute("DELETE FROM api_keys WHERE key = ?1", params![key])?;
            Ok(())
        })
        .await
    }

    async fn update_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        let key = key.clone();
        self.exec(move |conn| {
            let allowed = key.allowed_models.as_ref().map(|m| m.join(","));
            conn.execute(
                "UPDATE api_keys SET name = ?1, enabled = ?2, expires_at = ?3, spend_limit = ?4, allowed_models = ?5 WHERE key = ?6",
                params![key.name, key.enabled as i32, key.expires_at, key.spend_limit, allowed, key.key],
            )?;
            Ok(())
        })
        .await
    }

    async fn lookup_key(&self, key: &str) -> Result<Option<(User, ApiKey)>, DbError> {
        let key = key.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, u.token_version, u.role, u.concurrency_limit, u.currency, a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models
                 FROM api_keys a JOIN users u ON u.id = a.user_id WHERE a.key = ?1",
            )?;
            let mut rows = stmt.query_map(params![key], |row| {
                let allowed_models_str: Option<String> = row.get(14)?;
                let api_key = ApiKey {
                    key: row.get(8)?,
                    user_id: row.get(9)?,
                    name: row.get(10)?,
                    enabled: row.get::<_, i32>(11)? != 0,
                    expires_at: row.get(12)?,
                    spend_limit: row.get(13)?,
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                };
                let user = User {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    password_hash: None,
                    rate_limits: {
                        let rpm: Option<u64> = row.get(2)?;
                        let tpm: Option<u64> = row.get(3)?;
                        if rpm.is_some() || tpm.is_some() {
                            Some(crate::domain::user::RateLimit { rpm, tpm })
                        } else {
                            None
                        }
                    },
                    timezone: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    token_version: row.get::<_, i64>(5).unwrap_or(0),
                    role: row.get::<_, String>(6).unwrap_or_default(),
                    concurrency_limit: row.get::<_, u32>(7).unwrap_or(2000),
                    currency: row.get::<_, String>(8).unwrap_or_default(),
                };
                Ok((user, api_key))
            })?;
            match rows.next() {
                Some(Ok(pair)) => Ok(Some(pair)),
                _ => Ok(None),
            }
        })
        .await
    }

    async fn all_api_keys(&self) -> Result<Vec<(User, ApiKey)>, DbError> {
        self.exec(|conn| {
            let mut stmt = conn.prepare(
                "SELECT u.id, u.name, u.rpm, u.tpm, u.timezone, u.token_version, u.role, u.concurrency_limit, u.currency, a.key, a.user_id, a.name, a.enabled, a.expires_at, a.spend_limit, a.allowed_models
                 FROM api_keys a JOIN users u ON u.id = a.user_id ORDER BY a.key",
            )?;
            let rows = stmt.query_map([], |row| {
                let allowed_models_str: Option<String> = row.get(14)?;
                let api_key = ApiKey {
                    key: row.get(8)?,
                    user_id: row.get(9)?,
                    name: row.get(10)?,
                    enabled: row.get::<_, i32>(11)? != 0,
                    expires_at: row.get(12)?,
                    spend_limit: row.get(13)?,
                    allowed_models: allowed_models_str
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(|p| p.trim().to_string()).collect()),
                };
                let user = User {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    password_hash: None,
                    rate_limits: {
                        let rpm: Option<u64> = row.get(2)?;
                        let tpm: Option<u64> = row.get(3)?;
                        if rpm.is_some() || tpm.is_some() {
                            Some(crate::domain::user::RateLimit { rpm, tpm })
                        } else {
                            None
                        }
                    },
                    timezone: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    token_version: row.get::<_, i64>(5).unwrap_or(0),
                    role: row.get::<_, String>(6).unwrap_or_default(),
                    concurrency_limit: row.get::<_, u32>(7).unwrap_or(2000),
                    currency: row.get::<_, String>(8).unwrap_or_default(),
                };
                Ok((user, api_key))
            })?;
            let mut result = Vec::new();
            for row in rows {
                result.push(row?);
            }
            Ok(result)
        })
        .await
    }

    // ── Channels & Endpoints ─────────────────────────────────────────────

    async fn list_channels(&self) -> Result<Vec<Channel>, DbError> {
        self.exec(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, provider, priority, enabled FROM channels ORDER BY priority, id",
            )?;
            let mut channels: Vec<Channel> = stmt
                .query_map([], |row| {
                    Ok(Channel {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        provider: row.get(2)?,
                        priority: row.get(3)?,
                        enabled: row.get::<_, i32>(4)? != 0,
                        endpoints: Vec::new(),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut estmt = conn.prepare(
                "SELECT id, channel_id, url, api_key, weight, timeout_secs, enabled FROM endpoints ORDER BY channel_id",
            )?;
            let endpoint_rows = estmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(1)?,
                        Endpoint {
                            id: Some(row.get(0)?),
                            channel_id: row.get(1)?,
                            url: row.get(2)?,
                            api_key: row.get(3)?,
                            weight: row.get(4)?,
                            timeout_secs: row.get(5)?,
                            enabled: row.get::<_, i32>(6)? != 0,
                        },
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut eps_by_channel: std::collections::HashMap<String, Vec<Endpoint>> =
                std::collections::HashMap::new();
            for (ch_id, ep) in endpoint_rows {
                eps_by_channel.entry(ch_id).or_default().push(ep);
            }
            for ch in &mut channels {
                if let Some(eps) = eps_by_channel.remove(&ch.id) {
                    ch.endpoints = eps;
                }
            }
            Ok(channels)
        })
        .await
    }

    async fn get_channel(&self, id: &str) -> Result<Option<Channel>, DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, provider, priority, enabled FROM channels WHERE id = ?1",
            )?;
            let mut rows = stmt.query_map(params![id], |row| {
                Ok(Channel {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    provider: row.get(2)?,
                    priority: row.get(3)?,
                    enabled: row.get::<_, i32>(4)? != 0,
                    endpoints: Vec::new(),
                })
            })?;
            match rows.next() {
                Some(Ok(mut ch)) => {
                    let mut estmt = conn.prepare(
                        "SELECT id, channel_id, url, api_key, weight, timeout_secs, enabled FROM endpoints WHERE channel_id = ?1",
                    )?;
                    let eps = estmt
                        .query_map(params![ch.id], |row| {
                            Ok(Endpoint {
                                id: Some(row.get(0)?),
                                channel_id: row.get(1)?,
                                url: row.get(2)?,
                                api_key: row.get(3)?,
                                weight: row.get(4)?,
                                timeout_secs: row.get(5)?,
                                enabled: row.get::<_, i32>(6)? != 0,
                            })
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    ch.endpoints = eps;
                    Ok(Some(ch))
                }
                _ => Ok(None),
            }
        })
        .await
    }

    async fn create_channel(&self, ch: &Channel) -> Result<(), DbError> {
        let ch = ch.clone();
        self.exec(move |conn| {
            conn.execute(
                "INSERT INTO channels (id, name, provider, priority, enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![ch.id, ch.name, ch.provider, ch.priority, ch.enabled as i32],
            )?;
            for ep in &ch.endpoints {
                conn.execute(
                    "INSERT INTO endpoints (channel_id, url, api_key, weight, timeout_secs, enabled) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![ch.id, ep.url, ep.api_key, ep.weight, ep.timeout_secs, ep.enabled as i32],
                )?;
            }
            Ok(())
        })
        .await
    }

    async fn update_channel(&self, ch: &Channel) -> Result<(), DbError> {
        let ch = ch.clone();
        self.exec(move |conn| {
            conn.execute(
                "UPDATE channels SET name = ?1, provider = ?2, priority = ?3, enabled = ?4 WHERE id = ?5",
                params![ch.name, ch.provider, ch.priority, ch.enabled as i32, ch.id],
            )?;
            conn.execute("DELETE FROM endpoints WHERE channel_id = ?1", params![ch.id])?;
            for ep in &ch.endpoints {
                conn.execute(
                    "INSERT INTO endpoints (channel_id, url, api_key, weight, timeout_secs, enabled) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![ch.id, ep.url, ep.api_key, ep.weight, ep.timeout_secs, ep.enabled as i32],
                )?;
            }
            Ok(())
        })
        .await
    }

    async fn delete_channel(&self, id: &str) -> Result<(), DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            conn.execute("DELETE FROM channels WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    async fn get_endpoint(&self, id: i64) -> Result<Option<Endpoint>, DbError> {
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, channel_id, url, api_key, weight, timeout_secs, enabled FROM endpoints WHERE id = ?1",
            )?;
            let mut rows = stmt.query_map(params![id], |row| {
                Ok(Endpoint {
                    id: Some(row.get(0)?),
                    channel_id: row.get(1)?,
                    url: row.get(2)?,
                    api_key: row.get(3)?,
                    weight: row.get(4)?,
                    timeout_secs: row.get(5)?,
                    enabled: row.get::<_, i32>(6)? != 0,
                })
            })?;
            match rows.next() {
                Some(Ok(ep)) => Ok(Some(ep)),
                _ => Ok(None),
            }
        })
        .await
    }

    async fn update_endpoint_enabled(&self, id: i64, enabled: bool) -> Result<(), DbError> {
        self.exec(move |conn| {
            conn.execute(
                "UPDATE endpoints SET enabled = ?1 WHERE id = ?2",
                params![enabled as i32, id],
            )?;
            Ok(())
        })
        .await
    }

    // ── Models ───────────────────────────────────────────────────────────

    async fn list_models(&self) -> Result<Vec<Model>, DbError> {
        self.exec(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, model_pattern, prompt_price, completion_price, cache_read_price, cache_write_price, image_input_price, audio_input_price, audio_output_price, published, context_length, category FROM models ORDER BY id",
            )?;
            let mut models: Vec<Model> = stmt
                .query_map([], |row| {
                    Ok(Model {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        model_pattern: row.get(2)?,
                        pricing: Pricing {
                            prompt_price: row.get(3)?,
                            completion_price: row.get(4)?,
                            cache_read_price: row.get(5)?,
                            cache_write_price: row.get(6)?,
                            image_input_price: row.get(7)?,
                            audio_input_price: row.get(8)?,
                            audio_output_price: row.get(9)?,
                        },
                        channels: Vec::new(),
                        published: row.get::<_, i32>(10)? != 0,
                        context_length: row.get(11)?,
                        category: row.get::<_, String>(12).unwrap_or_default(),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut bstmt = conn.prepare(
                "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, '') FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id ORDER BY mc.model_id, mc.priority",
            )?;
            let binding_rows = bstmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        ModelChannel {
                            model_id: row.get(0)?,
                            channel_id: row.get(1)?,
                            priority: row.get(2)?,
                            provider: row.get::<_, String>(3).unwrap_or_default(),
                        },
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut by_model: std::collections::HashMap<String, Vec<ModelChannel>> =
                std::collections::HashMap::new();
            for (model_id, binding) in binding_rows {
                by_model.entry(model_id).or_default().push(binding);
            }
            for m in &mut models {
                if let Some(bindings) = by_model.remove(&m.id) {
                    m.channels = bindings;
                }
            }
            Ok(models)
        })
        .await
    }

    async fn get_model(&self, id: &str) -> Result<Option<Model>, DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, model_pattern, prompt_price, completion_price, cache_read_price, cache_write_price, image_input_price, audio_input_price, audio_output_price, published, context_length, category FROM models WHERE id = ?1",
            )?;
            let mut rows = stmt.query_map(params![id], |row| {
                Ok(Model {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    model_pattern: row.get(2)?,
                    pricing: Pricing {
                        prompt_price: row.get(3)?,
                        completion_price: row.get(4)?,
                        cache_read_price: row.get(5)?,
                        cache_write_price: row.get(6)?,
                        image_input_price: row.get(7)?,
                        audio_input_price: row.get(8)?,
                        audio_output_price: row.get(9)?,
                    },
                    channels: Vec::new(),
                    published: row.get::<_, i32>(10)? != 0,
                    context_length: row.get(11)?,
                    category: row.get::<_, String>(12).unwrap_or_default(),
                })
            })?;
            match rows.next() {
                Some(Ok(mut m)) => {
                    let mut bstmt = conn.prepare(
                        "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, '') FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id WHERE mc.model_id = ?1 ORDER BY mc.priority",
                    )?;
                    let bindings = bstmt
                        .query_map(params![m.id], |row| {
                            Ok(ModelChannel {
                                model_id: row.get(0)?,
                                channel_id: row.get(1)?,
                                priority: row.get(2)?,
                                provider: row.get::<_, String>(3).unwrap_or_default(),
                            })
                        })?
                        .collect::<Result<Vec<_>, _>>()?;
                    m.channels = bindings;
                    Ok(Some(m))
                }
                _ => Ok(None),
            }
        })
        .await
    }

    async fn create_model(&self, m: &Model) -> Result<(), DbError> {
        let m = m.clone();
        self.exec(move |conn| {
            conn.execute(
                "INSERT INTO models (id, name, model_pattern, prompt_price, completion_price, cache_read_price, cache_write_price, image_input_price, audio_input_price, audio_output_price, published, context_length, category) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![m.id, m.name, m.model_pattern, m.pricing.prompt_price, m.pricing.completion_price, m.pricing.cache_read_price, m.pricing.cache_write_price, m.pricing.image_input_price, m.pricing.audio_input_price, m.pricing.audio_output_price, m.published as i32, m.context_length, m.category],
            )?;
            for binding in &m.channels {
                conn.execute(
                    "INSERT INTO model_channels (model_id, channel_id, priority) VALUES (?1, ?2, ?3)",
                    params![m.id, binding.channel_id, binding.priority],
                )?;
            }
            Ok(())
        })
        .await
    }

    async fn update_model(&self, old_id: &str, m: &Model) -> Result<(), DbError> {
        let old_id = old_id.to_string();
        let m = m.clone();
        self.exec(move |conn| {
            conn.execute(
                "UPDATE models SET id=?1, name=?2, model_pattern=?3, prompt_price=?4, completion_price=?5, cache_read_price=?6, cache_write_price=?7, image_input_price=?8, audio_input_price=?9, audio_output_price=?10, published=?11, context_length=?12, category=?13 WHERE id=?14",
                params![m.id, m.name, m.model_pattern, m.pricing.prompt_price, m.pricing.completion_price, m.pricing.cache_read_price, m.pricing.cache_write_price, m.pricing.image_input_price, m.pricing.audio_input_price, m.pricing.audio_output_price, m.published as i32, m.context_length, m.category, old_id],
            )?;
            conn.execute("DELETE FROM model_channels WHERE model_id = ?1", params![old_id])?;
            for binding in &m.channels {
                conn.execute(
                    "INSERT INTO model_channels (model_id, channel_id, priority) VALUES (?1, ?2, ?3)",
                    params![m.id, binding.channel_id, binding.priority],
                )?;
            }
            Ok(())
        })
        .await
    }

    async fn delete_model(&self, id: &str) -> Result<(), DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            conn.execute("DELETE FROM models WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    async fn list_published_models(&self) -> Result<Vec<Model>, DbError> {
        self.exec(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, model_pattern, prompt_price, completion_price, cache_read_price, cache_write_price, image_input_price, audio_input_price, audio_output_price, published, context_length,category FROM models WHERE published = 1 ORDER BY id",
            )?;
            let mut models: Vec<Model> = stmt
                .query_map([], |row| {
                    Ok(Model {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        model_pattern: row.get(2)?,
                        pricing: Pricing {
                            prompt_price: row.get(3)?,
                            completion_price: row.get(4)?,
                            cache_read_price: row.get(5)?,
                            cache_write_price: row.get(6)?,
                            image_input_price: row.get(7)?,
                            audio_input_price: row.get(8)?,
                            audio_output_price: row.get(9)?,
                        },
                        channels: Vec::new(),
                        published: true,
                        context_length: row.get(11)?,
                        category: row.get::<_, String>(12).unwrap_or_default(),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut bstmt = conn.prepare(
                "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, '') FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id ORDER BY mc.model_id, mc.priority",
            )?;
            let binding_rows = bstmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        ModelChannel {
                            model_id: row.get(0)?,
                            channel_id: row.get(1)?,
                            priority: row.get(2)?,
                            provider: row.get::<_, String>(3).unwrap_or_default(),
                        },
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut by_model: std::collections::HashMap<String, Vec<ModelChannel>> =
                std::collections::HashMap::new();
            for (model_id, binding) in binding_rows {
                by_model.entry(model_id).or_default().push(binding);
            }
            for m in &mut models {
                if let Some(bindings) = by_model.remove(&m.id) {
                    m.channels = bindings;
                }
            }
            Ok(models)
        })
        .await
    }

    async fn set_model_published(&self, id: &str, published: bool) -> Result<(), DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            conn.execute(
                "UPDATE models SET published = ?1 WHERE id = ?2",
                params![published as i32, id],
            )?;
            Ok(())
        })
        .await
    }

    async fn set_model_pricing(&self, id: &str, pricing: &Pricing) -> Result<(), DbError> {
        let id = id.to_string();
        let p = pricing.clone();
        self.exec(move |conn| {
            conn.execute(
                "UPDATE models SET prompt_price=?1, completion_price=?2, cache_read_price=?3, cache_write_price=?4, image_input_price=?5, audio_input_price=?6, audio_output_price=?7 WHERE id=?8",
                params![p.prompt_price, p.completion_price, p.cache_read_price, p.cache_write_price, p.image_input_price, p.audio_input_price, p.audio_output_price, id],
            )?;
            Ok(())
        })
        .await
    }

    async fn set_model_context_length(&self, id: &str, context_length: i64) -> Result<(), DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            conn.execute(
                "UPDATE models SET context_length = ?1 WHERE id = ?2",
                params![context_length, id],
            )?;
            Ok(())
        })
        .await
    }

    // ── Subscriptions ────────────────────────────────────────────────────

    async fn subscribe_user(&self, user_id: &str, model_id: &str) -> Result<(), DbError> {
        let user_id = user_id.to_string();
        let model_id = model_id.to_string();
        self.exec(move |conn| {
            conn.execute(
                "INSERT OR IGNORE INTO user_subscriptions (user_id, model_id, created_at) VALUES (?1, ?2, ?3)",
                params![user_id, model_id, chrono::Utc::now().to_rfc3339()],
            )?;
            Ok(())
        })
        .await
    }

    async fn unsubscribe_user(&self, user_id: &str, model_id: &str) -> Result<(), DbError> {
        let user_id = user_id.to_string();
        let model_id = model_id.to_string();
        self.exec(move |conn| {
            conn.execute(
                "DELETE FROM user_subscriptions WHERE user_id = ?1 AND model_id = ?2",
                params![user_id, model_id],
            )?;
            Ok(())
        })
        .await
    }

    async fn delete_subscriptions_by_model(&self, model_id: &str) -> Result<(), DbError> {
        let model_id = model_id.to_string();
        self.exec(move |conn| {
            conn.execute(
                "DELETE FROM user_subscriptions WHERE model_id = ?1",
                params![model_id],
            )?;
            Ok(())
        })
        .await
    }

    async fn list_subscribed_model_ids(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            let mut stmt =
                conn.prepare("SELECT model_id FROM user_subscriptions WHERE user_id = ?1")?;
            let ids = stmt
                .query_map(params![user_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ids)
        })
        .await
    }

    async fn list_subscriptions(&self, user_id: &str) -> Result<Vec<Model>, DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.name, m.model_pattern, m.prompt_price, m.completion_price, m.cache_read_price, m.cache_write_price, m.image_input_price, m.audio_input_price, m.audio_output_price, m.published, m.context_length, m.category
                 FROM models m INNER JOIN user_subscriptions s ON m.id = s.model_id
                 WHERE s.user_id = ?1 ORDER BY m.id",
            )?;
            let mut models: Vec<Model> = stmt
                .query_map(params![user_id], |row| {
                    Ok(Model {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        model_pattern: row.get(2)?,
                        pricing: Pricing {
                            prompt_price: row.get(3)?,
                            completion_price: row.get(4)?,
                            cache_read_price: row.get(5)?,
                            cache_write_price: row.get(6)?,
                            image_input_price: row.get(7)?,
                            audio_input_price: row.get(8)?,
                            audio_output_price: row.get(9)?,
                        },
                        channels: Vec::new(),
                        published: row.get::<_, i32>(10)? != 0,
                        context_length: row.get(11)?,
                        category: row.get::<_, String>(12).unwrap_or_default(),
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            // Load bindings for all models (single query, no N+1)
            let mut bstmt = conn.prepare(
                "SELECT mc.model_id, mc.channel_id, mc.priority, COALESCE(c.provider, '') FROM model_channels mc LEFT JOIN channels c ON c.id = mc.channel_id ORDER BY mc.model_id, mc.priority",
            )?;
            let rows = bstmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        ModelChannel {
                            model_id: row.get(0)?,
                            channel_id: row.get(1)?,
                            priority: row.get(2)?,
                            provider: row.get::<_, String>(3).unwrap_or_default(),
                        },
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let mut by_model: std::collections::HashMap<String, Vec<ModelChannel>> =
                std::collections::HashMap::new();
            for (model_id, binding) in rows {
                by_model.entry(model_id).or_default().push(binding);
            }
            for m in &mut models {
                if let Some(bindings) = by_model.remove(&m.id) {
                    m.channels = bindings;
                }
            }
            Ok(models)
        })
        .await
    }

    // ── Routing Rules ────────────────────────────────────────────────────

    async fn list_rules(&self) -> Result<Vec<RoutingRule>, DbError> {
        self.exec(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name, user_id, model_pattern, channel_id FROM routing_rules ORDER BY name",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(RoutingRule {
                    name: row.get(0)?,
                    user_id: row.get(1)?,
                    model_pattern: row.get(2)?,
                    channel_id: row.get(3)?,
                })
            })?;
            let mut rules = Vec::new();
            for row in rows {
                rules.push(row?);
            }
            Ok(rules)
        })
        .await
    }

    async fn create_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        let r = r.clone();
        self.exec(move |conn| {
            conn.execute(
                "INSERT INTO routing_rules (name, user_id, model_pattern, channel_id) VALUES (?1, ?2, ?3, ?4)",
                params![r.name, r.user_id, r.model_pattern, r.channel_id],
            )?;
            Ok(())
        })
        .await
    }

    async fn update_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        let r = r.clone();
        self.exec(move |conn| {
            conn.execute(
                "UPDATE routing_rules SET user_id = ?1, model_pattern = ?2, channel_id = ?3 WHERE name = ?4",
                params![r.user_id, r.model_pattern, r.channel_id, r.name],
            )?;
            Ok(())
        })
        .await
    }

    async fn delete_rule(&self, name: &str) -> Result<(), DbError> {
        let name = name.to_string();
        self.exec(move |conn| {
            conn.execute("DELETE FROM routing_rules WHERE name = ?1", params![name])?;
            Ok(())
        })
        .await
    }

    // ── Usage Logs ───────────────────────────────────────────────────────

    async fn insert_usage(&self, record: &UsageRecord) -> Result<(), DbError> {
        let record = record.clone();
        self.exec(move |conn| {
            conn.execute(
                "INSERT INTO usage_logs (timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, request_body, response_body, reasoning_body, api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, client_ip)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                params![
                    record.timestamp, record.request_id, record.user_id, record.user_name,
                    record.channel_id, record.model, record.prompt_tokens, record.completion_tokens,
                    record.total_tokens, record.latency_ms, record.status_code, record.success as i32,
                    record.request_body, record.response_body, record.reasoning_body,
                    record.api_key_name, record.api_format, record.stream as i32,
                    record.cache_hit_input_tokens, record.prompt_price, record.completion_price,
                    record.client_ip,
                ],
            )?;
            Ok(())
        })
        .await
    }

    async fn count_usage(&self) -> Result<usize, DbError> {
        self.exec(|conn| {
            Ok(conn.query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))?)
        })
        .await
    }

    async fn count_usage_by_user(&self, user_id: &str) -> Result<usize, DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM usage_logs WHERE user_id = ?1",
                params![user_id],
                |row| row.get(0),
            )?)
        })
        .await
    }

    async fn count_usage_filtered(&self, filter: &UsageFilter) -> Result<usize, DbError> {
        let filter = filter.clone();
        self.exec(move |conn| {
            let mut conditions = Vec::new();
            let mut param_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(ref uid) = filter.user_id {
                conditions.push(format!("user_id = ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(uid.clone()));
            }
            if let Some(ref m) = filter.model {
                conditions.push(format!("model LIKE ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(format!("%{}%", m)));
            }
            if let Some(ref k) = filter.api_key_name {
                conditions.push(format!("api_key_name LIKE ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(format!("%{}%", k)));
            }
            if let Some(ref f) = filter.api_format {
                conditions.push(format!("api_format = ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(f.clone()));
            }
            if let Some(ref sd) = filter.start_date {
                conditions.push(format!("timestamp >= ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(sd.clone()));
            }
            if let Some(ref ed) = filter.end_date {
                conditions.push(format!("timestamp <= ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(ed.clone()));
            }

            if !conditions.is_empty() {
                let where_clause = conditions.join(" AND ");
                let sql = format!("SELECT COUNT(*) FROM usage_logs WHERE {}", where_clause);
                let mut stmt = conn.prepare(&sql)?;
                let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                    param_vals.iter().map(|p| p.as_ref()).collect();
                Ok(stmt.query_row(params_refs.as_slice(), |row| row.get(0))?)
            } else {
                Ok(conn.query_row("SELECT COUNT(*) FROM usage_logs", [], |row| {
                    row.get(0)
                })?)
            }
        })
        .await
    }

    async fn query_usage(
        &self,
        limit: usize,
        offset: usize,
        filter: &UsageFilter,
    ) -> Result<Vec<UsageRecord>, DbError> {
        let filter = filter.clone();
        self.exec(move |conn| {
            let mut conditions = Vec::new();
            let mut param_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

            if let Some(ref uid) = filter.user_id {
                conditions.push(format!("user_id = ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(uid.clone()));
            }
            if let Some(ref m) = filter.model {
                conditions.push(format!("model LIKE ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(format!("%{}%", m)));
            }
            if let Some(ref k) = filter.api_key_name {
                conditions.push(format!("api_key_name LIKE ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(format!("%{}%", k)));
            }
            if let Some(ref f) = filter.api_format {
                conditions.push(format!("api_format = ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(f.clone()));
            }
            if let Some(ref sd) = filter.start_date {
                conditions.push(format!("timestamp >= ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(sd.clone()));
            }
            if let Some(ref ed) = filter.end_date {
                conditions.push(format!("timestamp <= ?{}", param_vals.len() + 1));
                param_vals.push(Box::new(ed.clone()));
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let limit_idx = param_vals.len() + 1;
            let offset_idx = param_vals.len() + 2;

            let sql = format!(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, cache_read_price FROM usage_logs {} ORDER BY id DESC LIMIT ?{} OFFSET ?{}",
                where_clause, limit_idx, offset_idx
            );

            let mut stmt = conn.prepare(&sql)?;
            let limit_i64 = limit as i64;
            let offset_i64 = offset as i64;
            let mut params: Vec<&dyn rusqlite::types::ToSql> =
                Vec::with_capacity(param_vals.len() + 2);
            for p in &param_vals {
                params.push(p.as_ref());
            }
            params.push(&limit_i64);
            params.push(&offset_i64);

            let mut records = Vec::new();
            let mut rows = stmt.query(params.as_slice())?;
            while let Some(row) = rows.next()? {
                records.push(UsageRecord {
                    timestamp: row.get(0)?,
                    request_id: row.get(1)?,
                    user_id: row.get(2)?,
                    user_name: row.get(3)?,
                    channel_id: row.get(4)?,
                    model: row.get(5)?,
                    prompt_tokens: row.get(6)?,
                    completion_tokens: row.get(7)?,
                    total_tokens: row.get(8)?,
                    latency_ms: row.get(9)?,
                    status_code: row.get(10)?,
                    success: row.get::<_, i32>(11)? != 0,
                    request_body: None,
                    response_body: None,
                    reasoning_body: None,
                    api_key_name: row.get::<_, Option<String>>(12).ok().flatten(),
                    api_format: row.get::<_, String>(13).unwrap_or_default(),
                    stream: row.get::<_, i32>(14)? != 0,
                    cache_hit_input_tokens: row.get::<_, i64>(15)? as u64,
                    prompt_price: row.get::<_, f64>(16)?,
                    completion_price: row.get::<_, f64>(17)?,
                    cache_read_price: row.get::<_, f64>(18)?,
                    client_ip: None,
                });
            }
            Ok(records)
        })
        .await
    }

    async fn get_usage_detail(&self, request_id: &str) -> Result<Option<UsageRecord>, DbError> {
        let request_id = request_id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, request_body, response_body, reasoning_body, api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, cache_read_price, client_ip
                 FROM usage_logs WHERE request_id = ?1",
            )?;
            let mut rows = stmt.query_map(params![request_id], |row| {
                Ok(UsageRecord {
                    timestamp: row.get(0)?,
                    request_id: row.get(1)?,
                    user_id: row.get(2)?,
                    user_name: row.get(3)?,
                    channel_id: row.get(4)?,
                    model: row.get(5)?,
                    prompt_tokens: row.get(6)?,
                    completion_tokens: row.get(7)?,
                    total_tokens: row.get(8)?,
                    latency_ms: row.get(9)?,
                    status_code: row.get(10)?,
                    success: row.get::<_, i32>(11)? != 0,
                    request_body: row.get(12)?,
                    response_body: row.get(13)?,
                    reasoning_body: row.get(14)?,
                    api_key_name: row.get(15)?,
                    api_format: row.get(16)?,
                    stream: row.get::<_, i32>(17)? != 0,
                    cache_hit_input_tokens: row.get::<_, i64>(18)? as u64,
                    prompt_price: row.get::<_, f64>(19)?,
                    completion_price: row.get::<_, f64>(20)?,
                    cache_read_price: row.get::<_, f64>(21)?,
                    client_ip: row.get(22)?,
                })
            })?;
            match rows.next() {
                Some(Ok(r)) => Ok(Some(r)),
                _ => Ok(None),
            }
        })
        .await
    }

    async fn purge_usage_logs(&self, cutoff: &str) -> Result<usize, DbError> {
        let cutoff = cutoff.to_string();
        self.exec(move |conn| {
            let count = conn.execute(
                "DELETE FROM usage_logs WHERE timestamp < ?1",
                params![cutoff],
            )?;
            Ok(count)
        })
        .await
    }

    async fn usage_stats_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<(u64, u64, u64, u64), DbError> {
        let since = since.to_string();
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let (total, success, latency, total_tok): (u64, u64, u64, u64) =
                if let Some(ref uid) = uid {
                    conn.query_row(
                        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END),0), COALESCE(SUM(latency_ms),0), COALESCE(SUM(total_tokens),0)
                         FROM usage_logs WHERE user_id = ?1 AND timestamp >= ?2",
                        params![uid, since],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )?
                } else {
                    conn.query_row(
                        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END),0), COALESCE(SUM(latency_ms),0), COALESCE(SUM(total_tokens),0)
                         FROM usage_logs WHERE timestamp >= ?1",
                        params![since],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                    )?
                };
            Ok((total, success, latency, total_tok))
        })
        .await
    }

    async fn usage_cost_rows_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, DbError> {
        let since = since.to_string();
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let mut records = Vec::new();
            if let Some(ref uid) = uid {
                let mut stmt = conn.prepare(
                    "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, cache_read_price
                     FROM usage_logs WHERE user_id = ?1 AND timestamp >= ?2 ORDER BY id ASC",
                )?;
                let mut rows = stmt.query(params![uid, since])?;
                while let Some(row) = rows.next()? {
                    records.push(UsageRecord {
                        timestamp: row.get(0)?,
                        request_id: row.get(1)?,
                        user_id: row.get(2)?,
                        user_name: row.get(3)?,
                        channel_id: row.get(4)?,
                        model: row.get(5)?,
                        prompt_tokens: row.get(6)?,
                        completion_tokens: row.get(7)?,
                        total_tokens: row.get(8)?,
                        latency_ms: row.get(9)?,
                        status_code: row.get(10)?,
                        success: row.get::<_, i32>(11)? != 0,
                        request_body: None,
                        response_body: None,
                        reasoning_body: None,
                        api_key_name: row.get::<_, Option<String>>(12).ok().flatten(),
                        api_format: row.get::<_, String>(13).unwrap_or_default(),
                        stream: row.get::<_, i32>(14)? != 0,
                        cache_hit_input_tokens: row.get::<_, i64>(15)? as u64,
                        prompt_price: row.get::<_, f64>(16)?,
                        completion_price: row.get::<_, f64>(17)?,
                        cache_read_price: row.get::<_, f64>(18)?,
                        client_ip: None,
                    });
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, cache_read_price
                     FROM usage_logs WHERE timestamp >= ?1 ORDER BY id ASC",
                )?;
                let mut rows = stmt.query(params![since])?;
                while let Some(row) = rows.next()? {
                    records.push(UsageRecord {
                        timestamp: row.get(0)?,
                        request_id: row.get(1)?,
                        user_id: row.get(2)?,
                        user_name: row.get(3)?,
                        channel_id: row.get(4)?,
                        model: row.get(5)?,
                        prompt_tokens: row.get(6)?,
                        completion_tokens: row.get(7)?,
                        total_tokens: row.get(8)?,
                        latency_ms: row.get(9)?,
                        status_code: row.get(10)?,
                        success: row.get::<_, i32>(11)? != 0,
                        request_body: None,
                        response_body: None,
                        reasoning_body: None,
                        api_key_name: row.get::<_, Option<String>>(12).ok().flatten(),
                        api_format: row.get::<_, String>(13).unwrap_or_default(),
                        stream: row.get::<_, i32>(14)? != 0,
                        cache_hit_input_tokens: row.get::<_, i64>(15)? as u64,
                        prompt_price: row.get::<_, f64>(16)?,
                        completion_price: row.get::<_, f64>(17)?,
                        cache_read_price: row.get::<_, f64>(18)?,
                        client_ip: None,
                    });
                }
            }
            Ok(records)
        })
        .await
    }

    async fn query_usage_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, DbError> {
        let since = since.to_string();
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let mut records = Vec::new();
            if let Some(ref uid) = uid {
                let mut stmt = conn.prepare(
                    "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format, stream, cache_hit_input_tokens
                     FROM usage_logs WHERE user_id = ?1 AND timestamp >= ?2 ORDER BY id ASC",
                )?;
                let mut rows = stmt.query(params![uid, since])?;
                while let Some(row) = rows.next()? {
                    records.push(UsageRecord {
                        timestamp: row.get(0)?,
                        request_id: row.get(1)?,
                        user_id: row.get(2)?,
                        user_name: row.get(3)?,
                        channel_id: row.get(4)?,
                        model: row.get(5)?,
                        prompt_tokens: row.get(6)?,
                        completion_tokens: row.get(7)?,
                        total_tokens: row.get(8)?,
                        latency_ms: row.get(9)?,
                        status_code: row.get(10)?,
                        success: row.get::<_, i32>(11)? != 0,
                        request_body: None,
                        response_body: None,
                        reasoning_body: None,
                        api_key_name: row.get::<_, Option<String>>(12).ok().flatten(),
                        api_format: row.get::<_, String>(13).unwrap_or_default(),
                        stream: row.get::<_, i32>(14)? != 0,
                        cache_hit_input_tokens: row.get::<_, i64>(15)? as u64,
                        prompt_price: 0.0,
                        completion_price: 0.0,
                        cache_read_price: 0.0,
                        client_ip: None,
                    });
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format, stream, cache_hit_input_tokens
                     FROM usage_logs WHERE timestamp >= ?1 ORDER BY id ASC",
                )?;
                let mut rows = stmt.query(params![since])?;
                while let Some(row) = rows.next()? {
                    records.push(UsageRecord {
                        timestamp: row.get(0)?,
                        request_id: row.get(1)?,
                        user_id: row.get(2)?,
                        user_name: row.get(3)?,
                        channel_id: row.get(4)?,
                        model: row.get(5)?,
                        prompt_tokens: row.get(6)?,
                        completion_tokens: row.get(7)?,
                        total_tokens: row.get(8)?,
                        latency_ms: row.get(9)?,
                        status_code: row.get(10)?,
                        success: row.get::<_, i32>(11)? != 0,
                        request_body: None,
                        response_body: None,
                        reasoning_body: None,
                        api_key_name: row.get::<_, Option<String>>(12).ok().flatten(),
                        api_format: row.get::<_, String>(13).unwrap_or_default(),
                        stream: row.get::<_, i32>(14)? != 0,
                        cache_hit_input_tokens: row.get::<_, i64>(15)? as u64,
                        prompt_price: 0.0,
                        completion_price: 0.0,
                        cache_read_price: 0.0,
                        client_ip: None,
                    });
                }
            }
            Ok(records)
        })
        .await
    }

    async fn daily_usage_counts(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, i64)>, DbError> {
        let since = since.to_string();
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let mut records = Vec::new();
            let offset_expr = if tz_offset_seconds >= 0 {
                format!("datetime(timestamp, '+{} seconds')", tz_offset_seconds)
            } else {
                format!("datetime(timestamp, '-{} seconds')", -tz_offset_seconds)
            };
            let day_expr = format!("substr({}, 1, 10)", offset_expr);
            if let Some(ref uid) = uid {
                let sql = format!(
                    "SELECT {} as day, COUNT(*) FROM usage_logs WHERE user_id = ?1 AND timestamp >= ?2 GROUP BY day ORDER BY day ASC",
                    day_expr
                );
                let mut stmt = conn.prepare(&sql)?;
                let mut rows = stmt.query(params![uid, since])?;
                while let Some(row) = rows.next()? {
                    records.push((row.get::<_, String>(0)?, row.get::<_, i64>(1)?));
                }
            } else {
                let sql = format!(
                    "SELECT {} as day, COUNT(*) FROM usage_logs WHERE timestamp >= ?1 GROUP BY day ORDER BY day ASC",
                    day_expr
                );
                let mut stmt = conn.prepare(&sql)?;
                let mut rows = stmt.query(params![since])?;
                while let Some(row) = rows.next()? {
                    records.push((row.get::<_, String>(0)?, row.get::<_, i64>(1)?));
                }
            }
            Ok(records)
        })
        .await
    }

    async fn daily_usage_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64, u64)>, DbError> {
        let since = since.to_string();
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let mut records = Vec::new();
            let offset_expr = if tz_offset_seconds >= 0 {
                format!("datetime(timestamp, '+{} seconds')", tz_offset_seconds)
            } else {
                format!("datetime(timestamp, '-{} seconds')", -tz_offset_seconds)
            };
            let day_expr = format!("substr({}, 1, 10)", offset_expr);
            let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
                if let Some(ref uid) = uid {
                    (
                        format!(
                            "SELECT {} as day, COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0), COALESCE(SUM(CASE WHEN success=1 THEN 1 ELSE 0 END),0), COALESCE(SUM(latency_ms),0), COALESCE(SUM(cache_hit_input_tokens),0) FROM usage_logs WHERE user_id = ?1 AND timestamp >= ?2 GROUP BY day ORDER BY day ASC",
                            day_expr
                        ),
                        vec![Box::new(uid.clone()), Box::new(since.clone())],
                    )
                } else {
                    (
                        format!(
                            "SELECT {} as day, COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0), COALESCE(SUM(CASE WHEN success=1 THEN 1 ELSE 0 END),0), COALESCE(SUM(latency_ms),0), COALESCE(SUM(cache_hit_input_tokens),0) FROM usage_logs WHERE timestamp >= ?1 GROUP BY day ORDER BY day ASC",
                            day_expr
                        ),
                        vec![Box::new(since.clone())],
                    )
                };
            let params_ref: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(params_ref.as_slice())?;
            while let Some(row) = rows.next()? {
                records.push((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, u64>(5)?,
                    row.get::<_, u64>(6)?,
                    row.get::<_, u64>(7)?,
                ));
            }
            Ok(records)
        })
        .await
    }

    async fn model_activity(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64)>, DbError> {
        let since = since.to_string();
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let mut records = Vec::new();
            let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
                if let Some(ref uid) = uid {
                    ("SELECT model, COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(CASE WHEN success=1 THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN success=0 THEN 1 ELSE 0 END),0), COALESCE(SUM(cache_hit_input_tokens),0) FROM usage_logs WHERE timestamp >= ?1 AND user_id = ?2 GROUP BY model ORDER BY COUNT(*) DESC".into(),
                     vec![Box::new(since.clone()), Box::new(uid.clone())])
                } else {
                    ("SELECT model, COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(CASE WHEN success=1 THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN success=0 THEN 1 ELSE 0 END),0), COALESCE(SUM(cache_hit_input_tokens),0) FROM usage_logs WHERE timestamp >= ?1 GROUP BY model ORDER BY COUNT(*) DESC".into(),
                     vec![Box::new(since.clone())])
                };
            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let mut rows = stmt.query(params.as_slice())?;
            while let Some(row) = rows.next()? {
                records.push((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, u64>(5)?,
                    row.get::<_, u64>(6)?,
                ));
            }
            Ok(records)
        })
        .await
    }

    // ── Billing / Period ─────────────────────────────────────────────────

    async fn period_summary(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<(f64, u64, u64), DbError> {
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let start = format!("{}-{:02}-01T00:00:00", year, month);
            let end = if month == 12 {
                format!("{}-01-01T00:00:00", year + 1)
            } else {
                format!("{}-{:02}-01T00:00:00", year, month + 1)
            };
            let sql = format!(
                "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + completion_tokens / 1000000.0 * completion_price + cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), COUNT(*), COALESCE(SUM(total_tokens), 0) FROM usage_logs WHERE timestamp >= ?1 AND timestamp < ?2{}",
                if uid.is_some() { " AND user_id = ?3" } else { "" }
            );
            let mut stmt = conn.prepare(&sql)?;
            let result = if let Some(ref uid) = uid {
                stmt.query_row(params![start, end, uid], |row| {
                    Ok((row.get::<_, f64>(0)?, row.get::<_, u64>(1)?, row.get::<_, u64>(2)?))
                })
            } else {
                stmt.query_row(params![start, end], |row| {
                    Ok((row.get::<_, f64>(0)?, row.get::<_, u64>(1)?, row.get::<_, u64>(2)?))
                })
            };
            result.map_err(|e| DbError(e.to_string()))
        })
        .await
    }

    async fn period_model_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, f64)>, DbError> {
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let start = format!("{}-{:02}-01T00:00:00", year, month);
            let end = if month == 12 {
                format!("{}-01-01T00:00:00", year + 1)
            } else {
                format!("{}-{:02}-01T00:00:00", year, month + 1)
            };
            let sql = format!(
                "SELECT model, COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + completion_tokens / 1000000.0 * completion_price + cache_hit_input_tokens / 1000000.0 * cache_read_price), 0) FROM usage_logs WHERE timestamp >= ?1 AND timestamp < ?2{} GROUP BY model ORDER BY 2 DESC",
                if uid.is_some() { " AND user_id = ?3" } else { "" }
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut records = Vec::new();
            if let Some(ref uid) = uid {
                let mut rows = stmt.query(params![start, end, uid])?;
                while let Some(row) = rows.next()? {
                    records.push((row.get::<_, String>(0)?, row.get::<_, f64>(1)?));
                }
            } else {
                let mut rows = stmt.query(params![start, end])?;
                while let Some(row) = rows.next()? {
                    records.push((row.get::<_, String>(0)?, row.get::<_, f64>(1)?));
                }
            }
            Ok(records)
        })
        .await
    }

    async fn period_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, f64)>, DbError> {
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let start = format!("{}-{:02}-01T00:00:00", year, month);
            let end = if month == 12 {
                format!("{}-01-01T00:00:00", year + 1)
            } else {
                format!("{}-{:02}-01T00:00:00", year, month + 1)
            };
            let sql = format!(
                "SELECT ul.channel_id, COALESCE(c.name, ul.channel_id), COALESCE(SUM(ul.prompt_tokens / 1000000.0 * ul.prompt_price + ul.completion_tokens / 1000000.0 * ul.completion_price + ul.cache_hit_input_tokens / 1000000.0 * ul.cache_read_price), 0) FROM usage_logs ul LEFT JOIN channels c ON c.id = ul.channel_id WHERE ul.timestamp >= ?1 AND ul.timestamp < ?2{} GROUP BY ul.channel_id, c.name ORDER BY 3 DESC",
                if uid.is_some() { " AND ul.user_id = ?3" } else { "" }
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut records = Vec::new();
            if let Some(ref uid) = uid {
                let mut rows = stmt.query(params![start, end, uid])?;
                while let Some(row) = rows.next()? {
                    records.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, f64>(2)?));
                }
            } else {
                let mut rows = stmt.query(params![start, end])?;
                while let Some(row) = rows.next()? {
                    records.push((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, f64>(2)?));
                }
            }
            Ok(records)
        })
        .await
    }

    async fn daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, f64, u64)>, DbError> {
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let start = format!("{}-{:02}-01T00:00:00", year, month);
            let end = if month == 12 {
                format!("{}-01-01T00:00:00", year + 1)
            } else {
                format!("{}-{:02}-01T00:00:00", year, month + 1)
            };
            let sql = format!(
                "SELECT SUBSTR(timestamp, 1, 10) as day, COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + completion_tokens / 1000000.0 * completion_price + cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), COUNT(*) FROM usage_logs WHERE timestamp >= ?1 AND timestamp < ?2{} GROUP BY day ORDER BY day DESC",
                if uid.is_some() { " AND user_id = ?3" } else { "" }
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut records = Vec::new();
            if let Some(ref uid) = uid {
                let mut rows = stmt.query(params![start, end, uid])?;
                while let Some(row) = rows.next()? {
                    records.push((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, u64>(2)?,
                    ));
                }
            } else {
                let mut rows = stmt.query(params![start, end])?;
                while let Some(row) = rows.next()? {
                    records.push((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, u64>(2)?,
                    ));
                }
            }
            Ok(records)
        })
        .await
    }

    async fn count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<usize, DbError> {
        let uid = user_id.map(|s| s.to_string());
        self.exec(move |conn| {
            let start = format!("{}-{:02}-01T00:00:00", year, month);
            let end = if month == 12 {
                format!("{}-01-01T00:00:00", year + 1)
            } else {
                format!("{}-{:02}-01T00:00:00", year, month + 1)
            };
            let sql = format!(
                "SELECT COUNT(DISTINCT SUBSTR(timestamp, 1, 10)) FROM usage_logs WHERE timestamp >= ?1 AND timestamp < ?2{}",
                if uid.is_some() { " AND user_id = ?3" } else { "" }
            );
            if let Some(ref uid) = uid {
                conn.query_row(&sql, params![start, end, uid], |row| row.get(0))
                    .map_err(|e| DbError(e.to_string()))
            } else {
                conn.query_row(&sql, params![start, end], |row| row.get(0))
                    .map_err(|e| DbError(e.to_string()))
            }
        })
        .await
    }

    async fn daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, f64, u64)>, DbError> {
        let uid = user_id.map(|s| s.to_string());
        let limit_i64 = limit as i64;
        let offset_i64 = offset as i64;
        self.exec(move |conn| {
            let start = format!("{}-{:02}-01T00:00:00", year, month);
            let end = if month == 12 {
                format!("{}-01-01T00:00:00", year + 1)
            } else {
                format!("{}-{:02}-01T00:00:00", year, month + 1)
            };
            let mut records = Vec::new();
            if let Some(ref uid) = uid {
                let mut stmt = conn.prepare(
                    "SELECT SUBSTR(timestamp, 1, 10) as day, COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + completion_tokens / 1000000.0 * completion_price + cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), COUNT(*) FROM usage_logs WHERE timestamp >= ?1 AND timestamp < ?2 AND user_id = ?3 GROUP BY day ORDER BY day DESC LIMIT ?4 OFFSET ?5",
                )?;
                let mut rows = stmt.query(params![start, end, uid, limit_i64, offset_i64])?;
                while let Some(row) = rows.next()? {
                    records.push((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, u64>(2)?,
                    ));
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT SUBSTR(timestamp, 1, 10) as day, COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + completion_tokens / 1000000.0 * completion_price + cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), COUNT(*) FROM usage_logs WHERE timestamp >= ?1 AND timestamp < ?2 GROUP BY day ORDER BY day DESC LIMIT ?3 OFFSET ?4",
                )?;
                let mut rows = stmt.query(params![start, end, limit_i64, offset_i64])?;
                while let Some(row) = rows.next()? {
                    records.push((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, u64>(2)?,
                    ));
                }
            }
            Ok(records)
        })
        .await
    }

    async fn billing_months(&self) -> Result<Vec<String>, DbError> {
        self.exec(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT SUBSTR(timestamp, 1, 7) AS month FROM usage_logs ORDER BY month DESC",
            )?;
            let mut rows = stmt.query([])?;
            let mut months = Vec::new();
            while let Some(row) = rows.next()? {
                months.push(row.get::<_, String>(0)?);
            }
            Ok(months)
        })
        .await
    }

    async fn billing_months_for_user(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        let uid = user_id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT SUBSTR(timestamp, 1, 7) AS month FROM usage_logs WHERE user_id = ?1 ORDER BY month DESC",
            )?;
            let mut rows = stmt.query(rusqlite::params![uid])?;
            let mut months = Vec::new();
            while let Some(row) = rows.next()? {
                months.push(row.get::<_, String>(0)?);
            }
            Ok(months)
        })
        .await
    }

    async fn period_summary_all(&self) -> Result<Vec<(String, f64, u64, u64)>, DbError> {
        self.exec(|conn| {
            let mut stmt = conn.prepare(
                "SELECT SUBSTR(timestamp, 1, 7) AS month, COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + completion_tokens / 1000000.0 * completion_price + cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), COUNT(*), COALESCE(SUM(total_tokens), 0) FROM usage_logs GROUP BY month ORDER BY month DESC",
            )?;
            let mut rows = stmt.query([])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                records.push((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                ));
            }
            Ok(records)
        })
        .await
    }

    async fn period_summary_for_user(&self, user_id: &str) -> Result<Vec<(String, f64, u64, u64)>, DbError> {
        let uid = user_id.to_string();
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT SUBSTR(timestamp, 1, 7) AS month, COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + completion_tokens / 1000000.0 * completion_price + cache_hit_input_tokens / 1000000.0 * cache_read_price), 0), COUNT(*), COALESCE(SUM(total_tokens), 0) FROM usage_logs WHERE user_id = ?1 GROUP BY month ORDER BY month DESC",
            )?;
            let mut rows = stmt.query(rusqlite::params![uid])?;
            let mut records = Vec::new();
            while let Some(row) = rows.next()? {
                records.push((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                ));
            }
            Ok(records)
        })
        .await
    }

    async fn lookup_model_pricing(&self, model_name: &str) -> Result<(f64, f64), DbError> {
        let model_name = model_name.to_string();
        self.exec(move |conn| {
            let result = conn.query_row(
                "SELECT prompt_price, completion_price FROM models WHERE name = ?1",
                params![model_name],
                |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
            );
            match result {
                Ok(p) => Ok(p),
                Err(_) => {
                    let mut stmt = conn.prepare(
                        "SELECT prompt_price, completion_price, model_pattern FROM models",
                    )?;
                    let rows = stmt.query_map([], |row| {
                        Ok((
                            row.get::<_, f64>(0)?,
                            row.get::<_, f64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })?;
                    for row in rows {
                        let (p, c, pattern) = row?;
                        if pattern.ends_with('*') {
                            let prefix = &pattern[..pattern.len() - 1];
                            if model_name.starts_with(prefix) {
                                return Ok((p, c));
                            }
                        }
                        if pattern == model_name {
                            return Ok((p, c));
                        }
                    }
                    Ok((0.0, 0.0))
                }
            }
        })
        .await
    }

    // ── Wallet ───────────────────────────────────────────────────────────

    async fn get_wallet_balance(&self, user_id: &str) -> Result<(f64, f64), DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            conn.query_row(
                "SELECT balance, frozen FROM users WHERE id = ?1",
                params![user_id],
                |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
            )
            .map_err(|e| DbError(e.to_string()))
        })
        .await
    }

    async fn update_wallet_balance(&self, user_id: &str, balance: f64) -> Result<(), DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            conn.execute(
                "UPDATE users SET balance = ?1 WHERE id = ?2",
                params![balance, user_id],
            )?;
            Ok(())
        })
        .await
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
        let id = id.to_string();
        let user_id = user_id.to_string();
        let tx_type = tx_type.to_string();
        let method = method.to_string();
        let status = status.to_string();
        let note = note.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.exec(move |conn| {
            conn.execute(
                "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![id, user_id, tx_type, amount, balance_before, balance_after, method, status, note, now],
            )?;
            Ok(())
        })
        .await
    }

    async fn get_wallet_transactions(
        &self,
        user_id: &str,
        page: usize,
        size: usize,
    ) -> Result<Vec<WalletTransactionRow>, DbError> {
        let user_id = user_id.to_string();
        let offset = (page.saturating_sub(1)) * size;
        let size_i64 = size as i64;
        let offset_i64 = offset as i64;
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at
                 FROM wallet_transactions WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            )?;
            let rows = stmt.query_map(params![user_id, size_i64, offset_i64], |row| {
                Ok(WalletTransactionRow {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    amount: row.get(3)?,
                    balance_before: row.get(4)?,
                    balance_after: row.get(5)?,
                    method: row.get(6)?,
                    status: row.get(7)?,
                    note: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?;
            let mut transactions = Vec::new();
            for row in rows {
                transactions.push(row?);
            }
            Ok(transactions)
        })
        .await
    }

    async fn count_wallet_transactions(&self, user_id: &str) -> Result<usize, DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM wallet_transactions WHERE user_id = ?1",
                params![user_id],
                |row| row.get(0),
            )
            .map_err(|e| DbError(e.to_string()))
        })
        .await
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
        let uid = user_id.map(|s| s.to_string());
        let since = since.map(|s| s.to_string());
        let until = until.map(|s| s.to_string());
        let tx_type = tx_type.map(|s| s.to_string());
        self.exec(move |conn| {
            let mut where_clauses = Vec::new();
            let mut param_values: Vec<String> = Vec::new();
            if let Some(ref uid) = uid {
                where_clauses.push("user_id = ?".to_string());
                param_values.push(uid.clone());
            }
            if let Some(ref s) = since {
                where_clauses.push("created_at >= ?".to_string());
                param_values.push(s.clone());
            }
            if let Some(ref u) = until {
                where_clauses.push("created_at <= ?".to_string());
                param_values.push(u.clone());
            }
            if let Some(ref t) = tx_type {
                where_clauses.push("type = ?".to_string());
                param_values.push(t.clone());
            }
            let where_sql = if where_clauses.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", where_clauses.join(" AND "))
            };

            // Count distinct dates
            let count_sql = format!(
                "SELECT COUNT(DISTINCT substr(created_at,1,10)) FROM wallet_transactions{where_sql}"
            );
            let mut stmt = conn.prepare(&count_sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
            let total_dates: usize = stmt
                .query_row(params.as_slice(), |row| row.get(0))
                .map_err(|e| DbError(e.to_string()))?;

            // Query paginated dates
            let page_offset = (page.saturating_sub(1)) * size;
            let mut date_params = param_values.clone();
            let dates_sql = format!(
                "SELECT DISTINCT substr(created_at,1,10) as tx_date FROM wallet_transactions{where_sql} ORDER BY tx_date DESC LIMIT ? OFFSET ?"
            );
            date_params.push(size.to_string());
            date_params.push(page_offset.to_string());
            let mut stmt = conn.prepare(&dates_sql)?;
            let date_params_refs: Vec<&dyn rusqlite::types::ToSql> =
                date_params.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
            let dates: Vec<String> = stmt
                .query_map(date_params_refs.as_slice(), |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| DbError(e.to_string()))?;

            if dates.is_empty() {
                return Ok((Vec::new(), total_dates));
            }

            // Fetch txns for those dates
            let placeholders: Vec<String> = (0..dates.len())
                .map(|i| format!("?{}", param_values.len() + i + 1))
                .collect();
            let mut tx_params = param_values.clone();
            tx_params.extend(dates.iter().cloned());
            let tx_sql = format!(
                "SELECT id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at \
                 FROM wallet_transactions{where_sql} AND substr(created_at,1,10) IN ({}) \
                 ORDER BY created_at DESC",
                placeholders.join(",")
            );
            let tx_params_refs: Vec<&dyn rusqlite::types::ToSql> =
                tx_params.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
            let mut stmt = conn.prepare(&tx_sql)?;
            let mut rows = Vec::new();
            for row in stmt.query_map(tx_params_refs.as_slice(), |row| {
                Ok(WalletTransactionRow {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    amount: row.get(3)?,
                    balance_before: row.get(4)?,
                    balance_after: row.get(5)?,
                    method: row.get(6)?,
                    status: row.get(7)?,
                    note: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })? {
                rows.push(row?);
            }
            Ok((rows, total_dates))
        })
        .await
    }

    async fn get_total_consumed(&self, user_id: &str) -> Result<f64, DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + completion_tokens / 1000000.0 * completion_price + cache_hit_input_tokens / 1000000.0 * cache_read_price), 0)
                     FROM usage_logs WHERE user_id = ?1",
                    params![user_id],
                    |row| row.get::<_, f64>(0),
                )
                .unwrap_or(0.0))
        })
        .await
    }

    async fn get_total_recharged(&self, user_id: &str) -> Result<f64, DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            conn.query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM wallet_transactions WHERE user_id = ?1 AND type = 'recharge' AND status = 'completed'",
                params![user_id],
                |row| row.get::<_, f64>(0),
            )
            .map_err(|e| DbError(e.to_string()))
        })
        .await
    }

    async fn get_wallet_estimated_days(&self, user_id: &str) -> Result<Option<f64>, DbError> {
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            let thirty_days_ago =
                (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
            let total_cost: f64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(prompt_tokens / 1000000.0 * prompt_price + completion_tokens / 1000000.0 * completion_price + cache_hit_input_tokens / 1000000.0 * cache_read_price), 0)
                     FROM usage_logs WHERE user_id = ?1 AND timestamp >= ?2",
                    params![user_id, thirty_days_ago],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);
            let balance: f64 = conn
                .query_row(
                    "SELECT balance FROM users WHERE id = ?1",
                    params![user_id],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);
            let daily_avg = total_cost / 30.0;
            if daily_avg <= 0.0 {
                return Ok(None);
            }
            Ok(Some(balance / daily_avg))
        })
        .await
    }

    // ── Recharge Keys ────────────────────────────────────────────────────

    async fn create_recharge_key(
        &self,
        key: &str,
        amount: f64,
        created_by: &str,
        expires_at: Option<&str>,
    ) -> Result<(), DbError> {
        let key = key.to_string();
        let created_by = created_by.to_string();
        let expires_at = expires_at.map(|s| s.to_string());
        let now = chrono::Utc::now().to_rfc3339();
        self.exec(move |conn| {
            conn.execute(
                "INSERT INTO recharge_keys (key, amount, created_by, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![key, amount, created_by, now, expires_at],
            )?;
            Ok(())
        })
        .await
    }

    async fn redeem_recharge_key(&self, key: &str, user_id: &str) -> Result<f64, DbError> {
        let key = key.to_string();
        let user_id = user_id.to_string();
        self.exec(move |conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| DbError(format!("Failed to begin transaction: {}", e)))?;

            // Atomically claim the key with conditional UPDATE
            let now = chrono::Utc::now().to_rfc3339();
            let rows = tx.execute(
                "UPDATE recharge_keys SET used_by = ?1, used_at = ?2 WHERE key = ?3 AND used_by IS NULL AND (revoked IS NULL OR revoked = 0)",
                params![user_id, now, key],
            )?;

            if rows == 0 {
                return Err(DbError("Invalid or already used recharge key".to_string()));
            }

            // Read amount and validate
            let (amount, expires_at, revoked): (f64, Option<String>, i64) = tx
                .query_row(
                    "SELECT amount, expires_at, COALESCE(revoked, 0) FROM recharge_keys WHERE key = ?1",
                    params![key],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)?)),
                )
                .map_err(|_| DbError("Invalid recharge key".to_string()))?;

            if revoked != 0 {
                return Err(DbError("Recharge key has been revoked".to_string()));
            }
            if let Some(exp) = &expires_at {
                if let Ok(exp_time) = chrono::DateTime::parse_from_rfc3339(exp) {
                    if chrono::Utc::now() > exp_time {
                        return Err(DbError("Recharge key has expired".to_string()));
                    }
                }
            }

            // Add balance
            let (balance,): (f64,) = tx
                .query_row(
                    "SELECT balance FROM users WHERE id = ?1",
                    params![user_id],
                    |row| Ok((row.get::<_, f64>(0)?,)),
                )
                .map_err(|_| DbError("User not found".to_string()))?;

            let new_balance = balance + amount;
            tx.execute(
                "UPDATE users SET balance = ?1 WHERE id = ?2",
                params![new_balance, user_id],
            )?;

            // Record transaction
            tx.execute(
                "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    user_id,
                    "recharge",
                    amount,
                    balance,
                    new_balance,
                    "recharge_key",
                    "completed",
                    format!("Key recharge: {}", key),
                    now,
                ],
            )?;

            tx.commit()
                .map_err(|e| DbError(format!("Failed to commit: {}", e)))?;
            Ok(amount)
        })
        .await
    }

    async fn revoke_recharge_key(&self, key: &str) -> Result<(), DbError> {
        let key = key.to_string();
        self.exec(move |conn| {
            let rows = conn.execute(
                "UPDATE recharge_keys SET revoked = 1 WHERE key = ?1 AND used_by IS NULL AND (revoked IS NULL OR revoked = 0)",
                params![key],
            )?;
            if rows == 0 {
                return Err(DbError(
                    "Key not found or already used/revoked".to_string(),
                ));
            }
            Ok(())
        })
        .await
    }

    async fn list_recharge_keys(&self) -> Result<Vec<RechargeKeyRow>, DbError> {
        self.exec(|conn| {
            let mut stmt = conn.prepare(
                "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked FROM recharge_keys ORDER BY created_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(RechargeKeyRow {
                    key: row.get(0)?,
                    amount: row.get(1)?,
                    used_by: row.get(2)?,
                    used_at: row.get(3)?,
                    created_by: row.get(4)?,
                    created_at: row.get(5)?,
                    expires_at: row.get(6)?,
                    revoked: row.get::<_, i64>(7).unwrap_or(0) != 0,
                })
            })?;
            let mut keys = Vec::new();
            for row in rows {
                keys.push(row?);
            }
            Ok(keys)
        })
        .await
    }

    async fn list_recharge_keys_paginated(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<RechargeKeyRow>, DbError> {
        let limit_i64 = limit as i64;
        let offset_i64 = offset as i64;
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked FROM recharge_keys ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
            )?;
            let rows = stmt.query_map(params![limit_i64, offset_i64], |row| {
                Ok(RechargeKeyRow {
                    key: row.get(0)?,
                    amount: row.get(1)?,
                    used_by: row.get(2)?,
                    used_at: row.get(3)?,
                    created_by: row.get(4)?,
                    created_at: row.get(5)?,
                    expires_at: row.get(6)?,
                    revoked: row.get::<_, i64>(7).unwrap_or(0) != 0,
                })
            })?;
            let mut keys = Vec::new();
            for row in rows {
                keys.push(row?);
            }
            Ok(keys)
        })
        .await
    }

    async fn count_recharge_keys_filtered(
        &self,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<usize, DbError> {
        let search = search.map(|s| s.to_string());
        let user_search = user_search.map(|s| s.to_string());
        let status = status.map(|s| s.to_string());
        let now = chrono::Utc::now().to_rfc3339();
        self.exec(move |conn| {
            let (where_clause, param_values) = Self::build_recharge_key_filter(
                search.as_deref(),
                status.as_deref(),
                user_search.as_deref(),
                &now,
            );
            let sql = format!("SELECT COUNT(*) FROM recharge_keys {}", where_clause);
            let mut stmt = conn.prepare(&sql)?;
            let params_refs: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p as &dyn rusqlite::types::ToSql).collect();
            stmt.query_row(params_refs.as_slice(), |row| row.get(0))
                .map_err(|e| DbError(e.to_string()))
        })
        .await
    }

    async fn list_recharge_keys_filtered(
        &self,
        limit: usize,
        offset: usize,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<Vec<RechargeKeyRow>, DbError> {
        let search = search.map(|s| s.to_string());
        let user_search = user_search.map(|s| s.to_string());
        let status = status.map(|s| s.to_string());
        let now = chrono::Utc::now().to_rfc3339();
        let limit_i64 = limit as i64;
        let offset_i64 = offset as i64;
        self.exec(move |conn| {
            let (where_clause, mut param_values) = Self::build_recharge_key_filter(
                search.as_deref(),
                status.as_deref(),
                user_search.as_deref(),
                &now,
            );
            let sql = format!(
                "SELECT key, amount, used_by, used_at, created_by, created_at, expires_at, revoked FROM recharge_keys {} ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
                where_clause,
                param_values.len() + 1,
                param_values.len() + 2,
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut params_refs: Vec<&dyn rusqlite::types::ToSql> =
                param_values.iter().map(|p| p as &dyn rusqlite::types::ToSql).collect();
            params_refs.push(&limit_i64);
            params_refs.push(&offset_i64);
            let rows = stmt.query_map(params_refs.as_slice(), |row| {
                Ok(RechargeKeyRow {
                    key: row.get(0)?,
                    amount: row.get(1)?,
                    used_by: row.get(2)?,
                    used_at: row.get(3)?,
                    created_by: row.get(4)?,
                    created_at: row.get(5)?,
                    expires_at: row.get(6)?,
                    revoked: row.get::<_, i64>(7).unwrap_or(0) != 0,
                })
            })?;
            let mut keys = Vec::new();
            for row in rows {
                keys.push(row?);
            }
            Ok(keys)
        })
        .await
    }

    // ── Settings ─────────────────────────────────────────────────────────

    async fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let key = key.to_string();
        self.exec(move |conn| {
            let result = conn
                .query_row(
                    "SELECT value FROM balancer_settings WHERE key = ?1",
                    params![key],
                    |row| row.get::<_, String>(0),
                )
                .ok();
            Ok(result)
        })
        .await
    }

    async fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        let key = key.to_string();
        let value = value.to_string();
        self.exec(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO balancer_settings (key, value) VALUES (?1, ?2)",
                params![key, value],
            )?;
            Ok(())
        })
        .await
    }

    async fn get_gateway_config(&self) -> Result<GatewayRuntimeConfig, DbError> {
        let result = self.get_setting("gateway_config").await?;
        match result {
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
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, pattern_type, pattern, action, scope, channel_id, replacement, enabled, priority, created_at, updated_at FROM content_filter_rules ORDER BY priority ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(ContentFilterRule {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    pattern_type: row.get(2)?,
                    pattern: row.get(3)?,
                    action: row.get(4)?,
                    scope: row.get(5)?,
                    channel_id: row.get(6)?,
                    replacement: row.get(7)?,
                    enabled: row.get::<_, i32>(8)? != 0,
                    priority: row.get(9)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                })
            })?;
            let mut rules = Vec::new();
            for row in rows {
                rules.push(row?);
            }
            Ok(rules)
        })
        .await
    }

    async fn create_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError> {
        let rule = rule.clone();
        self.exec(move |conn| {
            conn.execute(
                "INSERT INTO content_filter_rules (id, name, pattern_type, pattern, action, scope, channel_id, replacement, enabled, priority, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    rule.id, rule.name, rule.pattern_type, rule.pattern,
                    rule.action, rule.scope, rule.channel_id, rule.replacement,
                    rule.enabled as i32, rule.priority, rule.created_at, rule.updated_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

    async fn update_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError> {
        let rule = rule.clone();
        self.exec(move |conn| {
            conn.execute(
                "UPDATE content_filter_rules SET name=?1, pattern_type=?2, pattern=?3, action=?4, scope=?5, channel_id=?6, replacement=?7, enabled=?8, priority=?9, updated_at=?10 WHERE id=?11",
                params![
                    rule.name, rule.pattern_type, rule.pattern, rule.action,
                    rule.scope, rule.channel_id, rule.replacement,
                    rule.enabled as i32, rule.priority, rule.updated_at, rule.id,
                ],
            )?;
            Ok(())
        })
        .await
    }

    async fn delete_filter_rule(&self, id: &str) -> Result<(), DbError> {
        let id = id.to_string();
        self.exec(move |conn| {
            conn.execute("DELETE FROM content_filter_rules WHERE id = ?1", params![id])?;
            Ok(())
        })
        .await
    }

    // ── Health Probe Results ─────────────────────────────────────────

    async fn insert_probe_result(&self, row: &ProbeResultRow) -> Result<(), DbError> {
        let row = row.clone();
        self.exec(move |conn| {
            conn.execute(
                "INSERT INTO probe_results (id, channel_id, model_id, success, latency_ms, error, probed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![row.id, row.channel_id, row.model_id, row.success as i32, row.latency_ms, row.error, row.probed_at],
            )?;
            Ok(())
        })
        .await
    }

    async fn all_latest_probe_results(&self) -> Result<Vec<ProbeResultRow>, DbError> {
        self.exec(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT p.id, p.channel_id, p.model_id, p.success, p.latency_ms, p.error, p.probed_at
                 FROM probe_results p
                 INNER JOIN (
                     SELECT channel_id, MAX(probed_at) AS max_ts
                     FROM probe_results
                     GROUP BY channel_id
                 ) latest ON p.channel_id = latest.channel_id AND p.probed_at = latest.max_ts
                 ORDER BY p.channel_id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(ProbeResultRow {
                    id: row.get(0)?,
                    channel_id: row.get(1)?,
                    model_id: row.get(2)?,
                    success: row.get::<_, i32>(3)? != 0,
                    latency_ms: row.get(4)?,
                    error: row.get(5)?,
                    probed_at: row.get(6)?,
                })
            })?;
            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }
            Ok(results)
        })
        .await
    }

    async fn get_balances_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, f64, f64)>, DbError> {
        let limit_i64 = limit as i64;
        let offset_i64 = offset as i64;
        self.exec(move |conn| {
            let mut stmt =
                conn.prepare("SELECT id, balance, frozen FROM users LIMIT ?1 OFFSET ?2")?;
            let rows = stmt.query_map(params![limit_i64, offset_i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })?;
            let mut balances = Vec::new();
            for row in rows {
                balances.push(row?);
            }
            Ok(balances)
        })
        .await
    }

    // ── Batch Operations ────────────────────────────────────────────────

    async fn batch_insert_usage_with_billing(
        &self,
        batch: &[UsageRecord],
        billing_enabled: bool,
    ) -> Result<Vec<(String, f64, f64)>, DbError> {
        let batch = batch.to_vec();
        self.exec(move |conn| {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| DbError(format!("Failed to begin transaction: {}", e)))?;

            let mut deductions: Vec<(String, f64, f64)> = Vec::new();

            for record in &batch {
                let (prompt_price, completion_price, cache_read_price) =
                    Self::pricing_lookup(&tx, &record.model);

                // Insert usage record with pricing snapshot
                tx.execute(
                    "INSERT INTO usage_logs (timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, request_body, response_body, reasoning_body, api_key_name, api_format, stream, cache_hit_input_tokens, prompt_price, completion_price, client_ip, cache_read_price)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
                    params![
                        record.timestamp, record.request_id, record.user_id, record.user_name,
                        record.channel_id, record.model, record.prompt_tokens, record.completion_tokens,
                        record.total_tokens, record.latency_ms, record.status_code, record.success as i32,
                        record.request_body, record.response_body, record.reasoning_body,
                        record.api_key_name, record.api_format, record.stream as i32,
                        record.cache_hit_input_tokens, prompt_price, completion_price,
                        record.client_ip, cache_read_price,
                    ],
                )?;

                if billing_enabled {
                    // Calculate cost including cache hits
                    let cost = record.prompt_tokens as f64 / 1000000.0 * prompt_price
                        + record.completion_tokens as f64 / 1000000.0 * completion_price
                        + record.cache_hit_input_tokens as f64 / 1000000.0 * cache_read_price;

                    if cost > 0.0 {
                        let (balance, frozen): (f64, f64) = tx
                            .query_row(
                                "SELECT balance, frozen FROM users WHERE id = ?1",
                                params![record.user_id],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .unwrap_or((0.0, 0.0));

                        let new_balance = balance - cost;
                        tx.execute(
                            "UPDATE users SET balance = ?1 WHERE id = ?2",
                            params![new_balance, record.user_id],
                        )?;

                        let now = chrono::Utc::now().to_rfc3339();
                        tx.execute(
                            "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                            params![
                                uuid::Uuid::new_v4().to_string(),
                                record.user_id,
                                "deduction",
                                -cost,
                                balance,
                                new_balance,
                                "usage",
                                "completed",
                                format!("Usage: {}", record.model),
                                now,
                            ],
                        )?;

                        deductions.push((record.user_id.clone(), new_balance, frozen));
                    }
                }
            }

            tx.commit()
                .map_err(|e| DbError(format!("Failed to commit batch: {}", e)))?;
            Ok(deductions)
        })
        .await
    }
}
