use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::user::{SessionInfo, User, USER_STATUS_ACTIVE};
use crate::server::AppState;

use super::*;

// ── Login ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct LoginReq {
    username: String,
    password: String,
}

fn session_cookie_name(is_secure: bool) -> &'static str {
    if is_secure {
        HOST_SESSION_COOKIE_NAME
    } else {
        SESSION_COOKIE_NAME
    }
}

fn session_cookie_value(token: &str, is_secure: bool) -> String {
    let secure_attr = if is_secure { "; Secure" } else { "" };
    let cookie_name = session_cookie_name(is_secure);
    format!("{cookie_name}={token}; HttpOnly{secure_attr}; Path=/; SameSite=Strict; Max-Age=86400")
}

fn expired_session_cookie_value(name: &str, is_secure: bool) -> String {
    let secure_attr = if is_secure { "; Secure" } else { "" };
    format!("{name}=; HttpOnly{secure_attr}; Path=/; SameSite=Strict; Max-Age=0")
}

fn run_dummy_password_check(password: &str) {
    let _ = bcrypt::verify(
        password,
        "$2b$10$EixZaYVK1fsbw1ZfbX3OXePaWxn96p36PQm4sEPhMNPfFhpYN76Oe",
    );
}

fn should_return_login_token(headers: &HeaderMap) -> bool {
    headers.get("origin").is_none() && headers.get("referer").is_none()
}

pub(crate) async fn admin_login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginReq>,
) -> Result<axum::response::Response, AdminError> {
    // Rate limit login attempts by real peer IP
    let client_ip = addr.ip().to_string();
    if let Some(fwd) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        tracing::debug!(real_ip = %client_ip, forwarded_for = %fwd, "login attempt");
    }
    state
        .rate_limiter
        .check_rpm(&format!("login:{}", client_ip), 10)
        .await
        .map_err(|_| AdminError::too_many_requests("Too many login attempts. Try again later."))?;

    // Authenticate against database (all users including admins)
    let user = state
        .db
        .get_user_with_password(&req.username)
        .await
        .map_err(db_err)?;

    let mut password_matched = false;
    if let Some(ref u) = user {
        if let Some(ref hash) = u.password_hash {
            if !hash.is_empty() {
                match bcrypt::verify(&req.password, hash) {
                    Ok(true) => {
                        password_matched = true;
                    }
                    Ok(false) => { /* wrong password */ }
                    Err(e) => {
                        tracing::error!("bcrypt verify error for user {}: {}", u.id, e);
                        return Err(AdminError::internal("Authentication error"));
                    }
                }
            } else {
                run_dummy_password_check(&req.password);
            }
        } else {
            run_dummy_password_check(&req.password);
        }
    } else {
        run_dummy_password_check(&req.password);
    }

    if password_matched {
        let u = user.unwrap();
        if u.status != USER_STATUS_ACTIVE {
            return Err(AdminError::unauthorized("Invalid credentials"));
        }
        let info = SessionInfo {
            user_id: u.id.clone(),
            user_name: u.name.clone(),
            role: u.role.clone(),
            token_version: u.token_version,
        };
        let token = state.admin.encode_token(&info)?;
        let is_secure = should_set_secure_cookie(&headers);
        let cookie = session_cookie_value(&token, is_secure);
        let mut response_headers = HeaderMap::new();
        response_headers.insert(
            axum::http::header::SET_COOKIE,
            axum::http::HeaderValue::from_str(&cookie).unwrap(),
        );

        let mut response_body = serde_json::json!({
            "role": u.role,
            "user_id": u.id,
            "user_name": u.name,
            "timezone": u.timezone,
            "currency": u.currency,
        });
        if should_return_login_token(&headers) {
            response_body["token"] = serde_json::Value::String(token);
        }

        return Ok((response_headers, Json(response_body)).into_response());
    }

    Err(AdminError::unauthorized("Invalid credentials"))
}

// ── Session probe (anonymous-safe) ───────────────────────────────

