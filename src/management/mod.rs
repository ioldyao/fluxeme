mod auth;
mod handlers;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::Router;

use crate::server::AppState;

async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        axum::Json(serde_json::json!({
            "error": {
                "type": "not_found",
                "message": "Management API endpoint not found",
            }
        })),
    )
}

pub fn routes(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", axum::routing::get(handlers::status))
        .route("/models", axum::routing::get(handlers::models))
        .route("/channels", axum::routing::get(handlers::channels))
        .route(
            "/routing/health",
            axum::routing::get(handlers::routing_health),
        )
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(
            state,
            auth::require_management_key,
        ))
}
