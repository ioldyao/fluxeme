use std::sync::Arc;

use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::JoinHandle;

use crate::db::Database;
use crate::domain::usage::UsageRecord;

#[derive(Clone)]
pub struct UsageService {
    sender: Sender<UsageRecord>,
    db: Arc<Database>,
}

impl UsageService {
    pub fn new(db: Arc<Database>) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel::<UsageRecord>(4096);
        let handle = tokio::spawn(background_writer(db.clone(), rx));

        (Self { sender: tx, db }, handle)
    }

    pub fn record(&self, record: UsageRecord) {
        if let Err(e) = self.sender.try_send(record) {
            tracing::warn!("Usage channel full, dropping record: {:?}", e.into_inner());
        }
    }

    pub fn query(&self, limit: usize, offset: usize, user_id: Option<&str>) -> Result<Vec<UsageRecord>, String> {
        self.db
            .query_usage(limit, offset, user_id)
            .map_err(|e| e.0)
    }

    pub fn count(&self) -> Result<usize, String> {
        self.db.count_usage().map_err(|e| e.0)
    }

    pub fn count_by_user(&self, user_id: &str) -> Result<usize, String> {
        self.db.count_usage_by_user(user_id).map_err(|e| e.0)
    }

    pub fn count_filtered(&self, user_id: Option<&str>) -> Result<usize, String> {
        self.db.count_usage_filtered(user_id).map_err(|e| e.0)
    }

    pub fn get_detail(&self, request_id: &str) -> Result<Option<crate::domain::usage::UsageRecord>, String> {
        self.db.get_usage_detail(request_id).map_err(|e| e.0)
    }

    pub fn daily_counts(&self, since: &str, user_id: Option<&str>) -> Result<Vec<(String, i64)>, String> {
        self.db.daily_usage_counts(since, user_id).map_err(|e| e.0)
    }

    pub fn stats_since(&self, since: &str, user_id: Option<&str>) -> Result<(u64, u64, u64, u64), String> {
        self.db.usage_stats_since(since, user_id).map_err(|e| e.0)
    }

    pub fn cost_rows_since(&self, since: &str, user_id: Option<&str>) -> Result<Vec<UsageRecord>, String> {
        self.db.usage_cost_rows_since(since, user_id).map_err(|e| e.0)
    }

    pub fn daily_stats(&self, since: &str, user_id: Option<&str>) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64)>, String> {
        self.db.daily_usage_stats(since, user_id).map_err(|e| e.0)
    }
}

async fn background_writer(db: Arc<Database>, mut rx: Receiver<UsageRecord>) {
    while let Some(record) = rx.recv().await {
        let mut batch = vec![record];
        while batch.len() < 100 {
            match rx.try_recv() {
                Ok(r) => batch.push(r),
                Err(_) => break,
            }
        }
        let db = db.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let conn = match db.conn() {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to get DB connection: {}", e);
                    return;
                }
            };
            if batch.len() == 1 {
                if let Err(e) = crate::db::insert_usage_row(&conn, &batch[0]) {
                    tracing::error!("Failed to insert usage record: {}", e);
                }
            } else {
                if let Err(e) = conn.execute("BEGIN", []) {
                    tracing::error!("Failed to begin transaction: {}", e);
                    return;
                }
                for r in &batch {
                    if let Err(e) = crate::db::insert_usage_row(&conn, r) {
                        tracing::error!("Failed to insert usage record: {}", e);
                        let _ = conn.execute("ROLLBACK", []);
                        return;
                    }
                }
                if let Err(e) = conn.execute("COMMIT", []) {
                    tracing::error!("Failed to commit batch: {}", e);
                }
            }
        })
        .await;
    }
}
