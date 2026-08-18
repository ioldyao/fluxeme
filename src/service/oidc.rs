//! OAuth2 / OIDC Resource Server ("Mode 2").
//!
//! Accepts access tokens issued by a trusted IdP (e.g. Keycloak) directly, in
//! place of a gateway-issued API key. The token is validated by checking its
//! RS256 signature against the IdP's JWKS, plus the `iss` (must be a trusted
//! issuer = an enabled SSO config's `issuer_url`) and `exp` claims. On success
//! the token's `sub` maps back to the gateway user created by the SSO login
//! flow (`sso:{provider_name}:{sub}`).
//!
//! `aud` is intentionally NOT validated: Keycloak sets an access token's `aud`
//! to the client that requested it (e.g. the portal), which is not the gateway.
//! Signature + issuer + expiry are the trust boundary here. If stricter
//! audience checking is needed later, add an allowed-audiences knob and call
//! `Validation::set_audience`.
//!
//! The hot path (`validate`) is synchronous — it only reads in-memory caches —
//! so it plugs into `AuthService::authenticate`, which has no `await`.
//! Caches are kept warm by `refresh()`, called at startup and on every SSO
//! config change (admin API + cross-instance config-version polling).

use std::collections::HashMap;
use std::sync::RwLock;

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

use crate::domain::sso::LiveSsoConfig;

/// Normalize an issuer URL for map keys / comparisons (strip trailing '/').
fn normalize_issuer(iss: &str) -> String {
    iss.trim_end_matches('/').to_string()
}

/// A trusted IdP that may issue access tokens for this gateway.
/// Derived from an enabled SSO config's `issuer_url` + `provider_name`.
#[derive(Debug, Clone)]
pub struct TrustedIssuer {
    /// Normalized issuer URL (e.g. `https://kc/realms/master`).
    pub issuer_url: String,
    /// Provider name used to build the gateway user id (`sso:{name}:{sub}`).
    pub provider_name: String,
}

/// A successfully validated external access token.
#[derive(Debug, Clone)]
pub struct OidcSubject {
    /// Subject from the IdP (`sub` claim).
    pub sub: String,
    /// Issuer the token was validated against.
    pub issuer: String,
    /// Gateway user id (`sso:{provider_name}:{sub}`) the token maps to.
    pub user_id: String,
}

/// Validation failure reasons. Callers translate to an auth error.
#[derive(Debug)]
pub enum OidcError {
    /// Token is not a JWT (e.g. a plain gateway API key).
    NotJwt,
    /// `iss` does not match any trusted issuer.
    UntrustedIssuer,
    /// JWKS for this issuer is not cached yet (call `refresh()`).
    JwksNotReady,
    /// No JWK matches the token's `kid`.
    JwksKeyNotFound,
    /// Signature verification failed.
    InvalidSignature,
    /// Token expired.
    Expired,
    /// Claim validation failed (malformed / missing required claims).
    InvalidClaims,
}

impl std::fmt::Display for OidcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            OidcError::NotJwt => "not a JWT",
            OidcError::UntrustedIssuer => "untrusted issuer",
            OidcError::JwksNotReady => "JWKS not ready",
            OidcError::JwksKeyNotFound => "signing key (kid) not found",
            OidcError::InvalidSignature => "invalid signature",
            OidcError::Expired => "token expired",
            OidcError::InvalidClaims => "invalid claims",
        };
        write!(f, "{msg}")
    }
}

impl std::error::Error for OidcError {}

/// Minimal OIDC discovery document (only what the resource server needs).
#[derive(Deserialize)]
struct OidcDiscovery {
    jwks_uri: String,
}

/// JWT payload claims read *before* signature verification — used only to pick
/// the right trusted issuer / JWKS and to reject obviously-expired tokens early.
#[derive(Deserialize)]
struct PayloadClaims {
    #[serde(default)]
    iss: Option<String>,
    #[serde(default)]
    exp: Option<i64>,
}

/// Claims shape jsonwebtoken deserializes into after signature verification.
#[derive(Deserialize)]
struct VerifiedClaims {
    sub: String,
}

