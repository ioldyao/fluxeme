use serde::Serialize;
use tokio::sync::broadcast;

use super::event::{RequestCompleted, RouteDecided};

/// Unified message type sent over the bus.
/// `#[serde(untagged)]` serialises each variant as its inner struct directly,
/// so the frontend receives the same JSON shape as the old `RequestEvent`.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum BusMessage {
    Completed(RequestCompleted),
    Decided(RouteDecided),
}

/// Lightweight event bus wrapping a `tokio::sync::broadcast` channel.
///
/// The bus is the single source of truth for real-time observability events.
/// Callers `clone()` it cheaply (the inner sender is `Clone`).
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<BusMessage>,
}

impl EventBus {
    /// Create a new bus with room for `capacity` unread events.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publish a route-decision event (frontend shows "in-flight" pulse).
    pub fn route_decided(&self, event: RouteDecided) {
        let _ = self.tx.send(BusMessage::Decided(event));
    }

    /// Publish a request-completed event (frontend increments counters).
    pub fn request_completed(&self, event: RequestCompleted) {
        let _ = self.tx.send(BusMessage::Completed(event));
    }

    /// Obtain a new receiver.  Each call produces an independent subscription
    /// that receives events published **after** the subscription was created.
    pub fn subscribe(&self) -> broadcast::Receiver<BusMessage> {
        self.tx.subscribe()
    }
}
