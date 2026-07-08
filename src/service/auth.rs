use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use axum::http::HeaderMap;

use crate::db::Database;
use crate::domain::user::{ApiKey, AuthResult, User};

pub struct AuthService {
    db: Arc<Database>,
    users: RwLock<HashMap<String, User>>,
    api_keys: RwLock<HashMap<String, (User, ApiKey)>>,
}

impl AuthService {
    pub fn new(db: Arc<Database>) -> Self {
        let svc = Self {
            db,
            users: RwLock::new(HashMap::new()),
            api_keys: RwLock::new(HashMap::new()),
        };
        svc.reload();
        svc
    }

    /// Reload all caches from database. Called after admin modifies users/keys.
    pub fn reload(&self) {
        match self.db.all_api_keys() {
            Ok(pairs) => {
                let mut map = HashMap::new();
                for (user, key) in &pairs {
                    map.insert(key.key.clone(), (user.clone(), key.clone()));
                }
                *self.api_keys.write().unwrap() = map;
            }
            Err(e) => tracing::error!("Failed to load API keys: {}", e),
        }

        match self.db.list_users() {
            Ok(users) => {
                let map: HashMap<_, _> = users.into_iter().map(|u| (u.id.clone(), u)).collect();
                *self.users.write().unwrap() = map;
            }
            Err(e) => tracing::error!("Failed to load users: {}", e),
        }
    }

    pub fn authenticate(&self, headers: &HeaderMap) -> Result<AuthResult, AuthError> {
        let key = self.extract_key(headers).ok_or_else(|| {
            AuthError("Missing or invalid API key".into())
        })?;

        // Check user API keys
        {
            let ak = self.api_keys.read().unwrap();
            if let Some((user, api_key)) = ak.get(&key) {
                if !api_key.enabled {
                    return Err(AuthError("API key is disabled".into()));
                }
                if let Some(ref expires) = api_key.expires_at {
                    if let Ok(exp) = chrono::DateTime::parse_from_rfc3339(expires) {
                        if chrono::Utc::now() > exp {
                            return Err(AuthError("API key has expired".into()));
                        }
                    }
                }
                return Ok(AuthResult {
                    user_id: user.id.clone(),
                    user_name: user.name.clone(),
                    rate_limits: user.rate_limits.as_ref().map(|rl| {
                        (rl.rpm.unwrap_or(u64::MAX), rl.tpm.unwrap_or(u64::MAX))
                    }),
                });
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
