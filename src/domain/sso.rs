use serde::{Deserialize, Serialize};

/// SSO configuration stored in PostgreSQL.
/// A row with `team_id = NULL` is a global/personal SSO provider.
/// A row with `team_id = Some(...)` is a team-scoped SSO provider
/// whose users auto-join the team on first login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsoConfigRow {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub provider_name: String,
    pub issuer_url: String,
    pub client_id: String,
    /// AES-256-GCM encrypted (omit from API responses to frontend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_encrypted: Option<String>,
    pub redirect_url: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_auto_create_user")]
    pub auto_create_user: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_restrictions: Option<String>,
    #[serde(default = "default_role")]
    pub default_role: String,
    pub created_at: String,
    pub updated_at: String,
}

fn default_enabled() -> bool {
    true
}

fn default_auto_create_user() -> bool {
    true
}

fn default_role() -> String {
    "user".to_string()
}

/// A fully-resolved (decrypted) SSO config ready for OIDC operations,
/// held in-memory by SsoModule.
#[derive(Debug, Clone)]
pub struct LiveSsoConfig {
    pub id: String,
    pub team_id: Option<String>,
    pub provider_name: String,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: String, // decrypted
    pub redirect_url: String,
    pub enabled: bool,
    pub auto_create_user: bool,
    pub domain_restrictions: Option<Vec<String>>,
    pub default_role: String,
}

impl LiveSsoConfig {
    /// Check whether a user's email domain is allowed by this config's restrictions.
    pub fn is_domain_allowed(&self, email: &str) -> bool {
        let restrictions = match &self.domain_restrictions {
            Some(d) if !d.is_empty() => d,
            _ => return true, // no restrictions
        };
        let email_domain = email.split('@').nth(1).unwrap_or("");
        if email_domain.is_empty() {
            return false;
        }
        restrictions.iter().any(|allowed| allowed == email_domain)
    }
}

/// POST/PUT request body for SSO config from frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsoConfigRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub provider_name: String,
    pub issuer_url: String,
    pub client_id: String,
    /// Raw (unencrypted) secret from the frontend. The backend encrypts on write.
    pub client_secret: String,
    pub redirect_url: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_auto_create_user")]
    pub auto_create_user: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_restrictions: Option<String>,
    #[serde(default = "default_role")]
    pub default_role: String,
}

/// An organization (tenant) from the IdP (e.g. Keycloak Organizations),
/// returned in the OIDC userinfo / ID token `organizations` claim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsoOrg {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
}
