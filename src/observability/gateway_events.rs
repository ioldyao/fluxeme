//! Typed API-gateway observability events and their non-blocking recorder.
//!
//! These events are deliberately separate from `usage_events` and billing.  The
//! recorder only performs a bounded `try_send`; Redis and ClickHouse I/O are
//! done by background tasks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc::{self, error::TrySendError, Receiver, Sender};
use tokio::task::JoinHandle;

use crate::cache::RedisCache;

fn default_status() -> String {
    "succeeded".to_string()
}

fn default_status_code() -> u16 {
    200
}

/// The terminal lifecycle event for one authenticated LLM request.
///
/// Fields are intentionally optional where a request can terminate before route
/// resolution. This keeps the event schema additive and lets old producers/readers
/// continue to decode events while the gateway migrates to lifecycle telemetry.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GatewayRequestEvent {
    pub timestamp: String,
    pub request_id: String,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub team_id: Option<String>,
    pub api_key_id: Option<String>,
    pub api_key_name: Option<String>,
    pub route_id: String,
    pub method: String,
    pub path: String,
    pub api_format: String,
    pub stream: bool,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
    pub requested_model: String,
    pub resolved_model: Option<String>,
    pub channel_id: Option<String>,
    pub endpoint_id: Option<i64>,
    pub endpoint_url: Option<String>,
    pub upstream_model: Option<String>,
    pub provider: Option<String>,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default = "default_status_code")]
    pub status_code: u16,
    pub error_stage: Option<String>,
    pub error_kind: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attempt_count: u32,
    pub successful_attempt: Option<u32>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub total_latency_ms: u64,
    pub ttft_ms: Option<u64>,
    pub client_disconnected: bool,
    pub termination_reason: Option<String>,
    pub billing_payment_mode: Option<String>,
    pub wallet_amount: Option<f64>,
    pub bytes_in: u64,
}

/// One concrete upstream attempt. There may be several attempts per request.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GatewayAttemptEvent {
    pub timestamp: String,
    pub request_id: String,
    pub attempt_id: String,
    pub attempt_no: u32,
    pub route_id: String,
    pub endpoint_url: Option<String>,
    pub status_code: Option<u16>,
    pub success: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// The client-visible result of a gateway request.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GatewayAccessEvent {
    pub timestamp: String,
    pub request_id: String,
    pub user_id: Option<String>,
    pub api_key_id: Option<String>,
    pub route_id: String,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub success: bool,
    pub latency_ms: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "event", rename_all = "snake_case")]
pub enum GatewayEvent {
    Request(GatewayRequestEvent),
    Attempt(GatewayAttemptEvent),
    Access(GatewayAccessEvent),
}

/// A bounded, non-blocking event recorder. Dropping an event when the local
/// queue is full is intentional: request handling must never wait on telemetry.
#[derive(Clone)]
pub struct GatewayEventRecorder {
    tx: Sender<GatewayEvent>,
    dropped: Arc<AtomicU64>,
}

impl GatewayEventRecorder {
    /// Create a recorder and spawn its Redis stream writer.
    pub fn new(cache: Arc<RedisCache>, capacity: usize) -> (Self, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(capacity.max(1));
        let dropped = Arc::new(AtomicU64::new(0));
        let handle = tokio::spawn(gateway_event_writer(cache, rx, dropped.clone()));
        (Self { tx, dropped }, handle)
    }

    pub fn record_request(&self, event: GatewayRequestEvent) {
        self.send(GatewayEvent::Request(event));
    }

    pub fn record_attempt(&self, event: GatewayAttemptEvent) {
        self.send(GatewayEvent::Attempt(event));
    }

    pub fn record_access(&self, event: GatewayAccessEvent) {
        self.send(GatewayEvent::Access(event));
    }

    pub fn try_record(&self, event: GatewayEvent) {
        self.send(event);
    }

    /// Number of events dropped because the bounded queue was full.
    pub fn dropped_count(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Test-only constructor: a recorder wired to a caller-owned channel so
    /// tests can inspect the exact events that were handed to the writer.
    #[cfg(test)]
    pub fn test_recorder(capacity: usize) -> (Self, Receiver<GatewayEvent>) {
        let (tx, rx) = mpsc::channel::<GatewayEvent>(capacity.max(1));
        let recorder = Self {
            tx,
            dropped: Arc::new(AtomicU64::new(0)),
        };
        (recorder, rx)
    }

    fn send(&self, event: GatewayEvent) {
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                tracing::warn!("gateway event queue full; dropping telemetry event");
            }
            Err(TrySendError::Closed(_)) => {
                tracing::debug!("gateway event writer is closed");
            }
        }
    }
}

