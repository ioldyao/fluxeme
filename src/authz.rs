use std::sync::Arc;

use casbin::{CoreApi, DefaultModel, Enforcer, MemoryAdapter, MgmtApi};
use tokio::sync::RwLock;

use crate::db::Database;
use crate::domain::team::TeamMember;

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
    ("admin", "admin:billing-groups"),
    ("admin", "admin:recharge-keys"),
    ("admin", "admin:health"),
    ("admin", "admin:settings"),
    ("admin", "admin:gateway"),
    ("admin", "admin:policies"),
    ("admin", "admin:announcements"),
    ("admin", "admin:teams"),
    ("admin", "admin:skillhub"),
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

    /// Seed default policies into the DB if missing.
    /// Idempotent — safe to run on every startup.
    pub async fn seed_defaults(&self, db: &Database) -> Result<(), Box<dyn std::error::Error>> {
        let existing = db.casbin_list_policies().await?;
        let existing_set: std::collections::HashSet<(String, String)> = existing
            .iter()
            .map(|(_ptype, v0, v1, _v2, _v3, _v4, _v5)| (v0.clone(), v1.clone()))
            .collect();

        for (role, perm) in DEFAULT_POLICIES {
            let key = (role.to_string(), perm.to_string());
            if !existing_set.contains(&key) {
                db.casbin_add_policy("p", role, perm, "", "", "", "")
                    .await?;
                tracing::info!("Seeded missing Casbin policy: {role} -> {perm}");
            }
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
            let _ = e.remove_policy(p.clone()).await;
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

/// Team-role → team-permission mapping.
///
/// These permissions are bound to a team via the domain dimension in
/// `casbin_team_model.conf`: `g(<user_id>, <perm>, <team_id>)`.
pub const TEAM_ROLE_PERMISSIONS: &[(&str, &[&str])] = &[
    ("owner", &["team:*"]),
    (
        "admin",
        &[
            "team:member:manage",
            "team:key:manage",
            "team:wallet:manage",
            "team:rule:manage",
            "team:usage:view",
            "team:billing:view",
        ],
    ),
    (
        "member",
        &[
            "team:key:use",
            "team:wallet:view",
            "team:rule:view",
            "team:usage:view",
            "team:billing:view",
        ],
    ),
];

/// Domain-aware RBAC enforcer for team-scoped permissions.
///
/// Uses a separate model (`config/casbin_team_model.conf`) and a separate
/// in-memory enforcer so the global admin `AuthzModule` stays untouched.
/// Team role assignments are stored in `casbin_policies` as `g` rows:
/// `g(<user_id>, <perm>, <team_id>)`.
pub struct TeamAuthzModule {
    enforcer: Arc<RwLock<Enforcer>>,
}

impl TeamAuthzModule {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let m = DefaultModel::from_file("config/casbin_team_model.conf").await?;
        let e = Enforcer::new(m, MemoryAdapter::default()).await?;
        Ok(Self {
            enforcer: Arc::new(RwLock::new(e)),
        })
    }

    /// Clear all team role bindings for a team and rebuild them from the
    /// given members' roles.
    ///
    /// Writes the standard RBAC-with-domains shape:
    /// - `g(user_id, role, team_id)` — user's role within the team
    /// - `p(role, team_id, perm)` — role's permission within the team
    /// `enforce` then matches `g(r.sub, p.sub, r.dom) && r.dom == p.dom && keyMatch(r.obj, p.obj)`.
    pub async fn sync_team_roles(&self, team_id: &str, members: &[TeamMember]) {
        let mut e = self.enforcer.write().await;

        // Clear existing g rows for this team: g = [user_id, role, team_id]
        let existing_g = e.get_grouping_policy();
        for row in &existing_g {
            if row.len() == 3 && row[2] == team_id {
                let _ = e.remove_grouping_policy(row.clone()).await;
            }
        }

        // Clear existing p rows for this team: p = [role, team_id, perm]
        let existing_p = e.get_policy();
        for row in &existing_p {
            if row.len() == 3 && row[1] == team_id {
                let _ = e.remove_policy(row.clone()).await;
            }
        }

        // Rebuild g (user → role) and p (role → perms) from members.
        let mut roles_in_team: Vec<&str> = Vec::new();
        for member in members {
            let _ = e
                .add_grouping_policy(vec![
                    member.user_id.clone(),
                    member.role.clone(),
                    team_id.to_string(),
                ])
                .await;
            if !roles_in_team.contains(&member.role.as_str()) {
                roles_in_team.push(member.role.as_str());
            }
        }
        for role in roles_in_team {
            let perms = TEAM_ROLE_PERMISSIONS
                .iter()
                .find(|(r, _)| *r == role)
                .map(|(_, perms)| *perms)
                .unwrap_or(&[]);
            for perm in perms {
                let _ = e
                    .add_policy(vec![
                        role.to_string(),
                        team_id.to_string(),
                        perm.to_string(),
                    ])
                    .await;
            }
        }
    }

    /// Check whether `user_id` has `perm` within `team_id`.
    pub async fn enforce(&self, team_id: &str, user_id: &str, perm: &str) -> bool {
        let e = self.enforcer.read().await;
        e.enforce((user_id.to_owned(), team_id.to_owned(), perm.to_owned()))
            .unwrap_or(false)
    }

    /// Rebuild all team role bindings from the database. Called on startup so
    /// team Casbin permissions survive a restart (the enforcer is in-memory).
    pub async fn reload_all(&self, db: &Database) {
        let members = match db.all_team_members().await {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("Team authz reload: failed to load members: {}", e);
                return;
            }
        };
        // Group members by team_id, then sync each team's roles.
        let mut by_team: std::collections::HashMap<String, Vec<TeamMember>> =
            std::collections::HashMap::new();
        for m in &members {
            by_team
                .entry(m.team_id.clone())
                .or_default()
                .push(m.clone());
        }
        for (team_id, team_members) in &by_team {
            self.sync_team_roles(team_id, team_members).await;
        }
        tracing::info!("Team authz reloaded roles for {} teams", by_team.len());
    }
}

