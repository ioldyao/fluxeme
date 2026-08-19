use std::fmt;

/// SkillHub 子系统错误。组合根负责映射到 HTTP 错误形态。
#[derive(Debug)]
pub enum SkillHubError {
    NotFound(String),
    Invalid(String),
    Conflict(String),
    Storage(String),
    Db(String),
    Internal(String),
}

impl fmt::Display for SkillHubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkillHubError::NotFound(m) => write!(f, "not found: {m}"),
            SkillHubError::Invalid(m) => write!(f, "invalid: {m}"),
            SkillHubError::Conflict(m) => write!(f, "conflict: {m}"),
            SkillHubError::Storage(m) => write!(f, "storage: {m}"),
            SkillHubError::Db(m) => write!(f, "db: {m}"),
            SkillHubError::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl std::error::Error for SkillHubError {}

impl From<sqlx_core::Error> for SkillHubError {
    fn from(e: sqlx_core::Error) -> Self {
        SkillHubError::Db(e.to_string())
    }
}
