use std::sync::Arc;

use casbin::{CoreApi, DefaultModel, Enforcer, MemoryAdapter, MgmtApi};
use tokio::sync::RwLock;

/// Wraps a Casbin enforcer behind an RwLock for thread-safe access.
///
/// Uses Casbin's in-memory DefaultAdapter. Policies are seeded on startup
/// and managed at runtime via the admin API.
pub struct AuthzModule {
    enforcer: Arc<RwLock<Enforcer>>,
}

impl AuthzModule {
    /// Initialize the Casbin enforcer with the RBAC model.
    ///
    /// Seeds default policies:
    /// - `admin` role → `admin:*` (all admin permissions via keyMatch wildcard)
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let m = DefaultModel::from_file("config/casbin_model.conf").await?;
        let mut e = Enforcer::new(m, MemoryAdapter::default()).await?;
        e.enable_auto_save(true);

        // Seed: admin role gets all admin:* permissions via the wildcard
        if !e.has_policy(vec!["admin".to_owned(), "admin:*".to_owned()]) {
            e.add_policy(vec!["admin".to_owned(), "admin:*".to_owned()])
                .await?;
            tracing::info!("Seeded default Casbin policy: admin -> admin:*");
        }

        let enforcer = e;

        Ok(Self {
            enforcer: Arc::new(RwLock::new(enforcer)),
        })
    }

    /// Check if a role has a given permission.
    ///
    /// Returns `true` if the role (directly or via role inheritance) is allowed.
    pub async fn enforce(&self, role: &str, permission: &str) -> bool {
        let guard = self.enforcer.read().await;
        guard
            .enforce((role.to_owned(), permission.to_owned()))
            .unwrap_or(false)
    }
}

impl Clone for AuthzModule {
    fn clone(&self) -> Self {
        Self {
            enforcer: self.enforcer.clone(),
        }
    }
}
