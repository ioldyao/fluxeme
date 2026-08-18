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
use crate::db::Database;
use crate::domain::sso::{LiveSsoConfig, SsoConfigRow};
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
pub(crate) struct OidcProviderMetadata {
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
    /// Organizations the user belongs to on the IdP (Keycloak Organizations).
    /// Keycloak returns this claim only when the client has an Organizations
    /// mapper configured; falls back to empty when absent.
    #[serde(default)]
    organizations: Vec<crate::domain::sso::SsoOrg>,
}

// ── Pending state (maps CSRF state to selected SSO config) ──────

pub(crate) struct PendingState {
    expires: Instant,
    config_id: String,
    /// PKCE code_verifier used for this authorization request (S256).
    code_verifier: String,
}

/// Generate a PKCE S256 code_verifier / code_challenge pair.
/// code_verifier: 43-char base64url (32 random bytes, no padding).
/// code_challenge: base64url(sha256(verifier)).
fn pkce_pair() -> (String, String) {
    use base64::Engine as _;
    use sha2::Digest as _;

    // 32 random bytes from two CSPRNG-backed UUID v4s (getrandom under the hood).
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());

    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);

    (verifier, challenge)
}

// ── SSO Module ──────────────────────────────────────────────────

pub struct SsoModule {
    /// All loaded SSO configs (including disabled). Uses RwLock for interior
    /// mutability so admin handlers can reload without moving out of Arc.
    configs: std::sync::RwLock<Vec<LiveSsoConfig>>,
    http_client: reqwest::Client,
    pub(crate) pending_states: Arc<dashmap::DashMap<String, PendingState>>,
    pub(crate) enc_key: String,
    db: Arc<Database>,
}