/// Decode the (unverified) JWT payload segment.
fn decode_payload(token: &str) -> Option<PayloadClaims> {
    use base64::Engine as _;
    let payload_b64 = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub struct OidcResourceServer {
    http_client: reqwest::Client,
    /// Trusted issuers (kept in sync with the enabled SSO configs).
    issuers: RwLock<Vec<TrustedIssuer>>,
    /// normalized issuer -> cached JWKS.
    jwks: RwLock<HashMap<String, JwkSet>>,
}

impl OidcResourceServer {
    pub fn new() -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("Failed to create OIDC HTTP client");
        Self {
            http_client,
            issuers: RwLock::new(Vec::new()),
            jwks: RwLock::new(HashMap::new()),
        }
    }

    /// Replace trusted issuers from the given SSO configs and (re)fetch their
    /// JWKS. On a per-issuer fetch failure the previous keys are kept. Removed
    /// providers drop out of the cache. Called at startup and on SSO config
    /// changes.
    pub async fn refresh(&self, providers: &[LiveSsoConfig]) {
        let issuers: Vec<TrustedIssuer> = providers
            .iter()
            .map(|c| TrustedIssuer {
                issuer_url: normalize_issuer(&c.issuer_url),
                provider_name: c.provider_name.clone(),
            })
            .collect();

        let mut next: HashMap<String, JwkSet> = HashMap::new();
        for cfg in providers {
            let key = normalize_issuer(&cfg.issuer_url);
            match self.fetch_jwks(cfg).await {
                Ok(set) => {
                    next.insert(key.clone(), set);
                }
                Err(e) => {
                    if let Some(prev) = self.jwks.read().unwrap().get(&key).cloned() {
                        next.insert(key, prev);
                    }
                    tracing::warn!(
                        issuer = %cfg.issuer_url,
                        error = %e,
                        "Failed to refresh OIDC JWKS; keeping previous keys"
                    );
                }
            }
        }

        *self.issuers.write().unwrap() = issuers;
        *self.jwks.write().unwrap() = next;
    }

    async fn fetch_jwks(&self, cfg: &LiveSsoConfig) -> Result<JwkSet, String> {
        let discovery_url = format!(
            "{}/.well-known/openid-configuration",
            normalize_issuer(&cfg.issuer_url)
        );
        let resp = self
            .http_client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("discovery returned {}", resp.status().as_u16()));
        }
        let meta: OidcDiscovery = resp.json().await.map_err(|e| e.to_string())?;

        let jwks_resp = self
            .http_client
            .get(&meta.jwks_uri)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !jwks_resp.status().is_success() {
            return Err(format!("jwks_uri returned {}", jwks_resp.status().as_u16()));
        }
        jwks_resp.json().await.map_err(|e| e.to_string())
    }

    /// Whether any trusted issuer is configured (for startup logging).
    pub fn is_trusting_any(&self) -> bool {
        !self.issuers.read().unwrap().is_empty()
    }

    /// Validate an external access token against the cached JWKS. Synchronous.
    pub fn validate(&self, token: &str) -> Result<OidcSubject, OidcError> {
        // Header: alg + kid (no signature check yet).
        let header = decode_header(token).map_err(|_| OidcError::NotJwt)?;
        let kid = header.kid.as_deref().ok_or(OidcError::JwksKeyNotFound)?;

        // Payload: iss + exp, to pick the trusted issuer and reject stale tokens.
        let payload = decode_payload(token).ok_or(OidcError::NotJwt)?;
        let iss = payload.iss.as_deref().ok_or(OidcError::UntrustedIssuer)?;
        if let Some(exp) = payload.exp {
            let now = chrono::Utc::now().timestamp();
            if exp < now - 60 {
                return Err(OidcError::Expired);
            }
        }

        // Find the trusted issuer matching this token's `iss`.
        let issuer_key = normalize_issuer(iss);
        let issuer = self
            .issuers
            .read()
            .unwrap()
            .iter()
            .find(|t| t.issuer_url == issuer_key)
            .cloned()
            .ok_or(OidcError::UntrustedIssuer)?;

        // Cached JWKS for this issuer + key by `kid`.
        let jwks = self
            .jwks
            .read()
            .unwrap()
            .get(&issuer_key)
            .cloned()
            .ok_or(OidcError::JwksNotReady)?;
        let jwk = jwks
            .keys
            .iter()
            .find(|k| k.common.key_id.as_deref() == Some(kid))
            .ok_or(OidcError::JwksKeyNotFound)?;

        // Verify signature + registered claims.
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&[iss.to_string()]);
        validation.set_required_spec_claims(&["exp", "iss"]);
        validation.leeway = 60;
        // `aud` is intentionally not validated: jsonwebtoken rejects tokens
        // carrying an `aud` when no allowed audience is configured, and Keycloak
        // sets `aud` to the requesting client (e.g. the portal), not the gateway.
        // See module docs for the trust boundary.
        validation.validate_aud = false;

        let key = DecodingKey::from_jwk(jwk).map_err(|_| OidcError::InvalidSignature)?;
        let data = decode::<VerifiedClaims>(token, &key, &validation).map_err(|e| {
            use jsonwebtoken::errors::ErrorKind;
            match e.kind() {
                ErrorKind::ExpiredSignature => OidcError::Expired,
                ErrorKind::InvalidSignature => OidcError::InvalidSignature,
                _ => OidcError::InvalidClaims,
            }
        })?;

        let provider_scope = if issuer.provider_name.is_empty() {
            "oidc"
        } else {
            issuer.provider_name.as_str()
        };
        let user_id = format!("sso:{provider_scope}:{}", data.claims.sub);

        Ok(OidcSubject {
            sub: data.claims.sub,
            issuer: iss.to_string(),
            user_id,
        })
    }
}

