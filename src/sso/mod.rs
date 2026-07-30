use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::http::{header::SET_COOKIE, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Redirect};
use serde::Deserialize;
use serde_json::Value;

use crate::admin::{
    extract_cookie_value, should_set_secure_cookie, AdminError, HOST_SESSION_COOKIE_NAME,
    SESSION_COOKIE_NAME,
};
use crate::config::types::SsoConfig;
use crate::db::Database;
use crate::domain::user::{SessionInfo, User, USER_STATUS_ACTIVE};
use crate::server::AppState;

const STATE_TTL: Duration = Duration::from_secs(300);
const SSO_STATE_COOKIE_NAME: &str = "sso_state";
const HOST_SSO_STATE_COOKIE_NAME: &str = "__Host-sso_state";

fn session_cookie_name(is_secure: bool) -> &'static str {
    if is_secure {
        HOST_SESSION_COOKIE_NAME
    } else {
        SESSION_COOKIE_NAME
    }
}

fn sso_state_cookie_name(is_secure: bool) -> &'static str {
    if is_secure {
        HOST_SSO_STATE_COOKIE_NAME
    } else {
        SSO_STATE_COOKIE_NAME
    }
}

fn session_cookie_value(token: &str, is_secure: bool) -> String {
    let secure_attr = if is_secure { "; Secure" } else { "" };
    let cookie_name = session_cookie_name(is_secure);
    format!("{cookie_name}={token}; HttpOnly{secure_attr}; Path=/; SameSite=Strict; Max-Age=86400")
}

fn sso_state_cookie_value(state: &str, is_secure: bool) -> String {
    let secure_attr = if is_secure { "; Secure" } else { "" };
    let cookie_name = sso_state_cookie_name(is_secure);
    format!("{cookie_name}={state}; HttpOnly{secure_attr}; Path=/; SameSite=Lax; Max-Age=300")
}

fn expired_sso_state_cookie_value(name: &str, is_secure: bool) -> String {
    let secure_attr = if is_secure { "; Secure" } else { "" };
    format!("{name}=; HttpOnly{secure_attr}; Path=/; SameSite=Lax; Max-Age=0")
}

// ── OIDC discovery document ─────────────────────────────────────

#[derive(Deserialize)]
struct OidcProviderMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    userinfo_endpoint: String,
}

// ── Token response ──────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

// ── UserInfo response ───────────────────────────────────────────

#[derive(Deserialize)]
struct UserInfo {
    sub: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

// ── SSO Module ──────────────────────────────────────────────────

pub struct SsoModule {
    metadata: Option<OidcProviderMetadata>,
    client_id: String,
    client_secret: String,
    redirect_url: String,
    provider_name: String,
    enabled: bool,
    http_client: reqwest::Client,
    pending_states: Arc<dashmap::DashMap<String, Instant>>,
    enc_key: String,
}

impl SsoModule {
    pub async fn new(cfg: &SsoConfig, enc_key: &str) -> Result<Self, String> {
        if !cfg.enabled {
            return Ok(Self {
                metadata: None,
                client_id: String::new(),
                client_secret: String::new(),
                redirect_url: String::new(),
                provider_name: String::new(),
                enabled: false,
                http_client: reqwest::Client::new(),
                pending_states: Arc::new(dashmap::DashMap::new()),
                enc_key: String::new(),
            });
        }

        if cfg.issuer_url.is_empty()
            || cfg.client_id.is_empty()
            || cfg.client_secret.is_empty()
            || cfg.redirect_url.is_empty()
        {
            return Err("SSO is enabled but issuer_url, client_id, client_secret, and redirect_url must all be set".into());
        }

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

        // Discover OIDC metadata from the issuer
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            cfg.issuer_url.trim_end_matches('/')
        );
        let metadata: OidcProviderMetadata = http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| format!("OIDC discovery failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Failed to parse OIDC metadata: {e}"))?;

