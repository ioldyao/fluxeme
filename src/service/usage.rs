use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::JoinHandle;

use crate::cache::{compute_gate_status, RedisCache};
use crate::config::types::GatewayRuntimeConfig;
use crate::db::Database;
use crate::domain::usage::UsageFilter;
use crate::domain::usage::UsageRecord;
use rusqlite::params;
use uuid::Uuid;
use chrono::Utc;

#[derive(Clone)]
pub struct UsageService {
    sender: Sender<UsageRecord>,
    db: Arc<Database>,
    cache: Arc<RedisCache>,
}

impl UsageService {
    pub fn new(db: Arc<Database>, cache: Arc<RedisCache>) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel::<UsageRecord>(4096);
        let handle = tokio::spawn(background_writer(db.clone(), cache.clone(), rx));

        (Self { sender: tx, db, cache }, handle)
    }

    pub fn record(&self, record: UsageRecord) {
        if let Err(e) = self.sender.try_send(record) {
            tracing::warn!("Usage channel full, dropping record: {:?}", e.into_inner());
        }
    }

    pub fn query(&self, limit: usize, offset: usize, filter: &UsageFilter) -> Result<Vec<UsageRecord>, String> {
        self.db
            .query_usage(limit, offset, filter)
            .map_err(|e| e.0)
    }

    pub fn count(&self) -> Result<usize, String> {
        self.db.count_usage().map_err(|e| e.0)
    }

    pub fn count_by_user(&self, user_id: &str) -> Result<usize, String> {
        self.db.count_usage_by_user(user_id).map_err(|e| e.0)
    }

    pub fn count_filtered(&self, filter: &UsageFilter) -> Result<usize, String> {
        self.db.count_usage_filtered(filter).map_err(|e| e.0)
    }

    pub fn get_detail(&self, request_id: &str) -> Result<Option<crate::domain::usage::UsageRecord>, String> {
        self.db.get_usage_detail(request_id).map_err(|e| e.0)
    }

    pub fn daily_counts(&self, since: &str, user_id: Option<&str>, tz_offset_seconds: i64) -> Result<Vec<(String, i64)>, String> {
        self.db.daily_usage_counts(since, user_id, tz_offset_seconds).map_err(|e| e.0)
    }

    pub fn stats_since(&self, since: &str, user_id: Option<&str>) -> Result<(u64, u64, u64, u64), String> {
        self.db.usage_stats_since(since, user_id).map_err(|e| e.0)
    }

    pub fn cost_rows_since(&self, since: &str, user_id: Option<&str>) -> Result<Vec<UsageRecord>, String> {
        self.db.usage_cost_rows_since(since, user_id).map_err(|e| e.0)
    }

    pub fn daily_stats(&self, since: &str, user_id: Option<&str>, tz_offset_seconds: i64) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64)>, String> {
        self.db.daily_usage_stats(since, user_id, tz_offset_seconds).map_err(|e| e.0)
    }
}