async fn gateway_event_writer(
    cache: Arc<RedisCache>,
    mut rx: Receiver<GatewayEvent>,
    dropped: Arc<AtomicU64>,
) {
    while let Some(event) = rx.recv().await {
        if let Err(error) = cache.push_gateway_event(event).await {
            tracing::warn!(%error, "gateway event stream write failed");
        }
    }
    tracing::warn!(
        dropped = dropped.load(Ordering::Relaxed),
        "gateway event writer exited"
    );
}

/// Consume gateway events from Redis and write them to ClickHouse.
pub async fn start_gateway_event_consumer(
    ch: Arc<crate::ch_backend::ClickHouseBackend>,
    cache: Arc<RedisCache>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
    loop {
        interval.tick().await;
        let records = match cache.read_gateway_events(500).await {
            Ok(records) => records,
            Err(error) => {
                tracing::warn!(%error, "gateway event stream read failed");
                continue;
            }
        };
        if records.is_empty() {
            continue;
        }
        let mut ids = Vec::with_capacity(records.len());
        let mut requests = Vec::new();
        let mut attempts = Vec::new();
        let mut accesses = Vec::new();
        for (id, event) in records {
            ids.push(id);
            match event {
                GatewayEvent::Request(event) => requests.push(event.into()),
                GatewayEvent::Attempt(event) => attempts.push(event.into()),
                GatewayEvent::Access(event) => accesses.push(event.into()),
            }
        }
        let result = ch
            .insert_gateway_events(&requests, &attempts, &accesses)
            .await;
        if let Err(error) = result {
            tracing::warn!(%error, "gateway event ClickHouse write failed; retaining Redis entries");
            continue;
        }
        if let Err(error) = cache.ack_gateway_events(&ids).await {
            tracing::warn!(%error, "gateway event stream acknowledgement failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_event() -> GatewayRequestEvent {
        GatewayRequestEvent {
            timestamp: "2026-09-05T12:00:00Z".to_string(),
            request_id: "req-1".to_string(),
            user_id: Some("user-1".to_string()),
            user_name: Some("Alice".to_string()),
            team_id: None,
            api_key_id: Some("key-1".to_string()),
            api_key_name: Some("my-key".to_string()),
            route_id: "route-1".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            api_format: "openai".to_string(),
            stream: false,
            client_ip: Some("1.2.3.4".to_string()),
            user_agent: None,
            requested_model: "gpt-4o".to_string(),
            resolved_model: Some("gpt-4o".to_string()),
            channel_id: Some("ch-1".to_string()),
            endpoint_id: Some(5),
            endpoint_url: Some("https://up.example/v1".to_string()),
            upstream_model: None,
            provider: Some("openai".to_string()),
            status: "succeeded".to_string(),
            status_code: 200,
            error_stage: None,
            error_kind: None,
            error_code: None,
            error_message: None,
            attempt_count: 1,
            successful_attempt: Some(1),
            prompt_tokens: 12,
            completion_tokens: 34,
            cache_read_tokens: 2,
            cache_write_tokens: 1,
            total_tokens: 49,
            total_latency_ms: 250,
            ttft_ms: Some(80),
            client_disconnected: false,
            termination_reason: Some("completed".to_string()),
            billing_payment_mode: Some("metered".to_string()),
            wallet_amount: Some(12.5),
            bytes_in: 1024,
        }
    }

    #[test]
    fn request_event_serializes_and_round_trips() {
        let event = GatewayEvent::Request(request_event());
        let json = serde_json::to_string(&event).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["kind"], "request");
        let decoded: GatewayEvent = serde_json::from_str(&json).unwrap();
        match decoded {
            GatewayEvent::Request(inner) => assert_eq!(inner.request_id, "req-1"),
            _ => panic!("expected a request event"),
        }
    }

    #[test]
    fn old_phase1_minimal_request_event_still_decodes() {
        // A Phase 1 event (before lifecycle fields) must decode with defaults
        // so pre-existing entries on the Redis stream are never lost when the
        // gateway rolls forward to the extended schema.
        let old_json = r#"{
            "kind": "request",
            "event": {
                "timestamp": "2026-09-05T12:00:00Z",
                "request_id": "req-old",
                "user_id": "user-1",
                "api_key_id": "key-1",
                "route_id": "route-1",
                "method": "POST",
                "path": "/v1/chat/completions",
                "bytes_in": 512
            }
        }"#;
        let decoded: GatewayEvent = serde_json::from_str(old_json).unwrap();
        match decoded {
            GatewayEvent::Request(inner) => {
                assert_eq!(inner.request_id, "req-old");
                assert_eq!(inner.route_id, "route-1");
                // Lifecycle fields fall back to safe defaults.
                assert_eq!(inner.status, "succeeded");
                assert_eq!(inner.status_code, 200);
                assert_eq!(inner.requested_model, "");
                assert_eq!(inner.attempt_count, 0);
                assert_eq!(inner.total_latency_ms, 0);
                assert!(!inner.stream);
                assert!(inner.resolved_model.is_none());
            }
            _ => panic!("expected a request event"),
        }
    }

    #[test]
    fn request_event_ch_row_maps_lifecycle_fields() {
        let row: crate::ch_backend::GatewayRequestEventRow = request_event().into();
        assert_eq!(row.status, "succeeded");
        assert_eq!(row.status_code, 200);
        assert_eq!(row.requested_model, "gpt-4o");
        assert_eq!(row.total_tokens, 49);
        assert_eq!(row.stream, 0);
        assert_eq!(row.api_format, "openai");
    }

    #[test]
    fn attempt_event_serializes_and_round_trips() {
        let event = GatewayEvent::Attempt(GatewayAttemptEvent {
            timestamp: "2026-09-05T12:00:01Z".to_string(),
            request_id: "req-1".to_string(),
            attempt_id: "att-1".to_string(),
            attempt_no: 1,
            route_id: "route-1".to_string(),
            endpoint_url: Some("https://upstream.example/v1".to_string()),
            status_code: Some(200),
            success: true,
            latency_ms: 120,
            error: None,
        });
        let json = serde_json::to_string(&event).unwrap();
        let decoded: GatewayEvent = serde_json::from_str(&json).unwrap();
        match decoded {
            GatewayEvent::Attempt(inner) => {
                assert_eq!(inner.attempt_no, 1);
                assert!(inner.success);
            }
            _ => panic!("expected an attempt event"),
        }
    }

    #[test]
    fn access_event_serializes_and_round_trips() {
        let event = GatewayEvent::Access(GatewayAccessEvent {
            timestamp: "2026-09-05T12:00:02Z".to_string(),
            request_id: "req-1".to_string(),
            user_id: Some("user-1".to_string()),
            api_key_id: Some("key-1".to_string()),
            route_id: "route-1".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            status_code: 200,
            success: true,
            latency_ms: 250,
            bytes_in: 1024,
            bytes_out: 4096,
        });
        let json = serde_json::to_string(&event).unwrap();
        let decoded: GatewayEvent = serde_json::from_str(&json).unwrap();
        match decoded {
            GatewayEvent::Access(inner) => assert_eq!(inner.bytes_out, 4096),
            _ => panic!("expected an access event"),
        }
    }

    #[test]
    fn full_queue_drops_without_blocking_and_counts() {
        let (tx, _rx) = mpsc::channel::<GatewayEvent>(1);
        let recorder = GatewayEventRecorder {
            tx,
            dropped: Arc::new(AtomicU64::new(0)),
        };
        // First send fills the only slot.
        recorder.try_record(GatewayEvent::Request(request_event()));
        assert_eq!(recorder.dropped_count(), 0);
        // Second send must drop (never block) and be counted.
        recorder.try_record(GatewayEvent::Request(request_event()));
        assert_eq!(recorder.dropped_count(), 1);
    }

    #[test]
    fn recorder_type_helpers_route_to_correct_kind() {
        let (tx, mut rx) = mpsc::channel::<GatewayEvent>(4);
        let recorder = GatewayEventRecorder {
            tx,
            dropped: Arc::new(AtomicU64::new(0)),
        };
        recorder.record_request(request_event());
        recorder.record_attempt(GatewayAttemptEvent {
            timestamp: "t".into(),
            request_id: "r".into(),
            attempt_id: "a".into(),
            attempt_no: 1,
            route_id: "rt".into(),
            endpoint_url: None,
            status_code: None,
            success: false,
            latency_ms: 0,
            error: None,
        });
        recorder.record_access(GatewayAccessEvent {
            timestamp: "t".into(),
            request_id: "r".into(),
            user_id: None,
            api_key_id: None,
            route_id: "rt".into(),
            method: "GET".into(),
            path: "/p".into(),
            status_code: 404,
            success: false,
            latency_ms: 5,
            bytes_in: 0,
            bytes_out: 0,
        });
        assert!(matches!(rx.try_recv(), Ok(GatewayEvent::Request(_))));
        assert!(matches!(rx.try_recv(), Ok(GatewayEvent::Attempt(_))));
        assert!(matches!(rx.try_recv(), Ok(GatewayEvent::Access(_))));
        assert!(rx.try_recv().is_err());
    }
}