        Ok(Self {
            metadata: Some(metadata),
            client_id: cfg.client_id.clone(),
            client_secret: crate::crypto::encrypt_store(&cfg.client_secret, enc_key),
            redirect_url: cfg.redirect_url.clone(),
            provider_name: cfg.provider_name.clone(),
            enabled: true,
            http_client,
            pending_states: Arc::new(dashmap::DashMap::new()),
            enc_key: enc_key.to_string(),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn provider_name(&self) -> &str {
        &self.provider_name
    }

    /// Generate the authorization URL and store CSRF state.
    pub fn authorize_url(&self) -> Result<(String, String), AdminError> {
        let meta = self
            .metadata
            .as_ref()
            .ok_or_else(|| AdminError::internal("SSO not configured"))?;

        self.pending_states
            .retain(|_, expires| *expires > Instant::now());

        let state = uuid::Uuid::new_v4().to_string();
        let auth_url = url::Url::parse_with_params(
            &meta.authorization_endpoint,
            &[
                ("response_type", "code"),
                ("client_id", &self.client_id),
                ("redirect_uri", &self.redirect_url),
                ("scope", "openid profile email"),
                ("state", &state),
            ],
        )
        .map_err(|e| AdminError::internal(format!("Failed to build auth URL: {e}")))?;

        self.pending_states
            .insert(state.clone(), Instant::now() + STATE_TTL);

        Ok((auth_url.to_string(), state))
    }

    /// Handle the OIDC callback: exchange code, fetch user info, create/find user, return JWT.
    pub async fn handle_callback(
        &self,
        code: &str,
        state: &str,
        state_cookie: &str,
        admin: &crate::admin::AdminModule,
        db: &Database,
    ) -> Result<String, AdminError> {
        // Clean up expired states
        self.pending_states
            .retain(|_, expires| *expires > Instant::now());

        // Verify CSRF state and bind it to the initiating browser.
        if state_cookie != state || self.pending_states.remove(state).is_none() {
            return Err(AdminError::unauthorized("Invalid or expired SSO state"));
        }

        let meta = self
            .metadata
            .as_ref()
            .ok_or_else(|| AdminError::internal("SSO not configured"))?;

        // Exchange authorization code for tokens
        let client_secret = crate::crypto::decrypt_load(&self.client_secret, &self.enc_key)
            .map_err(|e| AdminError::internal(format!("SSO secret decryption failed: {e}")))?;
        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.redirect_url),
            ("client_id", &self.client_id),
            ("client_secret", &client_secret),
        ];

        let token_resp: TokenResponse = self
            .http_client
            .post(&meta.token_endpoint)
            .form(&params)
            .send()
            .await
            .map_err(|e| AdminError::internal(format!("Token exchange failed: {e}")))?
            .json()
            .await
            .map_err(|e| AdminError::internal(format!("Failed to parse token response: {e}")))?;

        // Fetch user info with the access token
        let user_info: UserInfo = self
            .http_client
            .get(&meta.userinfo_endpoint)
            .header(
                "Authorization",
                format!("Bearer {}", token_resp.access_token),
            )
            .send()
            .await
            .map_err(|e| AdminError::internal(format!("UserInfo request failed: {e}")))?
            .json()
            .await
            .map_err(|e| AdminError::internal(format!("Failed to parse user info: {e}")))?;

        let sub = user_info.sub;
        let user_name = user_info
            .name
            .or(user_info.preferred_username)
            .or(user_info.email)
            .unwrap_or_else(|| sub.clone());
        let provider_scope = if self.provider_name.is_empty() {
            "oidc"
        } else {
            self.provider_name.as_str()
        };
        let user_id = format!("sso:{provider_scope}:{sub}");

        let user = match db.get_user(&user_id).await {
            Ok(Some(user)) => user,
            Ok(None) => {
                let user = User {
                    id: user_id.clone(),
                    name: user_name.clone(),
                    password_hash: None,
                    rate_limits: None,
                    timezone: "UTC".to_string(),
                    token_version: 0,
                    role: "user".to_string(),
                    concurrency_limit: 2000,
                    currency: "usd".to_string(),
                    status: USER_STATUS_ACTIVE.to_string(),
                    suspended_at: None,
                };
                db.create_user(&user)
                    .await
                    .map_err(|e| AdminError::internal(format!("Failed to create user: {e}")))?;
                user
            }
            Err(e) => {
                return Err(AdminError::internal(format!("Failed to load user: {e}")));
            }
        };

        if user.status != USER_STATUS_ACTIVE {
            return Err(AdminError::unauthorized("Invalid or expired SSO state"));
        }

        let info = SessionInfo {
            user_id: user.id.clone(),
            user_name: user.name.clone(),
            role: user.role.clone(),
            token_version: user.token_version,
        };

        admin.encode_token(&info)
    }
}

// ── HTTP handlers ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SsoCallbackParams {
    pub code: String,
    pub state: String,
}

/// SSO status endpoint (public, no auth needed)
pub async fn sso_status_handler(State(state): State<Arc<AppState>>) -> axum::Json<Value> {
    axum::Json(serde_json::json!({
        "enabled": state.sso.is_enabled(),
        "provider_name": state.sso.provider_name(),
    }))
}

/// SSO login redirect handler
pub async fn sso_login_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<axum::response::Response, AdminError> {
    if !state.sso.is_enabled() {
        return Err(AdminError::unauthorized("SSO not enabled"));
    }

    let is_secure = should_set_secure_cookie(&headers);
    let (auth_url, sso_state) = state.sso.authorize_url()?;
    let state_cookie = sso_state_cookie_value(&sso_state, is_secure);
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        SET_COOKIE,
        HeaderValue::from_str(&state_cookie).map_err(|e| AdminError::internal(e.to_string()))?,
    );

    Ok((response_headers, Redirect::to(&auth_url)).into_response())
}

/// SSO callback handler
pub async fn sso_callback_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<SsoCallbackParams>,
) -> Result<axum::response::Response, AdminError> {
    if !state.sso.is_enabled() {
        return Err(AdminError::unauthorized("SSO not enabled"));
    }

    let is_secure = should_set_secure_cookie(&headers);
    let state_cookie = extract_cookie_value(&headers, HOST_SSO_STATE_COOKIE_NAME)
        .or_else(|| extract_cookie_value(&headers, SSO_STATE_COOKIE_NAME))
        .ok_or_else(|| AdminError::unauthorized("Invalid or expired SSO state"))?;
    let token = state
        .sso
        .handle_callback(
            &params.code,
            &params.state,
            &state_cookie,
            &state.admin,
            &state.db,
        )
        .await?;

    let session_cookie = session_cookie_value(&token, is_secure);
    let mut response_headers = HeaderMap::new();
    response_headers.append(
        SET_COOKIE,
        HeaderValue::from_str(&session_cookie).map_err(|e| AdminError::internal(e.to_string()))?,
    );
    response_headers.append(
        SET_COOKIE,
        HeaderValue::from_str(&expired_sso_state_cookie_value(
            HOST_SSO_STATE_COOKIE_NAME,
            true,
        ))
        .map_err(|e| AdminError::internal(e.to_string()))?,
    );
    response_headers.append(
        SET_COOKIE,
        HeaderValue::from_str(&expired_sso_state_cookie_value(
            SSO_STATE_COOKIE_NAME,
            false,
        ))
        .map_err(|e| AdminError::internal(e.to_string()))?,
    );

    Ok((response_headers, Redirect::to("/sso/callback")).into_response())
}
