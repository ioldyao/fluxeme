use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::http::HeaderMap;

use crate::db::Database;
use crate::domain::user::{ApiKey, AuthResult, User, USER_STATUS_ACTIVE};
use crate::service::oidc::OidcResourceServer;

pub struct AuthService {
    db: Arc<Database>,
    users: RwLock<HashMap<String, User>>,
    api_keys: RwLock<HashMap<String, (User, ApiKey)>>,
    /// team_id -> (user_id -> role). Loaded on reload() so `authenticate`
    /// can resolve team membership synchronously (it has no await).
    team_memberships: RwLock<HashMap<String, HashMap<String, String>>>,
    /// Optional OAuth2/OIDC Resource Server (Mode 2): when a bearer token is
    /// not a gateway API key, try validating it as an access token issued by a
    /// trusted IdP (e.g. Keycloak). Attached at startup; see `attach_oidc`.
    oidc: RwLock<Option<Arc<OidcResourceServer>>>,
}

impl AuthService {
    pub async fn new(db: Arc<Database>) -> Self {
        let svc = Self {
            db,
            users: RwLock::new(HashMap::new()),
            api_keys: RwLock::new(HashMap::new()),
            team_memberships: RwLock::new(HashMap::new()),
            oidc: RwLock::new(None),
        };
        svc.reload().await;
        svc
    }

    /// Attach the OIDC Resource Server so `/v1/*` accepts access tokens issued
    /// by a trusted IdP in addition to gateway API keys.
    pub fn attach_oidc(&self, oidc: Arc<OidcResourceServer>) {
        *self.oidc.write().unwrap() = Some(oidc);
    }

    /// Reload all caches from database. Called after admin modifies users/keys.
    pub async fn reload(&self) {
        match self.db.all_api_keys().await {
            Ok(pairs) => {
                let mut map = HashMap::new();
                for (user, key) in &pairs {
                    map.insert(key.key.clone(), (user.clone(), key.clone()));
                }
                *self.api_keys.write().unwrap() = map;
            }
            Err(e) => tracing::error!("Failed to load API keys: {}", e),
        }

        match self.db.list_users(None).await {
            Ok(users) => {
                let map: HashMap<_, _> = users.into_iter().map(|u| (u.id.clone(), u)).collect();
                *self.users.write().unwrap() = map;
            }
            Err(e) => tracing::error!("Failed to load users: {}", e),
        }

        match self.db.all_team_members().await {
            Ok(members) => {
                let mut map: HashMap<String, HashMap<String, String>> = HashMap::new();
                for m in &members {
                    map.entry(m.team_id.clone())
                        .or_default()
                        .insert(m.user_id.clone(), m.role.clone());
                }
                *self.team_memberships.write().unwrap() = map;
            }
            Err(e) => tracing::error!("Failed to load team memberships: {}", e),
        }
    }

    pub fn authenticate(&self, headers: &HeaderMap) -> Result<AuthResult, AuthError> {
        let key = self
            .extract_key(headers)
            .ok_or_else(|| AuthError("Missing or invalid API key".into()))?;

        // Check user API keys
        {
            let ak = self.api_keys.read().unwrap();
            if let Some((user, api_key)) = ak.get(&key) {
                if !api_key.enabled {
                    return Err(AuthError("API key is disabled".into()));
                }
                if let Some(ref expires) = api_key.expires_at {
                    match chrono::DateTime::parse_from_rfc3339(expires) {
                        Ok(exp) => {
                            if chrono::Utc::now() > exp {
                                return Err(AuthError("API key has expired".into()));
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse expires_at '{}': {}", expires, e);
                        }
                    }
                }
                if user.status != USER_STATUS_ACTIVE {
                    return Err(AuthError("Unknown or disabled API key".into()));
                }
                // Team-scoped keys carry an active team context. For personal
                // keys (team_id = None) the fields stay None (existing behavior).
                let team_id = api_key.team_id.clone();
                let team_role = match &team_id {
                    Some(tid) => self
                        .team_memberships
                        .read()
                        .unwrap()
                        .get(tid)
                        .and_then(|by_user| by_user.get(&user.id))
                        .and_then(|role| crate::domain::team::TeamRole::from_str(role)),
                    None => None,
                };
                return Ok(AuthResult {
                    user_id: user.id.clone(),
                    user_name: user.name.clone(),
                    rate_limits: user
                        .rate_limits
                        .as_ref()
                        .map(|rl| (rl.rpm.unwrap_or(u64::MAX), rl.tpm.unwrap_or(u64::MAX))),
                    allowed_models: api_key.allowed_models.clone(),
                    api_key_name: api_key.name.clone(),
                    concurrency_limit: user.concurrency_limit,
                    team_id,
                    team_role,
                });
            }
        }

        // Mode 2: accept access tokens issued by a trusted IdP (OAuth2
        // Resource Server). Only reached when the presented key is not a
        // gateway API key. The token's `sub` maps to the SSO user created by
        // the gateway SSO login flow (`sso:{provider}:{sub}`).
        if let Some(oidc) = self.oidc.read().unwrap().as_ref() {
            match oidc.validate(&key) {
                Ok(subject) => {
                    let user = self.users.read().unwrap().get(&subject.user_id).cloned();
                    return match user {
                        Some(user) if user.status == USER_STATUS_ACTIVE => Ok(AuthResult {
                            user_id: user.id.clone(),
                            user_name: user.name.clone(),
                            rate_limits: user.rate_limits.as_ref().map(|rl| {
                                (rl.rpm.unwrap_or(u64::MAX), rl.tpm.unwrap_or(u64::MAX))
                            }),
                            allowed_models: None,
                            api_key_name: "oidc".to_string(),
                            concurrency_limit: user.concurrency_limit,
                            team_id: None,
                            team_role: None,
                        }),
                        Some(_) => Err(AuthError("User account is suspended".into())),
                        None => {
                            tracing::warn!(
                                user_id = %subject.user_id,
                                sub = %subject.sub,
                                issuer = %subject.issuer,
                                "Valid OIDC token but no matching SSO user; log into the gateway SSO once first"
                            );
                            Err(AuthError(
                                "OIDC identity has no gateway account; sign in via SSO first".into(),
                            ))
                        }
                    };
                }
                Err(e) => {
                    tracing::debug!(error = %e, "OIDC token validation failed");
                }
            }
        }

        Err(AuthError("Unknown or disabled API key".into()))
    }

    fn extract_key(&self, headers: &HeaderMap) -> Option<String> {
        if let Some(auth) = headers.get("authorization") {
            if let Ok(val) = auth.to_str() {
                if let Some(stripped) = val.strip_prefix("Bearer ") {
                    return Some(stripped.to_string());
                }
            }
        }
        if let Some(key) = headers.get("x-api-key") {
            if let Ok(val) = key.to_str() {
                return Some(val.to_string());
            }
        }
        None
    }
}

#[derive(Debug)]
pub struct AuthError(pub String);

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Authentication failed: {}", self.0)
    }
}