impl Clone for TeamAuthzModule {
    fn clone(&self) -> Self {
        Self {
            enforcer: self.enforcer.clone(),
        }
    }
}

#[cfg(test)]
mod team_authz_tests {
    use super::{TeamAuthzModule, TEAM_ROLE_PERMISSIONS};
    use crate::domain::team::TeamMember;

    fn member(user_id: &str, role: &str) -> TeamMember {
        TeamMember {
            team_id: "team-1".to_string(),
            user_id: user_id.to_string(),
            role: role.to_string(),
            joined_at: chrono::Utc::now(),
        }
    }

    async fn setup() -> TeamAuthzModule {
        let m = TeamAuthzModule::new().await.expect("team authz init");
        let members = vec![
            member("owner-u", "owner"),
            member("admin-u", "admin"),
            member("member-u", "member"),
        ];
        m.sync_team_roles("team-1", &members).await;
        m
    }

    #[tokio::test]
    async fn owner_has_all_permissions() {
        let m = setup().await;
        // Owner gets team:* which covers all team perms via keyMatch.
        assert!(m.enforce("team-1", "owner-u", "team:key:manage").await);
        assert!(m.enforce("team-1", "owner-u", "team:member:manage").await);
        assert!(m.enforce("team-1", "owner-u", "team:wallet:manage").await);
        assert!(m.enforce("team-1", "owner-u", "team:rule:manage").await);
    }

    #[tokio::test]
    async fn admin_has_manage_permissions_but_not_all() {
        let m = setup().await;
        assert!(m.enforce("team-1", "admin-u", "team:key:manage").await);
        assert!(m.enforce("team-1", "admin-u", "team:member:manage").await);
        assert!(m.enforce("team-1", "admin-u", "team:wallet:manage").await);
        // admin does NOT have owner-level team:* — only explicit perms.
        assert!(!m.enforce("team-1", "admin-u", "team:delete-team").await);
    }

    #[tokio::test]
    async fn member_only_has_view_permissions() {
        let m = setup().await;
        assert!(m.enforce("team-1", "member-u", "team:wallet:view").await);
        assert!(!m.enforce("team-1", "member-u", "team:key:manage").await);
        assert!(!m.enforce("team-1", "member-u", "team:wallet:manage").await);
    }

    #[tokio::test]
    async fn permissions_are_team_scoped() {
        let m = setup().await;
        // admin-u has perms in team-1, but not in another team.
        assert!(!m.enforce("team-other", "admin-u", "team:key:manage").await);
    }

    #[test]
    fn role_permissions_map_is_valid() {
        // Every role referenced must have a permission list.
        for (role, _) in TEAM_ROLE_PERMISSIONS {
            assert!(["owner", "admin", "member"].contains(role));
        }
        // owner maps to team:* so all perms are covered.
        assert_eq!(TEAM_ROLE_PERMISSIONS[0].0, "owner");
    }
}
