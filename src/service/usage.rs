use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::JoinHandle;

use crate::cache::{compute_gate_status, RedisCache};
use crate::db::Database;
use crate::domain::usage::UsageFilter;
use crate::domain::usage::UsageRecord;
use crate::observability::event::RequestCompleted;
use crate::observability::event_bus::EventBus;

#[derive(Clone)]
pub struct UsageService {
    sender: Sender<UsageRecord>,
    db: Arc<Database>,
    #[allow(dead_code)]
    cache: Arc<RedisCache>,
    event_bus: EventBus,
}

impl UsageService {
    pub fn new(
        db: Arc<Database>,
        cache: Arc<RedisCache>,
        event_bus: EventBus,
    ) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel::<UsageRecord>(4096);
        let handle = tokio::spawn(background_writer(db.clone(), cache.clone(), rx));

        (
            Self {
                sender: tx,
                db,
                cache,
                event_bus,
            },
            handle,
        )
    }

    pub fn record(&self, record: UsageRecord) {
        self.record_with_endpoint(record, None);
    }

    /// Record usage and broadcast a real-time event with endpoint_id.
    pub fn record_with_endpoint(&self, record: UsageRecord, endpoint_id: Option<i64>) {
        let event = RequestCompleted {
            timestamp: record.timestamp.clone(),
            request_id: record.request_id.clone(),
            model: record.model.clone(),
            channel_id: record.channel_id.clone(),
            endpoint_id,
            latency_ms: record.latency_ms,
            success: record.success,
            prompt_tokens: Some(record.prompt_tokens),
            completion_tokens: Some(record.completion_tokens),
        };
        if let Err(e) = self.sender.try_send(record) {
            tracing::warn!("Usage channel full, dropping record: {:?}", e.into_inner());
        }
        self.event_bus.request_completed(event);
    }

    pub async fn query(
        &self,
        limit: usize,
        offset: usize,
        filter: &UsageFilter,
    ) -> Result<Vec<UsageRecord>, String> {
        self.db
            .query_usage(limit, offset, filter)
            .await
            .map_err(|e| e.0)
    }

    pub async fn count(&self) -> Result<usize, String> {
        self.db.count_usage().await.map_err(|e| e.0)
    }

    pub async fn count_by_user(&self, user_id: &str) -> Result<usize, String> {
        self.db.count_usage_by_user(user_id).await.map_err(|e| e.0)
    }

    pub async fn count_filtered(&self, filter: &UsageFilter) -> Result<usize, String> {
        self.db.count_usage_filtered(filter).await.map_err(|e| e.0)
    }

    pub async fn get_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<crate::domain::usage::UsageRecord>, String> {
        self.db.get_usage_detail(request_id).await.map_err(|e| e.0)
    }

    pub async fn daily_counts(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, i64)>, String> {
        self.db
            .daily_usage_counts(since, user_id, tz_offset_seconds)
            .await
            .map_err(|e| e.0)
    }

    pub async fn stats_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<(u64, u64, u64, u64), String> {
        self.db
            .usage_stats_since(since, user_id)
            .await
            .map_err(|e| e.0)
    }

    pub async fn cost_rows_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, String> {
        self.db
            .usage_cost_rows_since(since, user_id)
            .await
            .map_err(|e| e.0)
    }

    pub async fn daily_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64, u64)>, String> {
        self.db
            .daily_usage_stats(since, user_id, tz_offset_seconds)
            .await
            .map_err(|e| e.0)
    }
}

async fn background_writer(
    db: Arc<Database>,
    cache: Arc<RedisCache>,
    mut rx: Receiver<UsageRecord>,
) {
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

        // Read billing_enabled from gateway config.
        // Log an error if the read fails — silently treating it as false
        // would silently disable billing for the entire batch.
        let billing_enabled = db
            .get_gateway_config()
            .await
            .map(|c| c.billing_enabled)
            .unwrap_or_else(|e| {
                tracing::error!(error = %e.0, "Failed to read gateway config for billing — falling back to disabled");
                false
            });

        // Write batch to DB and collect deduction results (atomic transaction)
        let result = db
            .batch_insert_usage_with_billing(&batch, billing_enabled)
            .await;

        // Sync deduction results to Redis
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
