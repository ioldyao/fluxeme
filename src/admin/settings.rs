use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use serde::Deserialize;
use serde_json::Value;

use crate::config::types::GatewayRuntimeConfig;
use crate::server::AppState;

use super::*;

// ── Global currency ─────────────────────────────────────────────────

pub(crate) async fn get_currency(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let currency = state
        .db
        .get_setting("site_currency")
        .await
        .map_err(db_err)?
        .unwrap_or_else(|| "usd".to_string());
    let rate: f64 = state
        .db
        .get_setting("exchange_rate")
        .await
        .map_err(db_err)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(7.2);
    Ok(Json(serde_json::json!({ "currency": currency, "rate": rate })))
}

#[derive(Deserialize)]
pub(crate) struct CurrencyReq {
    currency: String,
    rate: f64,
}

pub(crate) async fn set_currency(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CurrencyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    if !["usd", "cny"].contains(&req.currency.as_str()) {
        return Err(AdminError::bad_request("Invalid currency, must be 'usd' or 'cny'"));
    }
    if req.rate <= 0.0 {
        return Err(AdminError::bad_request("Exchange rate must be positive"));
    }
    state
        .db
        .set_setting("site_currency", &req.currency)
        .await
        .map_err(db_err)?;
    state
        .db
        .set_setting("exchange_rate", &req.rate.to_string())
        .await
        .map_err(db_err)?;
    Ok(Json(serde_json::json!({ "currency": req.currency, "rate": req.rate })))
}

// ── Settings ──────────────────────────────────────────────────────

pub(crate) async fn get_allow_private_ips(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let value = state
        .db
        .get_setting("allow_private_ips")
        .await
        .map_err(db_err)?;
    // Default to true when no setting is stored (matches AtomicBool default)
    let enabled = value.as_deref() != Some("false");
    Ok(Json(serde_json::json!({ "enabled": enabled })))
}

#[derive(Deserialize)]
pub(crate) struct AllowPrivateIpsReq {
    enabled: bool,
}

pub(crate) async fn set_allow_private_ips(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<AllowPrivateIpsReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let value = if req.enabled { "true" } else { "false" };
    state
        .db
        .set_setting("allow_private_ips", value)
        .await
        .map_err(db_err)?;
    crate::provider::set_allow_private_ips(req.enabled);
    Ok(Json(serde_json::json!({ "enabled": req.enabled })))
}

// ── Auto probe interval ────────────────────────────────────────────

const PROBE_INTERVAL_MIN_SECS: u64 = 10;
const PROBE_INTERVAL_MAX_SECS: u64 = 3600;

pub(crate) async fn get_probe_interval(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let value = state
        .db
        .get_setting("probe_interval_secs")
        .await
        .map_err(db_err)?;
    let interval_secs = value
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60)
        .clamp(PROBE_INTERVAL_MIN_SECS, PROBE_INTERVAL_MAX_SECS);
    Ok(Json(serde_json::json!({ "interval_secs": interval_secs })))
}

#[derive(Deserialize)]
pub(crate) struct ProbeIntervalReq {
    interval_secs: u64,
}

pub(crate) async fn set_probe_interval(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ProbeIntervalReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    if !(PROBE_INTERVAL_MIN_SECS..=PROBE_INTERVAL_MAX_SECS).contains(&req.interval_secs) {
        return Err(AdminError::bad_request(format!(
            "Probe interval must be between {} and {} seconds",
            PROBE_INTERVAL_MIN_SECS, PROBE_INTERVAL_MAX_SECS
        )));
    }
    state
        .db
        .set_setting("probe_interval_secs", &req.interval_secs.to_string())
        .await
        .map_err(db_err)?;
    Ok(Json(
        serde_json::json!({ "interval_secs": req.interval_secs }),
    ))
}

// ── Gateway Config ──────────────────────────────────────────────────

pub(crate) async fn get_gateway_config_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<GatewayRuntimeConfig>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:gateway").await?;
    let config = state.db.get_gateway_config().await.map_err(db_err)?;
    Ok(Json(config))
}

pub(crate) async fn set_gateway_config_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(config): Json<GatewayRuntimeConfig>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:gateway").await?;
    // Validate and persist
    state.db.set_gateway_config(&config).await.map_err(db_err)?;
    // Update in-memory config
    *state.gateway_config.write().unwrap() = config;
    Ok(Json(serde_json::json!({ "ok": true })))
}