impl Default for OidcResourceServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    const TEST_KID: &str = "test-rsa-key";
    const TEST_N: &str = "k8Loq1BUjDtikff5A_prAy7-NWO3UvIvx7MjisnF0Emr4YmPOKnGIRoZ-zB8Mh1127WNYZBm9S6ooDUK--jB6ixzoQZDEH35d51QPQNy9OUt1qRwSCbSjGYzKzAYEFY2AY2Hd-zzSkQn9gnjRCIxWjXiDq-ipPY6wxhKjjQNezNoMjU_b5mSlZH3yQLh4uHYqKYvfAsQSziua5gY6su6hYWU38yUt1BlE8rnpAImojUnnKsg9MJCl1YYeqQMA6W7HIZrCo8F22TFsW-49e2usULNfn3kpj-R4AMuaXRAN-UaaRSS6lVL40a97acWHSAlgnvpJ4nkuCV3DSAW2qcSqQ";
    const TEST_E: &str = "AQAB";
    const TEST_ISSUER: &str = "https://idp.example.com/realms/master";
    const TEST_RSA_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCTwuirUFSMO2KR\n\
9/kD+msDLv41Y7dS8i/HsyOKycXQSavhiY84qcYhGhn7MHwyHXXbtY1hkGb1Lqig\n\
NQr76MHqLHOhBkMQffl3nVA9A3L05S3WpHBIJtKMZjMrMBgQVjYBjYd37PNKRCf2\n\
CeNEIjFaNeIOr6Kk9jrDGEqONA17M2gyNT9vmZKVkffJAuHi4diopi98CxBLOK5r\n\
mBjqy7qFhZTfzJS3UGUTyuekAiaiNSecqyD0wkKXVhh6pAwDpbschmsKjwXbZMWx\n\
b7j17a6xQs1+feSmP5HgAy5pdEA35RppFJLqVUvjRr3tpxYdICWCe+knieS4JXcN\n\
IBbapxKpAgMBAAECggEABYZjscgqqSWtTVzyzDHIX5GZwsBMQgc5PyPVH+LkiSHA\n\
EgpVNx6uAF3b+9b3xd3xIrp6o1vFZcSNXJQvKXUuDwYDetFjn5G+Srkwn19qJHsP\n\
SDfU1PXSqJpHroU5WR8IHO3AU30iKbQ7tEjxXQJUSxW5sqfhkn58ewAFBaUvndwj\n\
KtZh9nVv+H3LKtmJ58/M5TWSd5Fb7PiiRurrkbIRBhw/fDKKtjhHaqMG923LavZz\n\
okDraBcidPAYaFThXus0rIrq8keX5tfRmqUyW1YAVCareRvCndQ5aI7f0eA6411N\n\
ZpVIwbNC1djsxPD6g2Y4HYsaRnu1d6gR2glyvJSZUwKBgQDJtS3D/+bJkzbCSFZY\n\
F+g+6z97s0GIThd3s9PIMGl3lWrQ4V0pX4ezFm5FrMfLYQbRTX8aTesOFsRxufOy\n\
Yg2IfVo7alSDxHtUeJfgeFY29EKolVOjCsE/zktm4qCtEgl97U8U4OyaXQCp7do0\n\
b5ADvFKHjuq6m9R4m95qGxDLxwKBgQC7iIP6LFOfLVFElVZTOUWvJ6YmAYgidpFw\n\
bmjNZisG5vzF7qhdNlrieMwW+Zp7xQiqiqlEVt2f/iJ1+Tp9ESiq/uiLOWALrl4+\n\
WYKmwpuhF4XMcvyh3aN8jvIwpeaPe0sNKimJoma0cDQmS5D5P5Xtq0pcJ6QHBfTM\n\
tEtB38fODwKBgQCDjylXcjwb82m+1CGE+argBt30F5nBhnWl/GNAadsQRSNTM/po\n\
dsdyVkn8JdJ2Y0VoFGy3QmTyXoUoTAmXqn57LI9Cu3p+KxPpp3If1T0eQLiNbkAL\n\
0oLy0+G4LE5yM5Z/TN3Ml1ua3tgE/X7Zvn4nAZiuk9ejeOne9ILfn+GXlwKBgBRA\n\
7DAKtYVNead0kXwvhU0jdRhJthAyygZghkUYsbDvJYGjAt/+TNaEwVYB4yNW5la0\n\
3w8Yapsq8UHYhu6W+dNt8GOI8MySKm+Fb0zfW7uMNNEd4hcBPvTm41VJtZrtb++e\n\
DBpnRbxbGebA5olkyqZ+h2tohJiVlhi9qBsXNhcVAoGBALf4UjM9jgaiZhCjeg7C\n\
1+fX5bm8g0/hdxAN7r10ePAP/JPZBKtySYsNikcfjkNG0bcSQaWfRVz/A/xDvlNm\n\
ULpqkdDpgi5KInL1jH4XTy1rWw8uGIuqWgghzVRrEh15rpvSeelSxPhBEWnqwK6m\n\
/DafO1teTqM5bKMpCMRy3Ru6\n\
-----END PRIVATE KEY-----\n";

    fn jwks_json() -> serde_json::Value {
        serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "kid": TEST_KID,
                "use": "sig",
                "alg": "RS256",
                "n": TEST_N,
                "e": TEST_E
            }]
        })
    }

    fn sign_token(iss: &str, sub: &str, exp_offset: i64, tamper: bool) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(TEST_KID.to_string());
        let claims = serde_json::json!({
            "iss": iss,
            "sub": sub,
            "aud": "portal-client",
            "azp": "portal-client",
            "iat": chrono::Utc::now().timestamp(),
            "exp": chrono::Utc::now().timestamp() + exp_offset,
        });
        let key = EncodingKey::from_rsa_pem(TEST_RSA_PEM.as_bytes()).unwrap();
        let mut token = encode(&header, &claims, &key).unwrap();
        if tamper {
            // Flip a char in the signature segment to break it.
            let parts: Vec<&str> = token.split('.').collect();
            let sig = parts[2].to_string();
            let mut bytes = sig.into_bytes();
            bytes[0] = if bytes[0] == b'a' { b'b' } else { b'a' };
            token = format!("{}.{}.{}", parts[0], parts[1], String::from_utf8(bytes).unwrap());
        }
        token
    }

    /// Build a server with the test issuer trusted and its JWKS cached.
    fn test_server() -> OidcResourceServer {
        let server = OidcResourceServer::new();
        *server.issuers.write().unwrap() = vec![TrustedIssuer {
            issuer_url: TEST_ISSUER.to_string(),
            provider_name: "oidc".to_string(),
        }];
        let set: JwkSet = serde_json::from_value(jwks_json()).unwrap();
        server
            .jwks
            .write()
            .unwrap()
            .insert(TEST_ISSUER.to_string(), set);
        server
    }

    #[test]
    fn validates_well_signed_token_and_maps_subject() {
        let server = test_server();
        let token = sign_token(TEST_ISSUER, "dev01-subject-id", 3600, false);

        let subject = server.validate(&token).unwrap();
        assert_eq!(subject.sub, "dev01-subject-id");
        assert_eq!(subject.issuer, TEST_ISSUER);
        assert_eq!(subject.user_id, "sso:oidc:dev01-subject-id");
    }

    #[test]
    fn rejects_tampered_signature() {
        let server = test_server();
        let token = sign_token(TEST_ISSUER, "dev01-subject-id", 3600, true);

        match server.validate(&token) {
            Err(OidcError::InvalidSignature) => {}
            other => panic!("expected InvalidSignature, got {other:?}"),
        }
    }

    #[test]
    fn rejects_untrusted_issuer() {
        let server = test_server();
        let token = sign_token("https://evil.example.com/realms/other", "dev01", 3600, false);

        match server.validate(&token) {
            Err(OidcError::UntrustedIssuer) => {}
            other => panic!("expected UntrustedIssuer, got {other:?}"),
        }
    }

    #[test]
    fn rejects_expired_token() {
        let server = test_server();
        let token = sign_token(TEST_ISSUER, "dev01", -7200, false);

        match server.validate(&token) {
            Err(OidcError::Expired) => {}
            other => panic!("expected Expired, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_jwks_cache() {
        let server = OidcResourceServer::new();
        *server.issuers.write().unwrap() = vec![TrustedIssuer {
            issuer_url: TEST_ISSUER.to_string(),
            provider_name: "oidc".to_string(),
        }];
        let token = sign_token(TEST_ISSUER, "dev01", 3600, false);

        match server.validate(&token) {
            Err(OidcError::JwksNotReady) => {}
            other => panic!("expected JwksNotReady, got {other:?}"),
        }
    }
}
