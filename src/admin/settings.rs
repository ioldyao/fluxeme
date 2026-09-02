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

/// Public app config — no auth required. Returns global display settings
/// such as currency that all frontends need.
pub(crate) async fn get_app_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Value>, AdminError> {
    let currency = state
        .db
        .get_setting("site_currency")
        .await
        .map_err(db_err)?
        .unwrap_or_else(|| "usd".to_string());
    Ok(Json(serde_json::json!({ "currency": currency })))
}

pub(crate) async fn set_currency(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CurrencyReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    if !["usd", "cny"].contains(&req.currency.as_str()) {
        return Err(AdminError::bad_request(
            "Invalid currency, must be 'usd' or 'cny'",
        ));
    }
    state
        .db
        .set_setting("site_currency", &req.currency)
        .await
        .map_err(db_err)?;
    Ok(Json(serde_json::json!({ "currency": req.currency })))
}

#[derive(Deserialize)]
pub(crate) struct CurrencyReq {
    currency: String,
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

// ── OIDC expected audience (Mode 2) ────────────────────────────────

/// GET /api/settings/oidc-audience — the `aud` claim Fluxeme requires on
/// external IdP access tokens. Empty/null = audience not checked.
pub(crate) async fn get_oidc_expected_audience(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let value = state
        .db
        .get_setting("oidc_expected_audience")
        .await
        .map_err(db_err)?;
    Ok(Json(serde_json::json!({ "audience": value })))
}

#[derive(Deserialize)]
pub(crate) struct OidcExpectedAudienceReq {
    /// Audience the external token's `aud` must contain. Empty disables.
    audience: String,
}

/// PUT /api/settings/oidc-audience — configure (or clear) the expected
/// audience, then apply it to the running OIDC resource server.
pub(crate) async fn set_oidc_expected_audience(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<OidcExpectedAudienceReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;

    let value = req.audience.trim().to_string();
    state
        .db
        .set_setting("oidc_expected_audience", &value)
        .await
        .map_err(db_err)?;

    let aud = if value.is_empty() {
        None
    } else {
        Some(value.clone())
    };
    state.oidc.set_expected_audience(aud);
    notify_config_changed(&state).await;

    Ok(Json(serde_json::json!({ "audience": value })))
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

// ── Circuit breaker parameters ────────────────────────────────────

/// GET /api/settings/breaker — current circuit breaker params.
/// Returns persisted values if set, otherwise the process defaults.
pub(crate) async fn get_breaker_params(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let threshold = state
        .db
        .get_setting("breaker_threshold")
        .await
        .map_err(db_err)?
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(crate::balancer::BREAKER_THRESHOLD_DEFAULT);
    let cooldown_secs = state
        .db
        .get_setting("breaker_cooldown_secs")
        .await
        .map_err(db_err)?
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(crate::balancer::BREAKER_COOLDOWN_DEFAULT);
    Ok(Json(serde_json::json!({
        "threshold": threshold,
        "cooldown_secs": cooldown_secs,
    })))
}

#[derive(Deserialize)]
pub(crate) struct BreakerParamsReq {
    threshold: u32,
    cooldown_secs: u64,
}

/// PUT /api/settings/breaker — persist and apply circuit breaker params.
/// Applied to newly rebuilt balancers; existing breakers keep state until the
/// next routing reload.
pub(crate) async fn set_breaker_params(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<BreakerParamsReq>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    if !(crate::balancer::BREAKER_THRESHOLD_MIN..=crate::balancer::BREAKER_THRESHOLD_MAX)
        .contains(&req.threshold)
    {
        return Err(AdminError::bad_request(format!(
            "threshold must be between {} and {}",
            crate::balancer::BREAKER_THRESHOLD_MIN,
            crate::balancer::BREAKER_THRESHOLD_MAX
        )));
    }
    if !(crate::balancer::BREAKER_COOLDOWN_MIN..=crate::balancer::BREAKER_COOLDOWN_MAX)
        .contains(&req.cooldown_secs)
    {
        return Err(AdminError::bad_request(format!(
            "cooldown_secs must be between {} and {}",
            crate::balancer::BREAKER_COOLDOWN_MIN,
            crate::balancer::BREAKER_COOLDOWN_MAX
        )));
    }
    state
        .db
        .set_setting("breaker_threshold", &req.threshold.to_string())
        .await
        .map_err(db_err)?;
    state
        .db
        .set_setting("breaker_cooldown_secs", &req.cooldown_secs.to_string())
        .await
        .map_err(db_err)?;
    crate::balancer::set_breaker_params(Some(req.threshold), Some(req.cooldown_secs));
    // Rebuild channel balancers so new params apply to live breakers.
    state.routing.reload().await.map_err(AdminError::internal)?;
    notify_config_changed(&state).await;
    Ok(Json(serde_json::json!({
        "threshold": req.threshold,
        "cooldown_secs": req.cooldown_secs,
    })))
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
