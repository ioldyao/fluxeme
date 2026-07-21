use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use bytes::Bytes;
use tokio::sync::broadcast;

use crate::observability::event_bus::BusMessage;
use crate::server::AppState;

/// WebSocket upgrade handler for real-time request path events.
/// GET /admin/api/health/ws
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, crate::admin::AdminError> {
    let _session = crate::admin::require_session_internal(&state.admin, &headers).await?;
    Ok(ws.on_upgrade(move |socket| handle_socket(socket, state.event_bus.subscribe())))
}

async fn handle_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<BusMessage>) {
    let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(15));

    loop {
        tokio::select! {
            biased;
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(_))) => {
                        let _ = socket.send(Message::Pong(Bytes::new())).await;
                    }
                    _ => {}
                }
            }
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        if let Ok(json) = serde_json::to_string(&event) {
                            if socket.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = ping_interval.tick() => {
                if socket.send(Message::Ping(Bytes::new())).await.is_err() {
                    break;
                }
            }
        }
    }
}
