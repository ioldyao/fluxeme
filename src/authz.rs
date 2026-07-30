use std::sync::Arc;

use casbin::{CoreApi, DefaultModel, Enforcer, MemoryAdapter, MgmtApi};
use tokio::sync::RwLock;

use crate::db::Database;

/// Default policies seeded on first run when the casbin_policies table is empty.
const DEFAULT_POLICIES: &[(&str, &str)] = &[
    ("admin", "admin:dashboard"),
    ("admin", "admin:users"),
    ("admin", "admin:channels"),
    ("admin", "admin:models"),
    ("admin", "admin:model-pricing"),
    ("admin", "admin:rules"),
    ("admin", "admin:moderation"),
    ("admin", "admin:usage"),
    ("admin", "admin:bills"),
    ("admin", "admin:recharge-keys"),
    ("admin", "admin:health"),
    ("admin", "admin:settings"),
    ("admin", "admin:gateway"),
    ("admin", "admin:policies"),
];

/// Wraps a Casbin enforcer behind an RwLock for thread-safe access.
///
/// Uses Casbin's in-memory MemoryAdapter. Policies are loaded from the
/// database on startup and can be reloaded at runtime.
pub struct AuthzModule {
    enforcer: Arc<RwLock<Enforcer>>,
}

impl AuthzModule {
    /// Initialize the Casbin enforcer with the RBAC model.
    /// No policies are seeded — call `seed_defaults` or `reload` to populate.
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let m = DefaultModel::from_file("config/casbin_model.conf").await?;
        let mut e = Enforcer::new(m, MemoryAdapter::default()).await?;
        e.enable_auto_save(true);

        Ok(Self {
            enforcer: Arc::new(RwLock::new(e)),
        })
    }

    /// Seed default policies into the DB if the table is empty.
    pub async fn seed_defaults(&self, db: &Database) -> Result<(), Box<dyn std::error::Error>> {
        let existing = db.casbin_list_policies().await?;
        if !existing.is_empty() {
            tracing::info!("Casbin policies already exist, skipping seed");
            return Ok(());
        }

        for (role, perm) in DEFAULT_POLICIES {
            db.casbin_add_policy("p", role, perm, "", "", "", "")
                .await?;
            tracing::info!("Seeded Casbin policy: {role} -> {perm}");
        }

        self.reload(db).await?;
        Ok(())
    }

    /// Reload all policies from the database into the enforcer.
    pub async fn reload(&self, db: &Database) -> Result<(), Box<dyn std::error::Error>> {
        let rows = db.casbin_list_policies().await?;
        let mut e = self.enforcer.write().await;

        // Clear existing policies
        let current = e.get_policy();
        for p in &current {
            let _ = e.remove_policy(p.clone());
        }

        // Load from DB
        for (_ptype, v0, v1, _v2, _v3, _v4, _v5) in &rows {
            let _ = e.add_policy(vec![v0.clone(), v1.clone()]).await;
        }

        tracing::info!("Reloaded {} Casbin policies", rows.len());
        Ok(())
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
