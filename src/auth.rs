use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::admin::AdminError;
use crate::domain::user::SessionInfo;
use crate::server::AppState;

/// Authenticated request context extracted from the JWT Bearer token.
///
/// Implements `FromRequestParts` so handlers can declare `auth: AuthCtx` as
/// a parameter instead of manually calling `require_session()`.
///
/// Examples:
/// ```ignore
/// // Handler that requires any authenticated session
/// async fn my_handler(auth: AuthCtx) -> Result<Json<...>, AdminError> { ... }
///
/// // Handler that additionally requires admin role
/// async fn admin_handler(auth: AuthCtx) -> Result<Json<...>, AdminError> {
///     auth.require_admin()?;
///     ...
/// }
/// ```
#[derive(Debug, Clone)]
pub struct AuthCtx {
    pub session: SessionInfo,
}

impl AuthCtx {
    /// Require the authenticated user to have the admin role.
    /// Returns 403 Forbidden if the user is not an admin.
    pub fn require_admin(&self) -> Result<(), AdminError> {
        if self.session.role != "admin" {
            return Err(AdminError::forbidden("Admin access required"));
        }
        Ok(())
    }

    /// Require the authenticated user to have a specific role.
    /// Returns 403 Forbidden if the user does not match.
    pub fn require_role(&self, role: &str) -> Result<(), AdminError> {
        if self.session.role != role {
            return Err(AdminError::forbidden("Insufficient permissions"));
        }
        Ok(())
    }
}

impl FromRequestParts<Arc<AppState>> for AuthCtx {
    type Rejection = AdminError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let session = state.admin.authenticate(&parts.headers).await?;
        Ok(AuthCtx { session })
    }
}
