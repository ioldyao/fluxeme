use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::Value;

use crate::domain::sso::{SsoConfigRequest, SsoConfigRow};
use crate::server::AppState;

use super::*;

/// List all SSO configurations (admin:settings).
pub(crate) async fn list_sso_configs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<SsoConfigRow>>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let configs = state.db.list_sso_configs().await.map_err(db_err)?;
    // Strip encrypted secrets from API responses
    let configs = configs
        .into_iter()
        .map(|mut c| {
            c.client_secret_encrypted = None;
            c
        })
        .collect();
    Ok(Json(configs))
}

/// Get a single SSO configuration (admin:settings).
pub(crate) async fn get_sso_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<SsoConfigRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;
    let mut config = state
        .db
        .get_sso_config(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("SSO config not found"))?;
    config.client_secret_encrypted = None;
    Ok(Json(config))
}

/// Create a new SSO configuration (admin:settings).
pub(crate) async fn create_sso_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<SsoConfigRequest>,
) -> Result<Json<SsoConfigRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;

    if req.issuer_url.is_empty() || req.client_id.is_empty() || req.client_secret.is_empty() {
        return Err(AdminError::bad_request(
            "issuer_url, client_id, and client_secret are required",
        ));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();

    // Encrypt the client secret for storage
    let enc_key = &state.sso.enc_key;
    let encrypted = crate::crypto::encrypt_store(&req.client_secret, enc_key);

    let config = SsoConfigRow {
        id: id.clone(),
        team_id: req.team_id,
        provider_name: req.provider_name,
        issuer_url: req.issuer_url,
        client_id: req.client_id,
        client_secret_encrypted: Some(encrypted),
        redirect_url: req.redirect_url,
        enabled: req.enabled,
        auto_create_user: req.auto_create_user,
        domain_restrictions: req.domain_restrictions,
        default_role: if req.default_role.is_empty() {
            "user".to_string()
        } else {
            req.default_role
        },
        created_at: now.clone(),
        updated_at: now,
    };

    state.db.create_sso_config(&config).await.map_err(db_err)?;

    // Reload SSO configs in-memory
    state.sso.reload_configs().await;

    // Refresh the OIDC resource server (trusted issuers + JWKS) so access
    // tokens from the new/changed provider validate immediately, and bump
    // config_version so other gateway instances converge too.
    state.oidc.refresh(&state.sso.providers()).await;
    notify_config_changed(&state).await;

    let mut response = config;
    response.client_secret_encrypted = None;
    Ok(Json(response))
}

/// Update an existing SSO configuration (admin:settings).
pub(crate) async fn update_sso_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<SsoConfigRequest>,
) -> Result<Json<SsoConfigRow>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;

    let existing = state
        .db
        .get_sso_config(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("SSO config not found"))?;

    let now = chrono::Utc::now().to_rfc3339();

    // Encrypt the client secret (or keep existing if not changing)
    let encrypted = if req.client_secret.is_empty() {
        existing.client_secret_encrypted.unwrap_or_default()
    } else {
        let enc_key = &state.sso.enc_key;
        crate::crypto::encrypt_store(&req.client_secret, enc_key)
    };

    let config = SsoConfigRow {
        id,
        team_id: req.team_id,
        provider_name: req.provider_name,
        issuer_url: req.issuer_url,
        client_id: req.client_id,
        client_secret_encrypted: Some(encrypted),
        redirect_url: req.redirect_url,
        enabled: req.enabled,
        auto_create_user: req.auto_create_user,
        domain_restrictions: req.domain_restrictions,
        default_role: if req.default_role.is_empty() {
            "user".to_string()
        } else {
            req.default_role
        },
        created_at: existing.created_at,
        updated_at: now,
    };

    state.db.update_sso_config(&config).await.map_err(db_err)?;

    // Reload SSO configs in-memory
    state.sso.reload_configs().await;

    // Refresh the OIDC resource server (trusted issuers + JWKS) so access
    // tokens from the new/changed provider validate immediately, and bump
    // config_version so other gateway instances converge too.
    state.oidc.refresh(&state.sso.providers()).await;
    notify_config_changed(&state).await;

    let mut response = config;
    response.client_secret_encrypted = None;
    Ok(Json(response))
}

/// Delete an SSO configuration (admin:settings).
pub(crate) async fn delete_sso_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    check_perm(&state.authz, &session, "admin:settings").await?;

    state
        .db
        .get_sso_config(&id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| AdminError::not_found("SSO config not found"))?;

    state.db.delete_sso_config(&id).await.map_err(db_err)?;

    // Reload SSO configs in-memory
    state.sso.reload_configs().await;

    // Refresh the OIDC resource server (trusted issuers + JWKS) so access
    // tokens from the new/changed provider validate immediately, and bump
    // config_version so other gateway instances converge too.
    state.oidc.refresh(&state.sso.providers()).await;
    notify_config_changed(&state).await;

    Ok(Json(serde_json::json!({"ok": true})))
}
