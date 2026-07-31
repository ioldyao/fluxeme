use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::cache::RedisCache;

use super::event::{RequestCompleted, RouteDecided};

/// Unified message type sent over the bus.
/// `#[serde(untagged)]` serialises each variant as its inner struct directly,
/// so the frontend receives the same JSON shape as the old `RequestEvent`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BusMessage {
    Completed(RequestCompleted),
    Decided(RouteDecided),
}

/// Payload published to the shared Redis channel so other gateway instances
/// can relay remote events to their local WebSocket clients.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteEnvelope {
    /// Origin instance id — subscribers skip their own echoes.
    pub instance_id: String,
    pub event: BusMessage,
}

/// Shared channel name for cross-instance event fan-out.
pub const BUS_CHANNEL: &str = "obs:bus";

/// Lightweight event bus wrapping a `tokio::sync::broadcast` channel.
///
/// The bus is the single source of truth for real-time observability events.
/// Callers `clone()` it cheaply (the inner sender is `Clone`).
///
/// When Redis is enabled, every published event is ALSO pushed to the shared
/// `obs:bus` channel. A background subscriber task on each instance (see
/// `start_remote_subscriber`) reads those and injects remote events into the
/// local broadcast, so WebSocket clients on any instance see all traffic.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<BusMessage>,
    redis: Option<Arc<RedisCache>>,
    instance_id: String,
}

impl EventBus {
    /// Create a new bus with room for `capacity` unread events.
    /// `redis` is `Some` when the shared Redis cache is enabled.
    pub fn new(capacity: usize, redis: Option<Arc<RedisCache>>, instance_id: String) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            redis,
            instance_id,
        }
    }

    /// Publish a route-decision event (frontend shows "in-flight" pulse).
    pub fn route_decided(&self, event: RouteDecided) {
        let msg = BusMessage::Decided(event);
        let _ = self.tx.send(msg.clone());
        self.publish_remote(msg);
    }

    /// Publish a request-completed event (frontend increments counters).
    pub fn request_completed(&self, event: RequestCompleted) {
        let msg = BusMessage::Completed(event);
        let _ = self.tx.send(msg.clone());
        self.publish_remote(msg);
    }

    /// Push an event to the shared Redis channel (if enabled), wrapped with
    /// the origin instance id so remote subscribers can skip their own echoes.
    fn publish_remote(&self, msg: BusMessage) {
        let Some(redis) = &self.redis else { return };
        let payload = serde_json::to_string(&RemoteEnvelope {
            instance_id: self.instance_id.clone(),
            event: msg,
        })
        .unwrap_or_default();
        let redis = redis.clone();
        tokio::spawn(async move {
            if let Err(e) = redis.publish(BUS_CHANNEL, &payload).await {
                tracing::warn!("Failed to publish event to Redis: {e}");
            }
        });
    }

    /// Inject a remote event (received from the Redis subscriber) into the
    /// local broadcast. Does not re-publish to Redis — avoids echo loops.
    pub fn inject_remote(&self, envelope: RemoteEnvelope) {
        if envelope.instance_id == self.instance_id {
            return; // skip our own echo (already delivered locally)
        }
        let _ = self.tx.send(envelope.event);
    }

    /// Obtain a new receiver.  Each call produces an independent subscription
    /// that receives events published **after** the subscription was created.
    pub fn subscribe(&self) -> broadcast::Receiver<BusMessage> {
        self.tx.subscribe()
    }
}
