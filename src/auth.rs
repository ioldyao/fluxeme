//! Permission constants and AuthCtx extractor for RBAC.
//!
//! Permission codes follow the format `resource:action` and are stored in
//! the `permissions` table, assigned via `role_permissions`.

use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::admin::AdminError;
use crate::domain::user::SessionInfo;
use crate::server::AppState;

/// Permission code constants used throughout the application.
pub mod perms {
    // System
    pub const SYSTEM_HEALTH: &str = "system:health";

    // User management
    pub const USER_CREATE: &str = "user:create";
    pub const USER_READ: &str = "user:read";
    pub const USER_UPDATE: &str = "user:update";
    pub const USER_DELETE: &str = "user:delete";

    // API Keys (self-service)
    pub const APIKEY_SELF_READ: &str = "apikey:self:read";
    pub const APIKEY_SELF_CREATE: &str = "apikey:self:create";
    pub const APIKEY_SELF_UPDATE: &str = "apikey:self:update";
    pub const APIKEY_SELF_DELETE: &str = "apikey:self:delete";

    // API Keys (admin)
    pub const APIKEY_READ: &str = "apikey:read";
    pub const APIKEY_CREATE: &str = "apikey:create";
    pub const APIKEY_UPDATE: &str = "apikey:update";
    pub const APIKEY_DELETE: &str = "apikey:delete";

    // Channels
    pub const CHANNEL_CREATE: &str = "channel:create";
    pub const CHANNEL_READ: &str = "channel:read";
    pub const CHANNEL_UPDATE: &str = "channel:update";
    pub const CHANNEL_DELETE: &str = "channel:delete";

    // Models
    pub const MODEL_CREATE: &str = "model:create";
    pub const MODEL_READ: &str = "model:read";
    pub const MODEL_UPDATE: &str = "model:update";
    pub const MODEL_DELETE: &str = "model:delete";
    pub const MODEL_PUBLISH: &str = "model:publish";

    // Routing
    pub const ROUTING_CREATE: &str = "routing:create";
    pub const ROUTING_READ: &str = "routing:read";
    pub const ROUTING_UPDATE: &str = "routing:update";
    pub const ROUTING_DELETE: &str = "routing:delete";

    // Usage logs
    pub const USAGE_READ: &str = "usage:read";
    pub const USAGE_EXPORT: &str = "usage:export";

    // Billing
    pub const BILLING_VIEW: &str = "billing:view";
    pub const BILLING_EXPORT: &str = "billing:export";
    pub const BILLING_MANAGE: &str = "billing:manage";

    // Wallet
    pub const WALLET_READ: &str = "wallet:read";
    pub const WALLET_RECHARGE: &str = "wallet:recharge";
    pub const WALLET_MANAGE: &str = "wallet:manage";

    // Recharge keys
    pub const RECHARGE_CREATE: &str = "recharge:create";
    pub const RECHARGE_READ: &str = "recharge:read";
    pub const RECHARGE_REVOKE: &str = "recharge:revoke";

    // Settings
    pub const SETTINGS_READ: &str = "settings:read";
    pub const SETTINGS_UPDATE: &str = "settings:update";
    pub const SETTINGS_GATEWAY: &str = "settings:gateway";

    // Dashboard
    pub const DASHBOARD_VIEW: &str = "dashboard:view";

    // Exchange rates
    pub const EXCHANGE_READ: &str = "exchange:read";
    pub const EXCHANGE_UPDATE: &str = "exchange:update";

    // Roles & Permissions
    pub const ROLE_READ: &str = "role:read";
    pub const ROLE_UPDATE: &str = "role:update";
    pub const PERMISSION_READ: &str = "permission:read";

    // Marketplace / Subscriptions
    pub const SUBSCRIPTION_READ: &str = "subscription:read";
    pub const SUBSCRIPTION_MANAGE: &str = "subscription:manage";
}

/// Extractor that validates a JWT session from the `Authorization` header.
///
/// Use as a handler parameter to replace manual `require_session` / `require_admin` calls.
/// The session is automatically validated (token, version, rate-limit) on extraction.
/// Call `require_perm()` to check granular permissions.
pub struct AuthCtx {
    pub session: SessionInfo,
}

impl AuthCtx {
    /// Check that the authenticated session has a specific permission.
    ///
    /// # Errors
    /// Returns `AdminError::Forbidden` if the permission is not present.
    pub fn require_perm(&self, perm: &str) -> Result<(), AdminError> {
        if self.session.permissions.iter().any(|p| p == perm) {
            Ok(())
        } else {
            Err(AdminError::forbidden("Insufficient permissions"))
        }
    }
}

fn extract_token(parts: &Parts) -> Result<String, AdminError> {
    parts
        .headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .ok_or_else(|| AdminError::unauthorized("Missing or invalid auth token"))
}

impl FromRequestParts<Arc<AppState>> for AuthCtx {
    type Rejection = AdminError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_token(parts)?;
        let mut session = state.admin.decode_token(&token)?;

        // Verify token_version against DB (session revocation enforcement)
        let db_user = state
            .db
            .get_user(&session.user_id)
            .await
            .map_err(|e| AdminError::internal(e.to_string()))?
            .ok_or_else(|| AdminError::unauthorized("User not found"))?;
        if db_user.token_version != session.token_version {
            return Err(AdminError::unauthorized(
                "Session has been revoked. Please log in again.",
            ));
        }

        // Reload permissions from DB so old tokens (issued before RBAC) and
        // permission changes take effect immediately without requiring re-login.
        if let Ok(perms) = state.db.get_role_permissions(&session.role).await {
            session.permissions = perms;
        }

        // Rate limit: 300 requests/minute per session
        state
            .rate_limiter
            .check_rpm(&format!("admin:{}", session.user_id), 300)
            .map_err(|_| AdminError::too_many_requests("Too many requests. Try again later."))?;

        Ok(AuthCtx { session })
    }
}
