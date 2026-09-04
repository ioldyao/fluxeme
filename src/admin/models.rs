use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::domain::channel::Channel;
use crate::domain::model::{MarketplaceFormats, MarketplaceModel, Model, Pricing};
use crate::server::AppState;

use super::*;

fn marketplace_projection(models: Vec<Model>, channels: Vec<Channel>) -> Vec<MarketplaceModel> {
    let channel_map = channels
        .into_iter()
        .map(|channel| (channel.id.clone(), channel))
        .collect::<std::collections::HashMap<_, _>>();
    let mut published = models
        .into_iter()
        .filter(|model| model.published)
        .collect::<Vec<_>>();
    published.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));

    let mut grouped = std::collections::BTreeMap::<String, Vec<Model>>::new();
    for model in published {
        grouped.entry(model.name.clone()).or_default().push(model);
    }

    grouped
        .into_iter()
        .filter_map(|(name, entries)| {
            let representative = entries.first()?;
            let mut formats = MarketplaceFormats::default();
            for entry in &entries {
                for binding in &entry.channels {
                    let Some(channel) = channel_map.get(&binding.channel_id) else {
                        continue;
                    };
                    if channel.provider.eq_ignore_ascii_case("openai") {
                        formats.openai = true;
                        formats.anthropic |= channel.anthropic_compat;
                    } else if channel.provider.eq_ignore_ascii_case("anthropic") {
                        formats.anthropic = true;
                    }
                }
            }
            Some(MarketplaceModel {
                name,
                pricing: representative.pricing.clone(),
                context_length: representative.context_length,
                category: representative.category.clone(),
                formats,
            })
        })
        .collect()
}

// ── Model CRUD ────────────────────────────────────────────────────

pub(crate) async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;
    let models = state.db.list_models().await.map_err(db_err)?;
    // Admin page handles visual grouping on frontend; return raw entries
    Ok(Json(models))
}

pub(crate) async fn create_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut model): Json<Model>,
) -> Result<Json<Model>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;

    normalize_and_validate_model(&mut model)?;

    state.db.create_model(&model).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=create_model target={}",
        session.user_id,
        model.id
    );

    Ok(Json(model))
}

pub(crate) async fn update_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(old_id): Path<String>,
    Json(mut model): Json<Model>,
) -> Result<Json<Model>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;

    normalize_and_validate_model(&mut model)?;
    if model.id != old_id {
        return Err(AdminError::bad_request("Model ID cannot be changed"));
    }
    state
        .db
        .update_model(&old_id, &model)
        .await
        .map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=update_model target={}",
        session.user_id,
        old_id
    );

    Ok(Json(model))
}

pub(crate) async fn delete_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;

    state.db.delete_model(&id).await.map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=delete_model target={}",
        session.user_id,
        id
    );

    Ok(Json(serde_json::json!({ "deleted": id })))
}

// ── Public Models (any authenticated user) ────────────────────────

pub(crate) async fn list_public_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<Model>>, AdminError> {
    require_session(&state.admin, &headers).await?;
    let models = state.db.list_published_models().await.map_err(db_err)?;
    Ok(Json(models))
}

pub(crate) async fn list_marketplace_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<MarketplaceModel>>, AdminError> {
    require_session(&state.admin, &headers).await?;
    let models = state.db.list_published_models().await.map_err(db_err)?;
    let channels = state.db.list_channels().await.map_err(db_err)?;
    Ok(Json(marketplace_projection(models, channels)))
}

pub(crate) async fn toggle_publish_model(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:models").await?;
    let models = state.db.list_models().await.map_err(db_err)?;
    let model = models
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AdminError::not_found("Model not found"))?;
    let new_status = !model.published;
    state
        .db
        .set_model_published(&id, new_status)
        .await
        .map_err(db_err)?;
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;

    tracing::info!(
        "admin={} action=toggle_publish_model target={} published={}",
        session.user_id,
        id,
        new_status
    );

    Ok(Json(
        serde_json::json!({ "id": id, "published": new_status }),
    ))
}

