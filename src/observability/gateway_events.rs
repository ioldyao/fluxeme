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

/// A gateway request entering the routing layer.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GatewayRequestEvent {
    pub timestamp: String,
    pub request_id: String,
    pub user_id: Option<String>,
    pub api_key_id: Option<String>,
    pub route_id: String,
    pub method: String,
    pub path: String,
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
            api_key_id: Some("key-1".to_string()),
            route_id: "route-1".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
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
