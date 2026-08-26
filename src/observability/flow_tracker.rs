use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::Utc;

use crate::cache::RedisCache;

const FLOW_KEY_PREFIX: &str = "flow:active:";
const FLOW_INDEX_KEY: &str = "flow:active:index";
const FLOW_COMPLETED_PREFIX: &str = "flow:completed:";
const FLOW_TTL_SECS: u64 = 3600;
const LOCAL_STALE_AFTER: Duration = Duration::from_secs(FLOW_TTL_SECS);

#[derive(Clone, Debug)]
pub struct ActiveRequest {
    pub request_id: String,
    pub model: String,
    pub channel_id: String,
    pub endpoint_id: Option<i64>,
    pub accepted_at: String,
    pub upstream_started_at: Option<String>,
    pub first_byte_at: Option<String>,
    last_seen: Instant,
    sequence: u64,
}

#[derive(Clone, Debug, Default)]
pub struct FlowSnapshot {
    pub as_of: String,
    pub in_flight: u64,
    pub upstream_generating: u64,
    pub upstream_outputting: u64,
}

#[derive(Clone)]
pub struct FlowTracker {
    inner: Arc<Mutex<HashMap<String, ActiveRequest>>>,
    redis: Arc<RedisCache>,
    instance_id: Arc<str>,
}

impl FlowTracker {
    pub fn new(redis: Arc<RedisCache>, instance_id: impl Into<Arc<str>>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            redis,
            instance_id: instance_id.into(),
        }
    }

    fn redis_key(&self, request_id: &str) -> String {
        format!("{FLOW_KEY_PREFIX}{}:{request_id}", self.instance_id)
    }

    fn completed_key(&self, request_id: &str) -> String {
        format!("{FLOW_COMPLETED_PREFIX}{}:{request_id}", self.instance_id)
    }

    fn persist(&self, request_id: &str, state: &'static str, sequence: u64) {
        let redis = self.redis.clone();
        let key = self.redis_key(request_id);
        let completed_key = self.completed_key(request_id);
        tokio::spawn(async move {
            if let Err(error) = redis
                .flow_set(
                    &key,
                    &completed_key,
                    FLOW_INDEX_KEY,
                    state,
                    sequence,
                    FLOW_TTL_SECS,
                )
                .await
            {
                tracing::warn!(%error, "failed to update distributed flow tracker");
            }
        });
    }

    fn remove_distributed(&self, request_id: &str, sequence: u64) {
        let redis = self.redis.clone();
        let key = self.redis_key(request_id);
        let completed_key = self.completed_key(request_id);
        tokio::spawn(async move {
            if let Err(error) = redis
                .flow_remove(&key, &completed_key, FLOW_INDEX_KEY, sequence)
                .await
            {
                tracing::warn!(%error, "failed to remove distributed flow tracker entry");
            }
        });
    }

    pub fn mark_accepted(
        &self,
        request_id: String,
        model: String,
        channel_id: String,
        endpoint_id: Option<i64>,
        accepted_at: String,
    ) {
        let now = Instant::now();
        let sequence = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .min(u64::MAX as u128) as u64;
        self.inner.lock().unwrap().insert(
            request_id.clone(),
            ActiveRequest {
                request_id: request_id.clone(),
                model,
                channel_id,
                endpoint_id,
                accepted_at,
                upstream_started_at: None,
                first_byte_at: None,
                last_seen: now,
                sequence,
            },
        );
        self.persist(&request_id, "accepted", sequence);
    }

    pub fn mark_upstream_started(&self, request_id: &str, at: String) {
        let changed = self.inner.lock().unwrap().get_mut(request_id).map(|req| {
            req.upstream_started_at = Some(at);
            req.last_seen = Instant::now();
            req.sequence = req.sequence.saturating_add(1);
            req.sequence
        });
        if let Some(sequence) = changed {
            self.persist(request_id, "generating", sequence);
        }
    }

    pub fn mark_first_byte(&self, request_id: &str, at: String) {
        let changed = self.inner.lock().unwrap().get_mut(request_id).map(|req| {
            req.first_byte_at = Some(at);
            req.last_seen = Instant::now();
            req.sequence = req.sequence.saturating_add(1);
            req.sequence
        });
        if let Some(sequence) = changed {
            self.persist(request_id, "outputting", sequence);
        }
    }

    pub fn mark_completed(&self, request_id: &str) {
        let sequence = self
            .inner
            .lock()
            .unwrap()
            .remove(request_id)
            .map(|req| req.sequence.saturating_add(1))
            .unwrap_or(u64::MAX);
        self.remove_distributed(request_id, sequence);
    }

    fn snapshot_local(&self) -> FlowSnapshot {
        let mut guard = self.inner.lock().unwrap();
        let now = Instant::now();
        guard.retain(|_, req| now.duration_since(req.last_seen) < LOCAL_STALE_AFTER);

        let mut snapshot = FlowSnapshot::default();
        for req in guard.values() {
            snapshot.in_flight += 1;
            if req.upstream_started_at.is_some() {
                if req.first_byte_at.is_some() {
                    snapshot.upstream_outputting += 1;
                } else {
                    snapshot.upstream_generating += 1;
                }
            }
        }
        snapshot.as_of = Utc::now().to_rfc3339();
        snapshot
    }

    /// Return the cluster-wide live totals from Redis. There is deliberately
    /// no PostgreSQL/ClickHouse fallback: a missing Redis snapshot is unknown,
    /// not zero and not a reason to trust a local partial view.
    pub async fn snapshot_global(&self) -> Result<FlowSnapshot, String> {
        // Keep the local lifecycle cache bounded even though Redis is the
        // authoritative source for the cluster-wide totals.
        let _ = self.snapshot_local();
        let (in_flight, upstream_generating, upstream_outputting) =
            self.redis.flow_snapshot(FLOW_INDEX_KEY).await?;
        Ok(FlowSnapshot {
            as_of: Utc::now().to_rfc3339(),
            in_flight,
            upstream_generating,
            upstream_outputting,
        })
    }

    /// Local snapshot retained for diagnostics and unit tests. Production
    /// admin metrics use `snapshot_global` so multiple instances are included.
    pub fn snapshot(&self) -> FlowSnapshot {
        self.snapshot_local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_registry_constants_are_stable() {
        assert_eq!(FLOW_TTL_SECS, 3600);
        assert_eq!(FLOW_INDEX_KEY, "flow:active:index");
    }
}
