use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::user::{SessionInfo, User};
use crate::server::AppState;

use super::*;

// ── Login ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct LoginReq {
    username: String,
    password: String,
}

pub(crate) async fn admin_login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginReq>,
) -> Result<axum::response::Response, AdminError> {
    // Rate limit login attempts by real peer IP
    let client_ip = addr.ip().to_string();
    if let Some(fwd) = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
    {
        tracing::debug!(real_ip = %client_ip, forwarded_for = %fwd, "login attempt");
    }
    state
        .rate_limiter
        .check_rpm(&format!("login:{}", client_ip), 10)
        .map_err(|_| AdminError::too_many_requests("Too many login attempts. Try again later."))?;

    // Authenticate against database (all users including admins)
    let user = state
        .db
        .get_user_with_password(&req.username)
        .await.map_err(db_err)?;

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
            }
        }
    } else {
        // Constant-time dummy to prevent user enumeration via timing
        let _ = bcrypt::verify(&req.password, "$2b$10$EixZaYVK1fsbw1ZfbX3OXePaWxn96p36PQm4sEPhMNPfFhpYN76Oe");
    }

    if password_matched {
        let u = user.unwrap();
        let info = SessionInfo {
            user_id: u.id.clone(),
            user_name: u.name.clone(),
            role: u.role.clone(),
            token_version: u.token_version,
        };
        let token = state.admin.encode_token(&info)?;
        // Set httpOnly cookie for browser-based admin UI (prevents XSS token theft)
        let cookie = format!(
            "session_token={}; HttpOnly; Path=/; SameSite=Strict; Max-Age=86400",
            token
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::SET_COOKIE,
            axum::http::HeaderValue::from_str(&cookie).unwrap(),
        );
        return Ok((
            headers,
            Json(serde_json::json!({
                "token": token,
                "role": u.role,
                "user_id": u.id,
                "user_name": u.name,
                "timezone": u.timezone,
                "currency": u.currency,
            })),
        ).into_response());
    }

    Err(AdminError::unauthorized("Invalid credentials"))
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
        .count_admins()
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
        .count_admins()
        .await
        .map_err(|e| AdminError::internal(e.to_string()))?;
    if count > 0 {
        return Err(AdminError::bad_request(
            "Admin already exists. Please log in.",
        ));
    }

    if req.username.is_empty() {
        return Err(AdminError::bad_request("Username is required"));
    }
    validate_password(&req.password)?;

    if state
        .db
        .get_user(&req.username)
        .await.map_err(db_err)?
        .is_some()
    {
        return Err(AdminError::bad_request("Username already exists"));
    }

    let hash =
        bcrypt::hash(&req.password, 10).map_err(|e| AdminError::internal(e.to_string()))?;

    let user = User {
        id: req.username.clone(),
        name: req.username.clone(),
        password_hash: Some(hash),
        rate_limits: None,
        timezone: "UTC".to_string(),
        token_version: 0,
        role: "admin".to_string(),
        concurrency_limit: 2000,
        currency: "usd".to_string(),
    };
    state.db.create_user(&user).await.map_err(db_err)?;
    state.auth.reload().await;

    tracing::info!("setup_register: first admin user created: {}", user.id);

    Ok(Json(serde_json::json!({ "ok": true })))
}
