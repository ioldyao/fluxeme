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
            Err(e) => {
                tracing::error!(
                    "Failed to load API keys; rejecting cached credentials: {}",
                    e
                );
                self.api_keys.write().unwrap().clear();
            }
        }

        match self.db.list_users(None).await {
            Ok(users) => {
                let map: HashMap<_, _> = users.into_iter().map(|u| (u.id.clone(), u)).collect();
                *self.users.write().unwrap() = map;
            }
            Err(e) => {
                tracing::error!("Failed to load users; rejecting cached user state: {}", e);
                self.users.write().unwrap().clear();
            }
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
            Err(e) => {
                tracing::error!(
                    "Failed to load team memberships; rejecting cached memberships: {}",
                    e
                );
                self.team_memberships.write().unwrap().clear();
            }
        }
    }

    pub fn authenticate(&self, headers: &HeaderMap) -> Result<AuthResult, AuthError> {
        let key = self
            .extract_key(headers)
            .ok_or_else(|| AuthError("Missing or invalid API key".into()))?;
        if key.starts_with("mk-") {
            return Err(AuthError(
                "Management API key is not valid for data-plane APIs".into(),
            ));
        }

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
                            // An invalid expiry must never turn into an unlimited key.
                            tracing::warn!("Failed to parse expires_at '{}': {}", expires, e);
                            return Err(AuthError("API key has an invalid expiration".into()));
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
                if team_id.is_some() && team_role.is_none() {
                    // A team key is only valid while its owner remains an active
                    // member with a recognized role in that team.
                    return Err(AuthError("Unknown or disabled API key".into()));
                }
                return Ok(AuthResult {
                    user_id: user.id.clone(),
                    user_name: user.name.clone(),
                    rate_limits: user
                        .rate_limits
                        .as_ref()
                        .map(|rl| (rl.rpm.unwrap_or(u64::MAX), rl.tpm.unwrap_or(u64::MAX))),
                    allowed_models: api_key.allowed_models.clone(),
                    scopes: api_key.scopes.clone(),
                    key_kind: api_key.key_kind.clone(),
                    api_key_name: api_key.name.clone(),
                    concurrency_limit: user.concurrency_limit,
                    team_id,
                    team_role,
                    billing_group_id: api_key.billing_group_id.clone(),
                    billing_payment_mode: api_key.billing_payment_mode,
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
                            rate_limits: user
                                .rate_limits
                                .as_ref()
                                .map(|rl| (rl.rpm.unwrap_or(u64::MAX), rl.tpm.unwrap_or(u64::MAX))),
                            allowed_models: None,
                            scopes: None,
                            key_kind: "oidc".to_string(),
                            api_key_name: "oidc".to_string(),
                            concurrency_limit: user.concurrency_limit,
                            team_id: None,
                            team_role: None,
                            billing_group_id:
                                crate::domain::billing_group::DEFAULT_BILLING_GROUP_ID.to_string(),
                            billing_payment_mode:
                                crate::domain::billing_group::BillingPaymentMode::Metered,
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
                                "OIDC identity has no gateway account; sign in via SSO first"
                                    .into(),
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

impl AuthError {
    /// Classify this auth failure into a stable, machine-readable kind used
    /// by the gateway access-event security log. Classification is driven by
    /// the message produced at the rejection site so the public `AuthError`
    /// shape stays unchanged (backward compatible).
    pub fn kind(&self) -> AuthErrorKind {
        let m = self.0.as_str();
        if m.starts_with("Missing") {
            AuthErrorKind::MissingAuthorization
        } else if m.contains("Management") {
            AuthErrorKind::ManagementKeyDenied
        } else if m.contains("expired") {
            AuthErrorKind::ExpiredKey
        } else if m.starts_with("Unknown or disabled") {
            AuthErrorKind::InvalidKey
        } else if m.contains("disabled") {
            AuthErrorKind::DisabledKey
        } else if m.contains("suspended") {
            AuthErrorKind::SuspendedUser
        } else if m.contains("no gateway account") {
            AuthErrorKind::UnrecognizedSubject
        } else {
            AuthErrorKind::InvalidKey
        }
    }
}

/// Stable classification of a gateway data-plane auth failure, for the
/// security access-event log. Distinct from the human-readable `AuthError`
/// message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthErrorKind {
    /// No `Authorization: Bearer` header and no `x-api-key` header.
    MissingAuthorization,
    /// A credential was presented but does not resolve to a usable key or
    /// OIDC subject (unknown key, malformed bearer, invalid expiration).
    InvalidKey,
    /// A known key that is explicitly disabled.
    DisabledKey,
    /// A known key whose `expires_at` is in the past.
    ExpiredKey,
    /// A `mk-*` management key presented to the data plane.
    ManagementKeyDenied,
    /// A valid key whose owning user account is suspended.
    SuspendedUser,
    /// A valid OIDC token whose `sub` has no gateway SSO account.
    UnrecognizedSubject,
}

impl AuthErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingAuthorization => "missing_authorization",
            Self::InvalidKey => "invalid_key",
            Self::DisabledKey => "disabled_key",
            Self::ExpiredKey => "expired_key",
            Self::ManagementKeyDenied => "management_key_denied",
            Self::SuspendedUser => "suspended_user",
            Self::UnrecognizedSubject => "unrecognized_subject",
        }
    }
}

