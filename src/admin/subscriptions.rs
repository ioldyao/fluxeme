use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::model::Model;
use crate::server::AppState;

use super::*;

// ── User Subscriptions ────────────────────────────────────────────

pub(crate) async fn list_my_subscriptions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    let models = state
        .db
        .list_subscriptions(&session.user_id)
        .await.map_err(db_err)?;
    Ok(Json(merge_same_named_models(models)))
}

pub(crate) async fn subscribe_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    // Subscribe to ALL same-named model entries so every channel
    // binding is available for routing.
    let models = state.db.list_models().await.map_err(db_err)?;
    let target_name = models.iter().find(|m| m.id == model_id).map(|m| m.name.clone());
    let mut subbed = Vec::new();
    for m in &models {
        if m.id == model_id || (target_name.as_ref() == Some(&m.name)) {
            state.db.subscribe_user(&session.user_id, &m.id).await.map_err(db_err)?;
            subbed.push(m.id.clone());
        }
    }
    Ok(Json(serde_json::json!({ "subscribed": subbed })))
}

pub(crate) async fn unsubscribe_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    // Unsubscribe from ALL same-named model entries.
    let models = state.db.list_models().await.map_err(db_err)?;
    let target_name = models.iter().find(|m| m.id == model_id).map(|m| m.name.clone());
    let mut unsubbed = Vec::new();
    for m in &models {
        if m.id == model_id || (target_name.as_ref() == Some(&m.name)) {
            let _ = state.db.unsubscribe_user(&session.user_id, &m.id).await;
            unsubbed.push(m.id.clone());
        }
    }
    Ok(Json(serde_json::json!({ "unsubscribed": unsubbed })))
}

#[derive(Deserialize)]
pub(crate) struct TestConnectionBody {
    model_id: String,
}

pub(crate) async fn test_subscription_connection(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<TestConnectionBody>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;

    // Check the user is subscribed to this model
    let subscribed = state
        .db
        .list_subscribed_model_ids(&session.user_id)
        .await.map_err(db_err)?;
    if !subscribed.contains(&body.model_id) {
        return Err(AdminError::forbidden("未订阅此模型"));
    }

    // Load model to get channel bindings
    let model = state
        .db
        .get_model(&body.model_id)
        .await.map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("模型不存在"))?;

    // Find the first enabled channel for this model (by priority)
    let mut bindings = model.channels;
    bindings.sort_by_key(|b| b.priority);

    let channel_id = bindings
        .iter()
        .find_map(|b| state.routing.get_channel(&b.channel_id).filter(|ch| ch.enabled).map(|ch| ch.id.clone()))
        .ok_or_else(|| AdminError::internal("该模型没有可用的通道"))?;

    // Resolve provider adapter + endpoint from the channel
    let (provider_name, balancer) = state
        .routing
        .get_route(&channel_id)
        .ok_or_else(|| AdminError::internal("通道路由不可用"))?;

    let adapter = state
        .providers
        .get(&provider_name)
        .ok_or_else(|| AdminError::internal("未找到提供商适配器"))?;

    let (endpoint_idx, endpoint) = balancer
        .as_health_aware()
        .select()
        .ok_or_else(|| AdminError::internal("没有可用的端点"))?;

    // Send a connectivity probe with standard parameters.
    let start = std::time::Instant::now();
    let result = if provider_name == "anthropic" {
        let test_body = serde_json::json!({
            "model": model.model_pattern,
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 512,
        });
        adapter.messages(endpoint, test_body).await
    } else {
        let test_body = serde_json::json!({
            "model": model.model_pattern,
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": 0.01,
            "max_tokens": 512,
            "top_p": 0.01,
        });
        adapter.chat_complete(endpoint, test_body).await
    };
    let latency_ms = start.elapsed().as_millis() as u64;

    // Store probe result in database (persistent across restarts)
    {
        let row = crate::db::ProbeResultRow {
            id: uuid::Uuid::new_v4().to_string(),
            channel_id: channel_id.clone(),
            model_id: model.id.clone(),
            success: result.is_ok(),
            latency_ms,
            error: result.as_ref().err().map(|e| e.0.clone()),
            probed_at: chrono::Utc::now().to_rfc3339(),
        };
        let _ = state.db.insert_probe_result(&row).await;
    }

    match result {
        Ok(resp) => {
            balancer.as_health_aware().record_success(endpoint_idx);
            Ok(Json(serde_json::json!({
                "success": true,
                "model": resp.get("model"),
                "status": "ok",
                "latency_ms": latency_ms,
            })))
        }
        Err(e) => {
            balancer.as_health_aware().record_failure(endpoint_idx);
            Ok(Json(serde_json::json!({
                "success": false,
                "error": e.0,
                "latency_ms": latency_ms,
            })))
        }
    }
}
