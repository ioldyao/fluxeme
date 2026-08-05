use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A team — a group of users sharing resources (API keys, wallet, usage,
/// routing rules). Team is a first-class ownership/billing entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    /// The user who owns the team. References users.id.
    pub owner_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Membership of a user within a team.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub team_id: String,
    pub user_id: String,
    /// "owner" | "admin" | "member"
    pub role: String,
    pub joined_at: DateTime<Utc>,
}

/// Team member roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TeamRole {
    Owner,
    Admin,
    Member,
}

impl TeamRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            TeamRole::Owner => "owner",
            TeamRole::Admin => "admin",
            TeamRole::Member => "member",
        }
    }

    pub fn from_str(s: &str) -> Option<TeamRole> {
        match s {
            "owner" => Some(TeamRole::Owner),
            "admin" => Some(TeamRole::Admin),
            "member" => Some(TeamRole::Member),
            _ => None,
        }
    }
}

impl std::fmt::Display for TeamRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Resolved team context attached to an authenticated request when a team is active.
/// Both fields are None for personal (non-team) accounts.
#[derive(Debug, Clone, Default)]
pub struct TeamContext {
    pub team_id: Option<String>,
    pub role_in_team: Option<TeamRole>,
}

impl TeamContext {
    pub fn none() -> Self {
        Self::default()
    }
}
