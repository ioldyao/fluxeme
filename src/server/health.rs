use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::server::AppState;

/// Liveness probe — returns 200 as long as the process is alive.
/// Load balancers use this to decide whether to keep sending traffic.
pub async fn liveness(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "status": "ok",
            "instance_id": state.instance_id,
            "version": env!("CARGO_PKG_VERSION"),
        })),
    )
}

/// Readiness probe — checks critical dependencies (PostgreSQL, Redis).
/// Load balancers use this to decide whether to route traffic to this instance.
/// Returns 503 when a critical dependency is unreachable so the LB drains the instance.
pub async fn readiness(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Value>) {
    let mut ok = true;
    let mut checks = serde_json::Map::new();

    // PostgreSQL
    match state.db.ping().await {
        Ok(()) => {
            checks.insert("postgres".to_string(), json!({ "status": "ok" }));
        }
        Err(e) => {
            ok = false;
            checks.insert(
                "postgres".to_string(),
                json!({ "status": "error", "error": e.to_string() }),
            );
        }
    }

    // Redis (skip when disabled)
    if state.cache.is_enabled() {
        match state.cache.ping().await {
            Ok(()) => {
                checks.insert("redis".to_string(), json!({ "status": "ok" }));
            }
            Err(e) => {
                ok = false;
                checks.insert(
                    "redis".to_string(),
                    json!({ "status": "error", "error": e }),
                );
            }
        }
    } else {
        checks.insert("redis".to_string(), json!({ "status": "disabled" }));
    }

    let status_code = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(json!({
            "status": if ok { "ok" } else { "unavailable" },
            "instance_id": state.instance_id,
            "version": env!("CARGO_PKG_VERSION"),
            "checks": checks,
        })),
    )
}