pub(crate) async fn update_model_pricing(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(pricing): Json<Pricing>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:model-pricing").await?;
    state
        .db
        .set_model_pricing(&id, &pricing)
        .await
        .map_err(db_err)?;

    tracing::info!(
        "admin={} action=update_model_pricing target={}",
        session.user_id,
        id
    );

    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Model Health Check ─────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ModelHealthCheckRequest {
    #[serde(default)]
    channel_ids: Vec<String>,
    #[serde(default)]
    stream: bool,
}

pub(crate) async fn model_health_check(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
    Json(request): Json<ModelHealthCheckRequest>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:health").await?;

    let results = state
        .health_probe
        .probe_model(&model_id, &request.channel_ids, request.stream)
        .await
        .map_err(AdminError::internal)?;

    Ok(Json(serde_json::json!({
        "model_id": model_id,
        "channel_results": results,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{ModelChannel, Pricing};

    fn model(id: &str, name: &str, published: bool, channel_id: &str) -> Model {
        Model {
            id: id.to_string(),
            name: name.to_string(),
            model_pattern: name.to_string(),
            pricing: Pricing::default(),
            channels: vec![ModelChannel {
                model_id: id.to_string(),
                channel_id: channel_id.to_string(),
                priority: 0,
                provider: String::new(),
                upstream_model: Some("internal-upstream-name".to_string()),
                max_tokens: None,
            }],
            published,
            context_length: Some(32_000),
            category: "chat".to_string(),
        }
    }

    fn channel(id: &str, provider: &str, anthropic_compat: bool) -> Channel {
        Channel {
            id: id.to_string(),
            name: id.to_string(),
            provider: provider.to_string(),
            enabled: true,
            anthropic_compat,
            endpoints: Vec::new(),
        }
    }

    #[test]
    fn marketplace_projection_filters_and_merges_by_display_name() {
        let models = vec![
            model("published", "DeepSeek-V4-Flash", true, "openai"),
            model("unpublished", "DeepSeek-V4-Flash", false, "anthropic"),
            model("other", "Claude", true, "anthropic"),
        ];
        let result = marketplace_projection(
            models,
            vec![
                channel("openai", "openai", false),
                channel("anthropic", "anthropic", false),
            ],
        );

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "Claude");
        assert_eq!(result[1].name, "DeepSeek-V4-Flash");
        assert!(result[1].formats.openai);
        assert!(!result[1].formats.anthropic);
    }

    #[test]
    fn marketplace_projection_aggregates_compatibility_without_internal_fields() {
        let mut configured = model("model", "DeepSeek-V4-Flash", true, "openai");
        configured.channels.push(ModelChannel {
            model_id: configured.id.clone(),
            channel_id: "openai-compat".to_string(),
            priority: 0,
            provider: String::new(),
            upstream_model: Some("another-internal-name".to_string()),
            max_tokens: None,
        });
        let result = marketplace_projection(
            vec![configured],
            vec![
                channel("openai", "openai", false),
                channel("openai-compat", "openai", true),
            ],
        );
        let json = serde_json::to_value(&result[0]).unwrap();

        assert!(result[0].formats.openai);
        assert!(result[0].formats.anthropic);
        assert!(json.get("id").is_none());
        assert!(json.get("model_pattern").is_none());
        assert!(json.get("channels").is_none());
        assert!(json.get("upstream_model").is_none());
    }
}

fn normalize_and_validate_model(model: &mut Model) -> Result<(), AdminError> {
    model.id = model.id.trim().to_string();
    model.name = model.name.trim().to_string();
    model.model_pattern = model.model_pattern.trim().to_string();
    if model.id.is_empty() {
        return Err(AdminError::bad_request("Model ID is required"));
    }
    if model.name.is_empty() {
        return Err(AdminError::bad_request("Model name is required"));
    }
    if model.model_pattern.is_empty() {
        return Err(AdminError::bad_request("Model pattern is required"));
    }
    if model
        .channels
        .iter()
        .any(|binding| binding.channel_id.trim().is_empty())
    {
        return Err(AdminError::bad_request("Channel ID cannot be empty"));
    }
    if model
        .channels
        .iter()
        .any(|binding| binding.max_tokens == Some(0))
    {
        return Err(AdminError::bad_request(
            "Binding max_tokens must be greater than zero",
        ));
    }
    Ok(())
}

// ── Probe Results ─────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProbeResultsQuery {
    model_id: Option<String>,
}

pub(crate) async fn list_probe_results(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ProbeResultsQuery>,
) -> Result<Json<Vec<crate::db::ProbeResultRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:health").await?;
    let results = state
        .health_probe
        .all_latest_probes()
        .await
        .map_err(AdminError::internal)?;
    let filtered = if let Some(model_id) = query.model_id.filter(|value| !value.trim().is_empty()) {
        results
            .into_iter()
            .filter(|row| row.model_id == model_id)
            .collect()
    } else {
        results
    };
    Ok(Json(filtered))
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct RecentProbesQuery {
    minutes: Option<i64>,
}

/// Raw probe results from the last N minutes (probe-driven timeline grid).
pub(crate) async fn list_recent_probes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<RecentProbesQuery>,
) -> Result<Json<Vec<crate::db::ProbeResultRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:health").await?;
    let minutes = query.minutes.unwrap_or(10).clamp(1, 60);
    // Probe results are observability data — ClickHouse only, no PG fallback.
    let ch = state
        .ch
        .as_ref()
        .ok_or_else(|| AdminError::internal("ClickHouse not configured"))?;
    let results = ch
        .recent_probe_results(minutes)
        .await
        .map_err(AdminError::internal)?;
    Ok(Json(results))
}
