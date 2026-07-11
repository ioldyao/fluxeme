mod channels;
mod models;
mod rules;
mod users;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::domain::channel::{Channel, Endpoint};
use crate::domain::model::{Model, Pricing};
use crate::domain::routing::RoutingRule;
use crate::domain::usage::UsageRecord;
use crate::domain::user::{ApiKey, User};

#[derive(Debug)]
pub struct DbError(pub String);

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        Self(e.to_string())
    }
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn new(path: &str) -> Self {
        let path = path.to_string();
        let exists = Path::new(&path).exists();
        let conn = Connection::open(&path)
            .unwrap_or_else(|e| panic!("Failed to open database at {}: {}", path, e));
        if !exists {
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
                .unwrap_or_else(|e| panic!("Failed to set pragmas: {}", e));
            Self::migrate_inner(&conn)
                .unwrap_or_else(|e| panic!("Failed to run initial migration: {}", e));
            tracing::info!("Database created at {}", path);
        }
        Self {
            conn: Mutex::new(conn),
        }
    }

    pub fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, DbError> {
        self.conn
            .lock()
            .map_err(|_| DbError("Database mutex poisoned".into()))
    }

    fn migrate_inner(conn: &Connection) -> Result<(), DbError> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                password_hash TEXT NOT NULL DEFAULT '',
                rpm INTEGER,
                tpm INTEGER
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
                completion_price REAL NOT NULL DEFAULT 0.0,
                cache_read_price REAL NOT NULL DEFAULT 0.0,
                cache_write_price REAL NOT NULL DEFAULT 0.0,
                image_input_price REAL NOT NULL DEFAULT 0.0,
                audio_input_price REAL NOT NULL DEFAULT 0.0,
                audio_output_price REAL NOT NULL DEFAULT 0.0
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
                api_key_name TEXT
            );
            ",
        )?;
        // Backward compat: add password_hash column to existing users table
        let _ = conn
            .execute_batch("ALTER TABLE users ADD COLUMN password_hash TEXT NOT NULL DEFAULT '';");
        // Backward compat: add request_body/response_body columns
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN request_body TEXT;");
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN response_body TEXT;");
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN reasoning_body TEXT;");
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN api_key_name TEXT;");
        // Backward compat: add published column to models
        let _ = conn
            .execute_batch("ALTER TABLE models ADD COLUMN published INTEGER NOT NULL DEFAULT 0;");
        // Backward compat: add context_length column to models
        let _ = conn.execute_batch("ALTER TABLE models ADD COLUMN context_length INTEGER;");
        // Backward compat: add pricing columns to models
        let _ = conn.execute_batch(
            "ALTER TABLE models ADD COLUMN cache_read_price REAL NOT NULL DEFAULT 0.0;",
        );
        let _ = conn.execute_batch(
            "ALTER TABLE models ADD COLUMN cache_write_price REAL NOT NULL DEFAULT 0.0;",
        );
        let _ = conn.execute_batch(
            "ALTER TABLE models ADD COLUMN image_input_price REAL NOT NULL DEFAULT 0.0;",
        );
        let _ = conn.execute_batch(
            "ALTER TABLE models ADD COLUMN audio_input_price REAL NOT NULL DEFAULT 0.0;",
        );
        let _ = conn.execute_batch(
            "ALTER TABLE models ADD COLUMN audio_output_price REAL NOT NULL DEFAULT 0.0;",
        );
        // Backward compat: add name column to channels
        let _ =
            conn.execute_batch("ALTER TABLE channels ADD COLUMN name TEXT NOT NULL DEFAULT '';");
        // Backward compat: add spend_limit/allowed_models columns to api_keys
        let _ = conn.execute_batch("ALTER TABLE api_keys ADD COLUMN spend_limit REAL;");
        let _ = conn.execute_batch("ALTER TABLE api_keys ADD COLUMN allowed_models TEXT;");
        // User model subscriptions
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS user_subscriptions (
                user_id TEXT NOT NULL,
                model_id TEXT NOT NULL REFERENCES models(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL,
                PRIMARY KEY (user_id, model_id)
            );",
        );
        // Performance indexes
        let _ = conn
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_usage_user_id ON usage_logs(user_id)");
        let _ = conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_logs(timestamp)",
        );
        let _ = conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_usage_user_timestamp ON usage_logs(user_id, timestamp)",
        );
        // Backward compat: add enabled column to endpoints
        let _ = conn
            .execute_batch("ALTER TABLE endpoints ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;");
        // Backward compat: add category column to models
        let _ = conn
            .execute_batch("ALTER TABLE models ADD COLUMN category TEXT NOT NULL DEFAULT '';");
        // Backward compat: add api_format column to usage_logs
        let _ = conn.execute_batch("ALTER TABLE usage_logs ADD COLUMN api_format TEXT NOT NULL DEFAULT '';");
        // Backward compat: add timezone column to users
        let _ = conn
            .execute_batch("ALTER TABLE users ADD COLUMN timezone TEXT NOT NULL DEFAULT 'UTC';");
        // Balancer settings table
        let _ = conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS balancer_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        );
        Ok(())
    }

    pub fn migrate(&self) -> Result<(), DbError> {
        Self::migrate_inner(&*self.conn()?)
    }

    // ── Delegating helpers ──────────────────────────────────────────

    pub fn list_users(&self) -> Result<Vec<User>, DbError> {
        users::list(&*self.conn()?)
    }
    pub fn get_user(&self, id: &str) -> Result<Option<User>, DbError> {
        users::get(&*self.conn()?, id)
    }
    pub fn get_user_with_password(&self, id: &str) -> Result<Option<User>, DbError> {
        users::get_with_password(&*self.conn()?, id)
    }
    pub fn create_user(&self, user: &User) -> Result<(), DbError> {
        users::create(&*self.conn()?, user)
    }
    pub fn update_user(&self, user: &User) -> Result<(), DbError> {
        users::update(&*self.conn()?, user)
    }
    pub fn get_user_timezone(&self, id: &str) -> Result<String, DbError> {
        users::get_timezone(&*self.conn()?, id)
    }
    pub fn update_user_timezone(&self, id: &str, timezone: &str) -> Result<(), DbError> {
        users::update_timezone(&*self.conn()?, id, timezone)
    }
    pub fn delete_user(&self, id: &str) -> Result<(), DbError> {
        users::delete(&*self.conn()?, id)
    }
    pub fn list_api_keys(&self, user_id: &str) -> Result<Vec<ApiKey>, DbError> {
        users::list_api_keys(&*self.conn()?, user_id)
    }
    pub fn create_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        users::create_api_key(&*self.conn()?, key)
    }
    pub fn delete_api_key(&self, key: &str) -> Result<(), DbError> {
        users::delete_api_key(&*self.conn()?, key)
    }
    pub fn update_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        users::update_api_key(&*self.conn()?, key)
    }
    #[allow(dead_code)]
    pub fn lookup_key(&self, key: &str) -> Result<Option<(User, ApiKey)>, DbError> {
        users::lookup_key(&*self.conn()?, key)
    }
    pub fn all_api_keys(&self) -> Result<Vec<(User, ApiKey)>, DbError> {
        users::all_api_keys(&*self.conn()?)
    }

    // ── Unused helpers (available for future use) ────────────────
    pub fn insert_usage(&self, record: &crate::domain::usage::UsageRecord) -> Result<(), DbError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO usage_logs (timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, request_body, response_body, reasoning_body, api_key_name, api_format)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            rusqlite::params![
                record.timestamp,
                record.request_id,
                record.user_id,
                record.user_name,
                record.channel_id,
                record.model,
                record.prompt_tokens,
                record.completion_tokens,
                record.total_tokens,
                record.latency_ms,
                record.status_code,
                record.success as i32,
                record.request_body,
                record.response_body,
                record.reasoning_body,
                record.api_key_name,
                record.api_format,
            ],
        )?;
        Ok(())
    }
    pub fn count_usage(&self) -> Result<usize, DbError> {
        let conn = self.conn()?;
        Ok(conn.query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))?)
    }
    pub fn count_usage_by_user(&self, user_id: &str) -> Result<usize, DbError> {
        let conn = self.conn()?;
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM usage_logs WHERE user_id = ?1",
            [user_id],
            |row| row.get(0),
        )?)
    }
    pub fn count_usage_filtered(&self, user_id: Option<&str>) -> Result<usize, DbError> {
        let conn = self.conn()?;
        if let Some(uid) = user_id {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM usage_logs WHERE user_id = ?1",
                [uid],
                |row| row.get(0),
            )?)
        } else {
            Ok(conn.query_row("SELECT COUNT(*) FROM usage_logs", [], |row| row.get(0))?)
        }
    }
    pub fn query_usage_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<crate::domain::usage::UsageRecord>, DbError> {
        use crate::domain::usage::UsageRecord;
        let conn = self.conn()?;
        let mut records = Vec::new();
        if let Some(uid) = user_id {
            let mut stmt = conn.prepare(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format
                 FROM usage_logs WHERE user_id = ?1 AND timestamp >= ?2 ORDER BY id ASC",
            )?;
            let mut rows = stmt.query(rusqlite::params![uid, since])?;
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
                });
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format
                 FROM usage_logs WHERE timestamp >= ?1 ORDER BY id ASC",
            )?;
            let mut rows = stmt.query(rusqlite::params![since])?;
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
                });
            }
        }
        Ok(records)
    }
    pub fn daily_usage_counts(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, i64)>, DbError> {
        let conn = self.conn()?;
        let mut records = Vec::new();
        let offset_expr = if tz_offset_seconds >= 0 {
            format!("datetime(timestamp, '+{} seconds')", tz_offset_seconds)
        } else {
            format!("datetime(timestamp, '-{} seconds')", -tz_offset_seconds)
        };
        let day_expr = format!("substr({}, 1, 10)", offset_expr);
        if let Some(uid) = user_id {
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
    }

    pub fn daily_usage_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64)>, DbError> {
        let conn = self.conn()?;
        let mut records = Vec::new();
        let offset_expr = if tz_offset_seconds >= 0 {
            format!("datetime(timestamp, '+{} seconds')", tz_offset_seconds)
        } else {
            format!("datetime(timestamp, '-{} seconds')", -tz_offset_seconds)
        };
        let day_expr = format!("substr({}, 1, 10)", offset_expr);
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = if let Some(uid) = user_id {
            (
                format!(
                    "SELECT {} as day, COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0), COALESCE(SUM(CASE WHEN success=1 THEN 1 ELSE 0 END),0), COALESCE(SUM(latency_ms),0) FROM usage_logs WHERE user_id = ?1 AND timestamp >= ?2 GROUP BY day ORDER BY day ASC",
                    day_expr
                ),
                vec![Box::new(uid.to_string()), Box::new(since.to_string())],
            )
        } else {
            (
                format!(
                    "SELECT {} as day, COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0), COALESCE(SUM(total_tokens),0), COALESCE(SUM(CASE WHEN success=1 THEN 1 ELSE 0 END),0), COALESCE(SUM(latency_ms),0) FROM usage_logs WHERE timestamp >= ?1 GROUP BY day ORDER BY day ASC",
                    day_expr
                ),
                vec![Box::new(since.to_string())],
            )
        };
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
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
            ));
        }
        Ok(records)
    }

    pub fn query_usage(
        &self,
        limit: usize,
        offset: usize,
        user_id: Option<&str>,
    ) -> Result<Vec<crate::domain::usage::UsageRecord>, DbError> {
        use crate::domain::usage::UsageRecord;
        let conn = self.conn()?;
        let mut records = Vec::new();

        if let Some(uid) = user_id {
            let mut stmt = conn.prepare(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format
                 FROM usage_logs WHERE user_id = ?1 ORDER BY id DESC LIMIT ?2 OFFSET ?3",
            )?;
            let mut rows = stmt.query(rusqlite::params![uid, limit as i64, offset as i64])?;
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
                });
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format
                 FROM usage_logs ORDER BY id DESC LIMIT ?1 OFFSET ?2",
            )?;
            let mut rows = stmt.query(rusqlite::params![limit as i64, offset as i64])?;
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
                });
            }
        }
        Ok(records)
    }

    pub fn get_usage_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<crate::domain::usage::UsageRecord>, DbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, request_body, response_body, reasoning_body, api_key_name, api_format
             FROM usage_logs WHERE request_id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![request_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(crate::domain::usage::UsageRecord {
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
            }))
        } else {
            Ok(None)
        }
    }

    // Channels
    pub fn list_channels(&self) -> Result<Vec<Channel>, DbError> {
        channels::list(&*self.conn()?)
    }
    #[allow(dead_code)]
    pub fn get_channel(&self, id: &str) -> Result<Option<Channel>, DbError> {
        channels::get(&*self.conn()?, id)
    }
    pub fn create_channel(&self, ch: &Channel) -> Result<(), DbError> {
        channels::create(&*self.conn()?, ch)
    }
    pub fn update_channel(&self, ch: &Channel) -> Result<(), DbError> {
        channels::update(&*self.conn()?, ch)
    }
    pub fn delete_channel(&self, id: &str) -> Result<(), DbError> {
        channels::delete(&*self.conn()?, id)
    }
    pub fn get_endpoint(&self, id: i64) -> Result<Option<Endpoint>, DbError> {
        channels::get_endpoint(&*self.conn()?, id)
    }
    pub fn update_endpoint_enabled(&self, id: i64, enabled: bool) -> Result<(), DbError> {
        channels::update_endpoint_enabled(&*self.conn()?, id, enabled)
    }

    // Models
    pub fn list_models(&self) -> Result<Vec<Model>, DbError> {
        models::list(&*self.conn()?)
    }
    #[allow(dead_code)]
    pub fn get_model(&self, id: &str) -> Result<Option<Model>, DbError> {
        models::get(&*self.conn()?, id)
    }
    pub fn create_model(&self, m: &Model) -> Result<(), DbError> {
        models::create(&*self.conn()?, m)
    }
    pub fn update_model(&self, m: &Model) -> Result<(), DbError> {
        models::update(&*self.conn()?, m)
    }
    pub fn delete_model(&self, id: &str) -> Result<(), DbError> {
        models::delete(&*self.conn()?, id)
    }

    // Routing rules
    pub fn list_rules(&self) -> Result<Vec<RoutingRule>, DbError> {
        rules::list(&*self.conn()?)
    }
    pub fn create_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        rules::create(&*self.conn()?, r)
    }
    pub fn update_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        rules::update(&*self.conn()?, r)
    }
    pub fn delete_rule(&self, name: &str) -> Result<(), DbError> {
        rules::delete(&*self.conn()?, name)
    }

    // Subscriptions
    pub fn list_published_models(&self) -> Result<Vec<Model>, DbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, name, model_pattern, prompt_price, completion_price, cache_read_price, cache_write_price, image_input_price, audio_input_price, audio_output_price, published, context_length, category FROM models WHERE published = 1 ORDER BY id")?;
        let models: Vec<Model> = stmt
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

        let mut result = Vec::new();
        for mut m in models {
            m.channels = models::list_bindings(&conn, &m.id)?;
            result.push(m);
        }
        Ok(result)
    }

    pub fn set_model_published(&self, id: &str, published: bool) -> Result<(), DbError> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE models SET published = ?1 WHERE id = ?2",
            params![published as i32, id],
        )?;
        Ok(())
    }

    pub fn set_model_pricing(&self, id: &str, pricing: &Pricing) -> Result<(), DbError> {
        models::update_pricing(&*self.conn()?, id, pricing)
    }

    pub fn set_model_context_length(&self, id: &str, context_length: i64) -> Result<(), DbError> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE models SET context_length = ?1 WHERE id = ?2",
            params![context_length, id],
        )?;
        Ok(())
    }

    pub fn subscribe_user(&self, user_id: &str, model_id: &str) -> Result<(), DbError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR IGNORE INTO user_subscriptions (user_id, model_id, created_at) VALUES (?1, ?2, ?3)",
            params![user_id, model_id, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn unsubscribe_user(&self, user_id: &str, model_id: &str) -> Result<(), DbError> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM user_subscriptions WHERE user_id = ?1 AND model_id = ?2",
            params![user_id, model_id],
        )?;
        Ok(())
    }

    pub fn delete_subscriptions_by_model(&self, model_id: &str) -> Result<(), DbError> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM user_subscriptions WHERE model_id = ?1",
            params![model_id],
        )?;
        Ok(())
    }

    pub fn list_subscribed_model_ids(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT model_id FROM user_subscriptions WHERE user_id = ?1")?;
        let ids = stmt
            .query_map(params![user_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    pub fn list_subscriptions(&self, user_id: &str) -> Result<Vec<Model>, DbError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT m.id, m.name, m.model_pattern, m.prompt_price, m.completion_price, m.cache_read_price, m.cache_write_price, m.image_input_price, m.audio_input_price, m.audio_output_price, m.published, m.context_length, m.category
             FROM models m INNER JOIN user_subscriptions s ON m.id = s.model_id
             WHERE s.user_id = ?1 ORDER BY m.id",
        )?;
        let models: Vec<Model> = stmt
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

        let mut result = Vec::new();
        for mut m in models {
            m.channels = models::list_bindings(&conn, &m.id)?;
            result.push(m);
        }
        Ok(result)
    }

    /// Delete usage log records older than the given cutoff timestamp.
    /// Returns the number of deleted rows.
    /// Get a balancer setting by key.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn()?;
        let result = conn
            .query_row(
                "SELECT value FROM balancer_settings WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .ok();
        Ok(result)
    }

    /// Set a balancer setting (upsert).
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO balancer_settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn purge_usage_logs(&self, cutoff: &str) -> Result<usize, DbError> {
        let conn = self.conn()?;
        let count = conn.execute(
            "DELETE FROM usage_logs WHERE timestamp < ?1",
            rusqlite::params![cutoff],
        )?;
        Ok(count)
    }

    pub fn usage_stats_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<(u64, u64, u64, u64), DbError> {
        let conn = self.conn()?;
        let (total, success, latency, total_tok): (u64, u64, u64, u64) = if let Some(uid) = user_id
        {
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
    }

    pub fn usage_cost_rows_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, DbError> {
        let conn = self.conn()?;
        let mut records = Vec::new();
        if let Some(uid) = user_id {
            let mut stmt = conn.prepare(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format
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
                });
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, api_key_name, api_format
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
                });
            }
        }
        Ok(records)
    }
}

pub fn insert_usage_row(conn: &Connection, record: &UsageRecord) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO usage_logs (timestamp, request_id, user_id, user_name, channel_id, model, prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, request_body, response_body, reasoning_body, api_key_name, api_format)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            record.timestamp,
            record.request_id,
            record.user_id,
            record.user_name,
            record.channel_id,
            record.model,
            record.prompt_tokens,
            record.completion_tokens,
            record.total_tokens,
            record.latency_ms,
            record.status_code,
            record.success as i32,
            record.request_body,
            record.response_body,
            record.reasoning_body,
            record.api_key_name,
            record.api_format,
        ],
    )?;
    Ok(())
}