async fn background_writer(db: Arc<Database>, cache: Arc<RedisCache>, mut rx: Receiver<UsageRecord>) {
    while let Some(record) = rx.recv().await {
        let mut batch = vec![record];
        let deadline = tokio::time::sleep(Duration::from_millis(10));
        tokio::pin!(deadline);

        while batch.len() < 100 {
            tokio::select! {
                biased;
                r = rx.recv() => match r {
                    Some(r) => batch.push(r),
                    None => break,
                },
                _ = &mut deadline => break,
            }
        }

        let db = db.clone();

        // ── Write batch to SQLite and collect deduction results ──
        let result: Result<Vec<(String, f64, f64)>, ()> = tokio::task::spawn_blocking(move || {
            let conn = match db.conn() {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to get DB connection: {}", e);
                    return Err(());
                }
            };
            if let Err(e) = conn.execute("BEGIN", []) {
                tracing::error!("Failed to begin transaction: {}", e);
                return Err(());
            }

            // Read billing_enabled from live config (catches runtime toggle)
            let billing_enabled = conn
                .query_row(
                    "SELECT value FROM balancer_settings WHERE key = 'gateway_config'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .ok()
                .and_then(|json| serde_json::from_str::<GatewayRuntimeConfig>(&json).ok())
                .map(|c| c.billing_enabled)
                .unwrap_or(false);

            let mut deductions: Vec<(String, f64, f64)> = Vec::new();

            for r in &batch {
                // Look up pricing inline using existing conn (avoids deadlock)
                let (pp, cp) = if r.prompt_price == 0.0 && r.completion_price == 0.0 {
                    pricing_lookup(&conn, &r.model)
                } else {
                    (r.prompt_price, r.completion_price)
                };

                if let Err(e) = crate::db::insert_usage_row_with_pricing(&conn, r, pp, cp) {
                    tracing::error!("Failed to insert usage record: {}", e);
                    let _ = conn.execute("ROLLBACK", []);
                    return Err(());
                }

                // ── Wallet balance deduction ──
                if billing_enabled {
                    let cost = (r.prompt_tokens as f64 / 1000.0 * pp)
                        + (r.completion_tokens as f64 / 1000.0 * cp);
                    if cost <= 0.0 {
                        if pp == 0.0 && cp == 0.0 {
                            tracing::warn!(
                                user_id = %r.user_id, model = %r.model,
                                "Wallet deduction skipped: zero cost. Has model pricing been configured?"
                            );
                        }
                    } else {
                        // Atomic deduction: balance can never go below 0
                        if let Err(e) = conn.execute(
                            "UPDATE users SET balance = MAX(balance - ?1, 0) WHERE id = ?2",
                            params![cost, r.user_id],
                        ) {
                            tracing::error!("Failed to deduct wallet balance: {}", e);
                            let _ = conn.execute("ROLLBACK", []);
                            return Err(());
                        }
                        let (new_balance, frozen): (f64, f64) = conn
                            .query_row(
                                "SELECT balance, frozen FROM users WHERE id = ?1",
                                params![r.user_id],
                                |row| Ok((row.get(0)?, row.get(1)?)),
                            )
                            .unwrap_or((0.0, 0.0));
                        let _ = conn.execute(
                            "INSERT INTO wallet_transactions (id, user_id, type, amount, balance_before, balance_after, method, status, note, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                            params![
                                Uuid::new_v4().to_string(), r.user_id, "deduction", -cost,
                                new_balance + cost, new_balance, "usage", "completed",
                                format!("API usage: {}", r.model),
                                Utc::now().to_rfc3339(),
                            ],
                        );
                        deductions.push((r.user_id.clone(), new_balance, frozen));
                    }
                }
            }
            if let Err(e) = conn.execute("COMMIT", []) {
                tracing::error!("Failed to commit batch: {}", e);
                return Err(());
            }
            Ok(deductions)
        })
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Spawn blocking join error: {}", e);
            Err(())
        });

        // ── Sync deduction results to Redis ──
        if let Ok(deductions) = result {
            for (user_id, new_balance, frozen) in &deductions {
                let status = compute_gate_status(*new_balance, *frozen);
                if let Err(e) = cache
                    .set_gate_and_balance(user_id, status, *new_balance)
                    .await
                {
                    tracing::warn!(user_id, "Failed to update Redis gate status: {}", e);
                }
            }
        }
    }
}

/// Look up model pricing using an existing connection (avoids deadlock with db.conn()).
fn pricing_lookup(conn: &rusqlite::Connection, model_name: &str) -> (f64, f64) {
    if let Ok(pair) = conn.query_row(
        "SELECT prompt_price, completion_price FROM models WHERE name = ?1",
        params![model_name],
        |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
    ) {
        return pair;
    }
    if let Ok(mut stmt) = conn.prepare(
        "SELECT prompt_price, completion_price, model_pattern FROM models",
    ) {
        if let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, f64>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, String>(2)?,
            ))
        }) {
            for row in rows.flatten() {
                let (p, c, pattern) = row;
                if pattern.ends_with('*') {
                    let prefix = &pattern[..pattern.len() - 1];
                    if model_name.starts_with(prefix) {
                        return (p, c);
                    }
                } else if pattern == model_name {
                    return (p, c);
                }
            }
        }
    }
    (0.0, 0.0)
}
