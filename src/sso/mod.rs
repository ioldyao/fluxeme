use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Query, State};
use axum::response::Redirect;
use serde::Deserialize;
use serde_json::Value;

use crate::admin::AdminError;
use crate::config::types::SsoConfig;
use crate::db::Database;
use crate::domain::user::{SessionInfo, User};
use crate::server::AppState;

const STATE_TTL: Duration = Duration::from_secs(300);

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
}

impl SsoModule {
    pub async fn new(cfg: &SsoConfig) -> Result<Self, String> {
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
            client_secret: cfg.client_secret.clone(),
            redirect_url: cfg.redirect_url.clone(),
            provider_name: cfg.provider_name.clone(),
            enabled: true,
            http_client,
            pending_states: Arc::new(dashmap::DashMap::new()),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn provider_name(&self) -> &str {
        &self.provider_name
    }

    /// Generate the authorization URL and store CSRF state.
    pub fn authorize_url(&self) -> Result<String, AdminError> {
        let meta = self
            .metadata
            .as_ref()
            .ok_or_else(|| AdminError::internal("SSO not configured"))?;

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
            .insert(state, Instant::now() + STATE_TTL);

        Ok(auth_url.to_string())
    }

    /// Handle the OIDC callback: exchange code, fetch user info, create/find user, return JWT.
    pub async fn handle_callback(
        &self,
        code: &str,
        state: &str,
        admin: &crate::admin::AdminModule,
        db: &Database,
    ) -> Result<String, AdminError> {
        // Clean up expired states
        self.pending_states.retain(|_, expires| *expires > Instant::now());

        // Verify CSRF state
        if self.pending_states.remove(state).is_none() {
            return Err(AdminError::unauthorized("Invalid or expired SSO state"));
        }

        let meta = self
            .metadata
            .as_ref()
            .ok_or_else(|| AdminError::internal("SSO not configured"))?;

        // Exchange authorization code for tokens
        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.redirect_url),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
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
            .header("Authorization", format!("Bearer {}", token_resp.access_token))
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

        // Create user if not exists
        if db.get_user(&sub).unwrap_or(None).is_none() {
            let user = User {
                id: sub.clone(),
                name: user_name.clone(),
                password_hash: None,
                rate_limits: None,
            };
            db.create_user(&user)
                .map_err(|e| AdminError::internal(format!("Failed to create user: {e}")))?;
        }

        let info = SessionInfo {
            user_id: sub.clone(),
            user_name: user_name.clone(),
            role: "user".to_string(),
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
pub async fn sso_status_handler(
    State(state): State<Arc<AppState>>,
) -> axum::Json<Value> {
    axum::Json(serde_json::json!({
        "enabled": state.sso.is_enabled(),
        "provider_name": state.sso.provider_name(),
    }))
}

/// SSO login redirect handler
pub async fn sso_login_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Redirect, AdminError> {
    if !state.sso.is_enabled() {
        return Err(AdminError::unauthorized("SSO not enabled"));
    }

    let auth_url = state.sso.authorize_url()?;
    Ok(Redirect::to(&auth_url))
}

/// SSO callback handler
pub async fn sso_callback_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SsoCallbackParams>,
) -> Result<Redirect, AdminError> {
    if !state.sso.is_enabled() {
        return Err(AdminError::unauthorized("SSO not enabled"));
    }

    let token = state
        .sso
        .handle_callback(&params.code, &params.state, &state.admin, &state.db)
        .await?;

    Ok(Redirect::to(&format!("/sso/callback#token={token}")))
}
