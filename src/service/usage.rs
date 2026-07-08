use std::sync::Arc;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::db::Database;
use crate::domain::usage::UsageRecord;

#[derive(Clone)]
pub struct UsageService {
    sender: UnboundedSender<UsageRecord>,
    db: Arc<Database>,
}

impl UsageService {
    pub fn new(db: Arc<Database>) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::unbounded_channel::<UsageRecord>();
        let handle = tokio::spawn(background_writer(db.clone(), rx));

        (Self { sender: tx, db }, handle)
    }

    pub fn record(&self, record: UsageRecord) {
        let _ = self.sender.send(record);
    }

    pub fn query(&self, limit: usize, user_id: Option<&str>) -> Result<Vec<UsageRecord>, String> {
        self.db
            .query_usage(limit, user_id)
            .map_err(|e| e.0)
    }

    pub fn count(&self) -> Result<usize, String> {
        self.db.count_usage().map_err(|e| e.0)
    }

    pub fn count_by_user(&self, user_id: &str) -> Result<usize, String> {
        self.db.count_usage_by_user(user_id).map_err(|e| e.0)
    }

    pub fn get_detail(&self, request_id: &str) -> Result<Option<crate::domain::usage::UsageRecord>, String> {
        self.db.get_usage_detail(request_id).map_err(|e| e.0)
    }
}

async fn background_writer(db: Arc<Database>, mut rx: UnboundedReceiver<UsageRecord>) {
    while let Some(record) = rx.recv().await {
        let db = db.clone();
        let _ = tokio::task::spawn_blocking(move || {
            if let Err(e) = db.insert_usage(&record) {
                tracing::error!("Failed to insert usage record: {}", e);
            }
        })
        .await;
    }
}