/// One-way fingerprint of a presented credential for the security access log.
/// The raw key is never stored; only a short SHA-256 hex prefix is kept so
/// the same key can be correlated across events without leaking it.
pub fn credential_fingerprint(credential: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(credential.as_bytes());
    hex::encode(&digest[..8])
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Authentication failed: {}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_error_kind_classifies_each_rejection_site() {
        assert_eq!(
            AuthError("Missing or invalid API key".into()).kind(),
            AuthErrorKind::MissingAuthorization
        );
        assert_eq!(
            AuthError("Unknown or disabled API key".into()).kind(),
            AuthErrorKind::InvalidKey
        );
        assert_eq!(
            AuthError("API key has an invalid expiration".into()).kind(),
            AuthErrorKind::InvalidKey
        );
        assert_eq!(
            AuthError("API key is disabled".into()).kind(),
            AuthErrorKind::DisabledKey
        );
        assert_eq!(
            AuthError("API key has expired".into()).kind(),
            AuthErrorKind::ExpiredKey
        );
        assert_eq!(
            AuthError("Management API key is not valid for data-plane APIs".into()).kind(),
            AuthErrorKind::ManagementKeyDenied
        );
        assert_eq!(
            AuthError("User account is suspended".into()).kind(),
            AuthErrorKind::SuspendedUser
        );
        assert_eq!(
            AuthError("OIDC identity has no gateway account; sign in via SSO first".into()).kind(),
            AuthErrorKind::UnrecognizedSubject
        );
    }

    #[test]
    fn auth_error_kind_has_stable_snake_case_labels() {
        assert_eq!(
            AuthErrorKind::MissingAuthorization.as_str(),
            "missing_authorization"
        );
        assert_eq!(AuthErrorKind::InvalidKey.as_str(), "invalid_key");
        assert_eq!(AuthErrorKind::DisabledKey.as_str(), "disabled_key");
        assert_eq!(AuthErrorKind::ExpiredKey.as_str(), "expired_key");
        assert_eq!(
            AuthErrorKind::ManagementKeyDenied.as_str(),
            "management_key_denied"
        );
        assert_eq!(AuthErrorKind::SuspendedUser.as_str(), "suspended_user");
        assert_eq!(
            AuthErrorKind::UnrecognizedSubject.as_str(),
            "unrecognized_subject"
        );
    }

    #[test]
    fn credential_fingerprint_is_deterministic_and_hides_the_raw_key() {
        let key = "sk-a-very-long-secret-api-key-123456";
        let first = credential_fingerprint(key);
        let second = credential_fingerprint(key);
        assert_eq!(first, second, "fingerprint must be deterministic");
        assert_ne!(first, key, "fingerprint must never equal the raw key");
        assert!(
            !key.contains(&first),
            "raw key must not embed the fingerprint"
        );
        assert_eq!(first.len(), 16, "fingerprint is 8 bytes hex-encoded");
        // A different key must produce a different fingerprint.
        assert_ne!(
            first,
            credential_fingerprint("sk-a-different-secret-api-key-654321")
        );
    }
}
