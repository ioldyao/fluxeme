use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::JoinHandle;

use crate::cache::{compute_gate_status, RedisCache};
use crate::ch_backend::{ClickHouseBackend, UsageEvent};
use crate::db::Database;
use crate::domain::usage::UsageFilter;
use crate::domain::usage::UsageRecord;
use crate::observability::event::RequestCompleted;
use crate::observability::event_bus::EventBus;

const N_BILLING_WORKERS: usize = 4;
const BILLING_CHANNEL_CAP: usize = 16384;

/// Routes a `UsageRecord` to a billing worker by hashing the user_id.
fn billing_shard(record: &UsageRecord) -> usize {
    let mut hasher = DefaultHasher::new();
    record.user_id.hash(&mut hasher);
    (hasher.finish() as usize) % N_BILLING_WORKERS
}

#[derive(Clone)]
pub struct UsageService {
    billing_senders: Arc<Vec<Sender<UsageRecord>>>,
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
        ch: Option<Arc<ClickHouseBackend>>,
    ) -> (Self, Vec<JoinHandle<()>>) {
        let mut senders = Vec::with_capacity(N_BILLING_WORKERS);
        let mut handles = Vec::with_capacity(N_BILLING_WORKERS);

        for i in 0..N_BILLING_WORKERS {
            let (tx, rx) = mpsc::channel::<UsageRecord>(BILLING_CHANNEL_CAP);
            senders.push(tx);
            let h = tokio::spawn(billing_worker(i, db.clone(), cache.clone(), ch.clone(), rx));
            handles.push(h);
        }

        (
            Self {
                billing_senders: Arc::new(senders),
                db,
                cache,
                event_bus,
            },
            handles,
        )
    }

    /// Record usage (no endpoint_id). Shorthand for `record_with_endpoint(record, None)`.
    pub fn record(&self, record: UsageRecord) {
        self.record_with_endpoint(record, None);
    }

    /// Record usage with an optional endpoint_id.
    ///
    /// 1. Broadcasts a `RequestCompleted` event on the event bus (real-time WS push).
    /// 2. Routes the `UsageRecord` to the appropriate billing worker via shard.
    ///    If the channel is full, the record is dropped with a CRITICAL log.
    ///    Phase 5 will replace this drop with a local WAL fallback.
    pub fn record_with_endpoint(&self, record: UsageRecord, endpoint_id: Option<i64>) {
        // 1. Broadcast real-time event (always succeeds, non-blocking)
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
        self.event_bus.request_completed(event);

        // 2. Route billing to sharded worker
        let idx = billing_shard(&record);
        if let Err(e) = self.billing_senders[idx].try_send(record) {
            let record = e.into_inner();
            tracing::warn!(
                worker = idx, request_id = record.request_id,
                "Billing channel full — falling back to Redis Stream backlog"
            );
            // Fallback to Redis Stream — fired-and-forget via spawn so the
            // synchronous poll_next caller returns immediately.
            let cache = self.cache.clone();
            tokio::spawn(async move {
                if let Err(e) = cache.backlog_billing_record(record).await {
                    tracing::error!("Redis backlog XADD failed: {e}");
                }
            });
        }
    }

    // ── Read-through query methods (unchanged, still hit PG) ──────────

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
        self.db
            .count_usage_by_user(user_id)
            .await
            .map_err(|e| e.0)
    }

    pub async fn count_filtered(&self, filter: &UsageFilter) -> Result<usize, String> {
        self.db
            .count_usage_filtered(filter)
            .await
            .map_err(|e| e.0)
    }

    pub async fn get_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<UsageRecord>, String> {
        self.db
            .get_usage_detail(request_id)
            .await
            .map_err(|e| e.0)
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

    pub async fn funnel_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<crate::db::FunnelStats, String> {
        self.db
            .funnel_stats(since, user_id)
            .await
            .map_err(|e| e.0)
    }
}

/// Billing worker: drains its shard channel, batches records, and runs the PG
/// transaction (pricing lookup → balance deduction → wallet tx → usage_billing).
async fn billing_worker(
    id: usize,
    db: Arc<Database>,
    cache: Arc<RedisCache>,
    ch: Option<Arc<ClickHouseBackend>>,
    mut rx: Receiver<UsageRecord>,
) {
    tracing::info!("Billing worker {id} started");

    while let Some(record) = rx.recv().await {
        let mut batch = vec![record];
        let deadline = tokio::time::sleep(Duration::from_millis(10));
        tokio::pin!(deadline);

        // Batch up to 100 records or 10ms
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
        let billing_enabled = db
            .get_gateway_config()
            .await
            .map(|c| c.billing_enabled)
            .unwrap_or_else(|e| {
                tracing::error!(
                    worker = id,
                    error = %e.0,
                    "Failed to read gateway config — billing disabled"
                );
                false
            });

        // Write batch to PG and collect deduction results (atomic transaction)
        match db
            .batch_insert_usage_with_billing(&batch, billing_enabled)
            .await
        {
            Ok(deductions) => {
                // Sync deduction results to Redis
                for (user_id, new_balance, frozen) in &deductions {
                    let status = compute_gate_status(*new_balance, *frozen);
                    if let Err(e) = cache
                        .set_gate_and_balance(user_id, status, *new_balance)
                        .await
                    {
                        tracing::warn!(worker = id, user_id, "Redis gate update: {e}");
                    }
                }

                // Write observability data to ClickHouse (best-effort)
                if let Some(ref ch) = ch {
                    let events: Vec<UsageEvent> = batch.iter().map(usage_record_to_event).collect();
                    let request_ids: Vec<String> =
                        batch.iter().map(|r| r.request_id.clone()).collect();
                    match ch.insert_usage_events(&events).await {
                        Ok(()) => {
                            if let Err(e) = db.mark_usage_billing_written(&request_ids).await {
                                tracing::warn!(
                                    worker = id, count = events.len(), error = %e.0,
                                    "CH write ok but mark_written failed — compensation will retry"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                worker = id, count = events.len(), error = e,
                                "CH write failed — compensation task will catch up"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    worker = id, batch_size = batch.len(), error = %e.0,
                    "Usage billing transaction failed"
                );
            }
        }
    }

    tracing::warn!("Billing worker {id} exiting (channel closed)");
}

fn usage_record_to_event(r: &UsageRecord) -> UsageEvent {
    let total_tokens = if r.total_tokens > 0 {
        r.total_tokens
    } else {
        r.prompt_tokens + r.completion_tokens
    };
    // cost_amount is already computed and stored in usage_billing; for the CH
    // UsageEvent we just set it to 0 — the compensation task reads real cost
    // from usage_billing. Direct write from worker doesn't persist cost_amount
    // because pricing lookup happened inside the PG transaction, not in Rust.
    UsageEvent {
        timestamp: r.timestamp.clone(),
        request_id: r.request_id.clone(),
        user_id: r.user_id.clone(),
        user_name: r.user_name.clone(),
        channel_id: r.channel_id.clone(),
        model: r.model.clone(),
        prompt_tokens: r.prompt_tokens,
        completion_tokens: r.completion_tokens,
        total_tokens,
        latency_ms: r.latency_ms,
        status_code: r.status_code,
        success: if r.success { 1 } else { 0 },
        api_key_name: r.api_key_name.clone(),
        api_format: r.api_format.clone(),
        stream: if r.stream { 1 } else { 0 },
        cache_hit_input_tokens: r.cache_hit_input_tokens,
        cost_amount: 0.0, // real cost set by compensation task
        client_ip: r.client_ip.clone(),
        endpoint_id: None, // not available on UsageRecord
    }
}