impl SsoModule {
    /// Create a new SsoModule by loading all configs from the database.
    pub async fn new(enc_key: &str, db: Arc<Database>) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        let module = Self {
            configs: std::sync::RwLock::new(Vec::new()),
            http_client,
            pending_states: Arc::new(dashmap::DashMap::new()),
            enc_key: enc_key.to_string(),
            db,
        };
        module.reload_configs().await;
        module
    }

    /// Reload all SSO configs from the database.
    pub async fn reload_configs(&self) {
        let rows = match self.db.list_sso_configs().await {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!("Failed to load SSO configs from DB: {}", e);
                return;
            }
        };

        let total = rows.len();
        let configs: Vec<LiveSsoConfig> = rows
            .into_iter()
            .filter(|r| r.enabled)
            .filter_map(|r| self.resolve_config(r))
            .collect();

        let enabled_count = configs.len();
        *self.configs.write().unwrap() = configs;
        tracing::info!(
            "SSO configs loaded: {} enabled / {} total",
            enabled_count,
            total
        );
    }

    fn resolve_config(&self, row: SsoConfigRow) -> Option<LiveSsoConfig> {
        let client_secret = match row.client_secret_encrypted {
            Some(ref enc) => match crate::crypto::decrypt_load(enc, &self.enc_key) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!("Failed to decrypt SSO config {} secret: {}", row.id, e);
                    return None;
                }
            },
            None => {
                tracing::error!("SSO config {} has no encrypted secret", row.id);
                return None;
            }
        };

        let domain_restrictions = row.domain_restrictions.map(|d| {
            d.split(',')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        });

        Some(LiveSsoConfig {
            id: row.id,
            team_id: row.team_id,
            provider_name: row.provider_name,
            issuer_url: row.issuer_url,
            client_id: row.client_id,
            client_secret,
            redirect_url: row.redirect_url,
            enabled: row.enabled,
            auto_create_user: row.auto_create_user,
            domain_restrictions,
            default_role: row.default_role,
        })
    }

    pub fn is_enabled(&self) -> bool {
        !self.configs.read().unwrap().is_empty()
    }

    pub fn providers(&self) -> Vec<LiveSsoConfig> {
        self.configs.read().unwrap().iter().filter(|c| c.enabled).cloned().collect()
    }

    pub fn find_config(&self, config_id: &str) -> Option<LiveSsoConfig> {
        self.configs.read().unwrap().iter().find(|c| c.id == config_id).cloned()
    }

    /// Find the first enabled config matching an optional team_id.
    /// If team_id is Some, returns the team's config (if any).
    /// If team_id is None, returns the global config (team_id IS NULL).
    pub fn find_config_for_team(&self, team_id: Option<&str>) -> Option<LiveSsoConfig> {
        let configs = self.configs.read().unwrap();
        if let Some(tid) = team_id {
            if let Some(cfg) = configs.iter().find(|c| c.team_id.as_deref() == Some(tid)) {
                return Some(cfg.clone());
            }
        }
        // Fall back to global config (no team binding)
        configs.iter().find(|c| c.team_id.is_none()).cloned()
    }

    /// Discover OIDC metadata for a given issuer URL.
    pub async fn discover_metadata(
        &self,
        issuer_url: &str,
    ) -> Result<OidcProviderMetadata, AdminError> {
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            issuer_url.trim_end_matches('/')
        );
        let resp = self
            .http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| AdminError::internal(format!("OIDC discovery failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            tracing::error!(
                %status,
                %discovery_url,
                body = %body,
                "OIDC discovery endpoint returned non-success status"
            );
            return Err(AdminError::bad_request(format!(
                "OIDC discovery failed with status {}: {}",
                status.as_u16(),
                body
            )));
        }
        resp.json()
            .await
            .map_err(|e| AdminError::internal(format!("Failed to parse OIDC metadata: {e}")))
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
            .retain(|_, ps| ps.expires > Instant::now());

        // Verify CSRF state and bind it to the initiating browser.
        let pending = self.pending_states.remove(state).map(|(_, ps)| ps);
        if state_cookie != state || pending.is_none() {
            return Err(AdminError::unauthorized("Invalid or expired SSO state"));
        }
        let pending = pending.unwrap();

        // Find the SSO config for this pending state
        let cfg = self
            .find_config(&pending.config_id)
            .ok_or_else(|| AdminError::internal("SSO config not found for state"))?;

        // Discover OIDC metadata
        let metadata = self.discover_metadata(&cfg.issuer_url).await?;

        // Exchange authorization code for tokens (with PKCE code_verifier)
        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &cfg.redirect_url),
            ("client_id", &cfg.client_id),
            ("client_secret", &cfg.client_secret),
            ("code_verifier", &pending.code_verifier),
        ];

        let token_resp: TokenResponse = self
            .http_client
            .post(&metadata.token_endpoint)
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
            .get(&metadata.userinfo_endpoint)
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
            .or(user_info.email.clone())
            .unwrap_or_else(|| sub.clone());

        // Check domain restrictions
        if let Some(ref email) = user_info.email {
            if !cfg.is_domain_allowed(email) {
                return Err(AdminError::unauthorized(
                    "Your email domain is not allowed for this SSO provider",
                ));
            }
        } else if let Some(ref restrictions) = cfg.domain_restrictions {
            if !restrictions.is_empty() {
                return Err(AdminError::unauthorized(
                    "Email is required for domain-restricted SSO providers",
                ));
            }
        }

        let provider_scope = if cfg.provider_name.is_empty() {
            "oidc"
        } else {
            cfg.provider_name.as_str()
        };
        let user_id = format!("sso:{provider_scope}:{sub}");

        let user = match db.get_user(&user_id).await {
            Ok(Some(user)) => user,
            Ok(None) => {
                if !cfg.auto_create_user {
                    return Err(AdminError::unauthorized(
                        "User not found and auto-creation is disabled for this SSO provider",
                    ));
                }
                let user = User {
                    id: user_id.clone(),
                    name: user_name.clone(),
                    password_hash: None,
                    rate_limits: None,
                    timezone: "UTC".to_string(),
                    token_version: 0,
                    role: cfg.default_role.clone(),
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
            return Err(AdminError::unauthorized("User account is suspended"));
        }

        // Persist IdP organizations (Keycloak Organizations) for this user.
        if !user_info.organizations.is_empty() {
            let orgs_json = serde_json::to_string(&user_info.organizations)
                .map_err(|e| AdminError::internal(format!("Failed to serialize orgs: {e}")))?;
            db.upsert_sso_user_orgs(&user.id, &orgs_json)
                .await
                .map_err(|e| AdminError::internal(format!("Failed to save user orgs: {e}")))?;
        }

        // Auto-join team if this SSO config is team-scoped
        if let Some(ref team_id) = cfg.team_id {
            let is_member = db.get_team_member(team_id, &user.id).await.map_err(|e| {
                AdminError::internal(format!("Failed to check team membership: {e}"))
            })?;
            if is_member.is_none() {
                let role = if cfg.default_role == "admin" {
                    "admin"
                } else {
                    "member"
                };
                db.add_team_member(team_id, &user.id, role)
                    .await
                    .map_err(|e| {
                        AdminError::internal(format!("Failed to auto-join team: {e}"))
                    })?;
                tracing::info!(
                    "SSO auto-joined user {} to team {} with role {}",
                    user.id,
                    team_id,
                    role
                );
            }
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

#[derive(Deserialize)]
pub struct SsoLoginQuery {
    #[serde(default)]
    pub config_id: Option<String>,
    #[serde(default)]
    pub team_id: Option<String>,
}

/// SSO status endpoint (public, no auth needed)
pub async fn sso_status_handler(State(state): State<Arc<AppState>>) -> axum::Json<Value> {
    let providers: Vec<Value> = state
        .sso
        .providers()
        .iter()
        .map(|cfg| {
            serde_json::json!({
                "id": cfg.id,
                "provider_name": cfg.provider_name,
                "team_id": cfg.team_id,
                "enabled": cfg.enabled,
            })
        })
        .collect();

    axum::Json(serde_json::json!({
        "enabled": state.sso.is_enabled(),
        "providers": providers,
    }))
}

/// SSO login redirect handler
pub async fn sso_login_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<SsoLoginQuery>,
) -> Result<axum::response::Response, AdminError> {
    if !state.sso.is_enabled() {
        return Err(AdminError::unauthorized("SSO not enabled"));
    }

    // Find SSO config: prefer config_id, then team_id, then global
    let cfg = if let Some(ref cid) = query.config_id {
        state
            .sso
            .find_config(cid)
            .ok_or_else(|| AdminError::bad_request("SSO config not found"))?
    } else {
        state
            .sso
            .find_config_for_team(query.team_id.as_deref())
            .ok_or_else(|| AdminError::bad_request("No SSO config found"))?
    };

    let is_secure = should_set_secure_cookie(&headers);

    // Discover OIDC metadata and build auth URL
    let metadata = state.sso.discover_metadata(&cfg.issuer_url).await?;

    let sso_state = uuid::Uuid::new_v4().to_string();
    let (code_verifier, code_challenge) = pkce_pair();
    let auth_url = url::Url::parse_with_params(
        &metadata.authorization_endpoint,
        &[
            ("response_type", "code"),
            ("client_id", &cfg.client_id),
            ("redirect_uri", &cfg.redirect_url),
            ("scope", "openid profile email"),
            ("state", &sso_state),
            ("code_challenge", &code_challenge),
            ("code_challenge_method", "S256"),
        ],
    )
    .map_err(|e| AdminError::internal(format!("Failed to build auth URL: {e}")))?;

    // Store pending state with config_id reference + PKCE verifier
    state.sso.pending_states.insert(
        sso_state.clone(),
        PendingState {
            expires: Instant::now() + STATE_TTL,
            config_id: cfg.id.clone(),
            code_verifier,
        },
    );

    let state_cookie = sso_state_cookie_value(&sso_state, is_secure);
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        SET_COOKIE,
        HeaderValue::from_str(&state_cookie).map_err(|e| AdminError::internal(e.to_string()))?,
    );

    Ok((response_headers, Redirect::to(auth_url.as_str())).into_response())
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

    // The callback may have auto-created (or re-logged) a user; reload the
    // auth cache so the OIDC Resource Server can immediately resolve this SSO
    // identity from an external access token (Mode 2) too.
    state.auth.reload().await;

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