/// Returns the current session status. Always returns 200 — anonymous
/// users get `{ "authenticated": false }` instead of a 401.
pub(crate) async fn auth_session(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Value>, AdminError> {
    // Try to extract and validate the session token (same logic as require_session
    // but without returning an error on missing/invalid tokens).
    let token = match extract_token(&headers) {
        Ok(t) => t,
        Err(_) => return Ok(Json(serde_json::json!({ "authenticated": false }))),
    };
    let info = match state.admin.decode_token(&token) {
        Ok(s) => s,
        Err(_) => return Ok(Json(serde_json::json!({ "authenticated": false }))),
    };
    let db_user = match state.db.get_user(&info.user_id).await {
        Ok(Some(u)) => u,
        _ => return Ok(Json(serde_json::json!({ "authenticated": false }))),
    };
    if db_user.token_version != info.token_version || db_user.status != USER_STATUS_ACTIVE {
        return Ok(Json(serde_json::json!({ "authenticated": false })));
    }

    // Build granted permissions list
    let all_known = [
        "admin:dashboard",
        "admin:users",
        "admin:channels",
        "admin:models",
        "admin:model-pricing",
        "admin:rules",
        "admin:moderation",
        "admin:usage",
        "admin:bills",
        "admin:recharge-keys",
        "admin:health",
        "admin:settings",
        "admin:gateway",
        "admin:policies",
        "admin:announcements",
        "admin:teams",
        "admin:skillhub",
        "admin:management-keys",
    ];
    let mut permissions = Vec::new();
    for perm in &all_known {
        if state.authz.enforce(&info.role, perm).await {
            permissions.push(perm.to_string());
        }
    }

    let teams = state
        .db
        .list_teams_for_user(&info.user_id)
        .await
        .unwrap_or_default();

    Ok(Json(serde_json::json!({
        "authenticated": true,
        "user": {
            "id": info.user_id,
            "name": info.user_name,
            "role": info.role,
        },
        "permissions": permissions,
        "portals": {
            "user": true,
            "admin": info.role == "admin",
        },
        "teams": teams,
    })))
}

// ── Logout ────────────────────────────────────────────────────────

pub(crate) async fn admin_logout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<axum::response::Response, AdminError> {
    let session = require_session(&state.admin, &headers).await?;
    state
        .db
        .bump_user_token_version(&session.user_id)
        .await
        .map_err(db_err)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    let is_secure = should_set_secure_cookie(&headers);
    let mut response_headers = HeaderMap::new();
    response_headers.append(
        axum::http::header::SET_COOKIE,
        axum::http::HeaderValue::from_str(&expired_session_cookie_value(
            HOST_SESSION_COOKIE_NAME,
            true,
        ))
        .map_err(|e| AdminError::internal(e.to_string()))?,
    );
    response_headers.append(
        axum::http::header::SET_COOKIE,
        axum::http::HeaderValue::from_str(&expired_session_cookie_value(
            SESSION_COOKIE_NAME,
            false,
        ))
        .map_err(|e| AdminError::internal(e.to_string()))?,
    );
    response_headers.append(
        axum::http::header::SET_COOKIE,
        axum::http::HeaderValue::from_str(&expired_session_cookie_value(
            session_cookie_name(is_secure),
            is_secure,
        ))
        .map_err(|e| AdminError::internal(e.to_string()))?,
    );

    Ok((response_headers, Json(serde_json::json!({ "ok": true }))).into_response())
}

// ── Setup (first-time admin registration) ─────────────────────────

#[derive(Serialize)]
pub(crate) struct SetupStatus {
    setup_required: bool,
}

pub(crate) async fn setup_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SetupStatus>, AdminError> {
    let count = state
        .db
        .count_admins(None)
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?;
    Ok(Json(SetupStatus {
        setup_required: count == 0,
    }))
}

#[derive(Deserialize)]
pub(crate) struct SetupRegisterReq {
    username: String,
    password: String,
}

pub(crate) async fn setup_register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetupRegisterReq>,
) -> Result<Json<Value>, AdminError> {
    let count = state
        .db
        .count_admins(None)
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?;
    if count > 0 {
        return Err(AdminError::bad_request("Setup already completed"));
    }

    let username = req.username.trim();
    if username.is_empty() {
        return Err(AdminError::bad_request("Username is required"));
    }

    validate_password(&req.password)?;

    let hash = bcrypt::hash(&req.password, 10).map_err(|e| AdminError::internal(e.to_string()))?;
    let user = User {
        id: username.to_string(),
        name: username.to_string(),
        password_hash: Some(hash),
        rate_limits: None,
        timezone: "UTC".to_string(),
        token_version: 0,
        role: "admin".to_string(),
        concurrency_limit: 2000,
        currency: "usd".to_string(),
        status: USER_STATUS_ACTIVE.to_string(),
        suspended_at: None,
    };

    state
        .db
        .create_initial_admin(&user)
        .await
        .map_err(db_err_bad_request)?;
    state.auth.reload().await;
    notify_config_changed(&state).await;

    Ok(Json(serde_json::json!({ "ok": true })))
}
