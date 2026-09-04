// ── Shared request helpers (moved from server/handlers/common.rs) ──

use std::net::SocketAddr;

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use serde_json::Value;

use crate::cache::GateStatus;
use crate::server::AppState;
use rust_decimal::Decimal;

// ── Error type ────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Debug)]
pub enum GatewayError {
    Auth(String),
    RateLimit(String),
    Route(String),
    BadRequest(String),
    Upstream(String),
    Internal(String),
    PaymentRequired(String),
    /// Model exists but is currently unavailable (circuit open, no healthy
    /// endpoint) — mapped to 503 so clients retry instead of concluding the
    /// model doesn't exist.
    ServiceUnavailable(String),
}

impl GatewayError {
    fn status(&self) -> StatusCode {
        match self {
            Self::Auth(_) => StatusCode::UNAUTHORIZED,
            Self::RateLimit(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Route(_) => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Upstream(_) => StatusCode::BAD_GATEWAY,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::PaymentRequired(_) => StatusCode::PAYMENT_REQUIRED,
            Self::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Auth(m)
            | Self::RateLimit(m)
            | Self::Route(m)
            | Self::BadRequest(m)
            | Self::Upstream(m)
            | Self::PaymentRequired(m)
            | Self::ServiceUnavailable(m) => m,
            Self::Internal(_) => "Internal server error",
        }
    }
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let status = self.status();
        let message = self.message();
        // Safety net: any 5xx (upstream 502/503, internal 500, timeout) that
        // reaches the response layer gets an error log with the real message,
        // even if a specific handler branch forgot to log it. 4xx are normal
        // business results and are intentionally not logged here.
        if status.as_u16() >= 500 {
            tracing::error!(%status, error = %message, "Gateway request failed");
        }
        let body = serde_json::json!({
            "error": {
                "message": message,
                "type": "gateway_error",
            }
        });
        (status, Json(body)).into_response()
    }
}

impl From<crate::service::auth::AuthError> for GatewayError {
    fn from(e: crate::service::auth::AuthError) -> Self {
        Self::Auth(e.0)
    }
}

impl From<crate::service::routing::RouteError> for GatewayError {
    fn from(e: crate::service::routing::RouteError) -> Self {
        use crate::service::routing::RouteErrorKind;
        match e.kind {
            // A model that exists but is currently unhealthy is *unavailable*
            // (503), not missing (404). 404 makes clients like Claude Code
            // conclude the model doesn't exist; 503 lets them retry.
            RouteErrorKind::NotFound => Self::Route(e.message),
            RouteErrorKind::Unavailable => Self::ServiceUnavailable(e.message),
        }
    }
}

impl From<crate::ratelimit::RateLimitError> for GatewayError {
    fn from(e: crate::ratelimit::RateLimitError) -> Self {
        match e {
            crate::ratelimit::RateLimitError::Exceeded(message) => Self::RateLimit(message),
            crate::ratelimit::RateLimitError::Unavailable(message) => Self::Internal(message),
        }
    }
}

impl From<crate::provider::ProviderError> for GatewayError {
    fn from(e: crate::provider::ProviderError) -> Self {
        Self::Upstream(e.0)
    }
}

impl From<crate::service::moderation::FilterBlocked> for GatewayError {
    fn from(e: crate::service::moderation::FilterBlocked) -> Self {
        Self::BadRequest(e.0)
    }
}

// ── Request helpers ───────────────────────────────────────────────

pub(crate) fn authorize_effective_model(
    user: &crate::domain::user::AuthResult,
    resolved_model: &str,
) -> Result<(), GatewayError> {
    if user.key_kind == "platform" {
        if !user
            .scopes
            .as_ref()
            .is_some_and(|scopes| scopes.iter().any(|scope| scope == "model"))
        {
            return Err(GatewayError::Auth(
                "API key is not authorized for model access".into(),
            ));
        }
    } else if let Some(ref scopes) = user.scopes {
        if !scopes.iter().any(|scope| scope == "model") {
            return Err(GatewayError::Auth(
                "API key is not authorized for model access".into(),
            ));
        }
    }
    if let Some(ref allowed) = user.allowed_models {
        if !allowed.iter().any(|model| model == resolved_model) {
            return Err(GatewayError::Auth(
                "Model not allowed for this API key".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn trim_model(body: &mut Value) -> Result<String, GatewayError> {
    let model_val = body["model"].clone();
    let s = model_val
        .as_str()
        .ok_or_else(|| GatewayError::BadRequest("Missing 'model' field".into()))?
        .trim()
        .to_string();
    if s.is_empty() {
        return Err(GatewayError::BadRequest("'model' field is empty".into()));
    }
    body["model"] = Value::String(s.clone());
    Ok(s)
}

/// Move inline `role: "system"` messages to the top-level Anthropic `system`
/// field, and normalize Claude-Code-style bodies into the plainest form that
/// strict upstreams accept:
///   - `system` may be a string OR a content-block array → collapse to string
///   - `metadata` (always a JSON object from Claude Code) → drop it; some
///     upstreams (e.g. aiionly's Java gateway) declare it as a non-object type
///     and 500 with a Jackson parse error on the object form
///   - a `content` array containing only text blocks → collapse to string
///
/// Claude Code occasionally sends system prompts as inline messages with
/// role="system", which SGLang's /v1/messages rejects (only "user" and
/// "assistant" are allowed in the messages array).
pub(crate) fn normalize_messages_body(body: &mut Value) {
    // Top-level `system`: string or content-block array → plain string.
    if let Some(sys) = body.get("system").cloned() {
        let text = match &sys {
            Value::String(s) => Some(s.clone()),
            Value::Array(blocks) => {
                let mut t = String::new();
                for b in blocks {
                    if b.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(s) = b.get("text").and_then(|v| v.as_str()) {
                            if !t.is_empty() {
                                t.push('\n');
                            }
                            t.push_str(s);
                        }
                    }
                }
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            }
            _ => None,
        };
        match text {
            Some(t) => body["system"] = Value::String(t),
            None => {
                if let Some(obj) = body.as_object_mut() {
                    obj.remove("system");
                }
            }
        }
    }

    // `metadata` is always a JSON object from Claude Code. Some upstreams
    // declare it as a String/other type and fail to parse the object form.
    if let Some(obj) = body.as_object_mut() {
        obj.remove("metadata");
    }

    // Collapse text-only content arrays to a plain string. Strict upstreams
    // that declare `content` as String reject `[{type:"text",...}]` arrays.
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            let text_only = matches!(
                msg.get("content"),
                Some(Value::Array(blocks))
                    if !blocks.is_empty()
                        && blocks.iter().all(|b| b.get("type").and_then(|v| v.as_str())
                            == Some("text"))
            );
            if text_only {
                if let Some(Value::Array(blocks)) = msg.get("content").cloned() {
                    let mut t = String::new();
                    for b in &blocks {
                        if let Some(s) = b.get("text").and_then(|v| v.as_str()) {
                            if !t.is_empty() {
                                t.push('\n');
                            }
                            t.push_str(s);
                        }
                    }
                    msg["content"] = Value::String(t);
                }
            }
        }
    }

    let existing_system = body
        .get("system")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return;
    };

    // Collect inline system messages into a single string
    let mut system_text = String::new();
    let mut filtered = Vec::new();

    for msg in messages.drain(..) {
        if msg.get("role").and_then(|r| r.as_str()) == Some("system") {
            if let Some(content) = msg.get("content") {
                match content {
                    Value::String(s) => {
                        if !system_text.is_empty() {
                            system_text.push('\n');
                        }
                        system_text.push_str(s);
                    }
                    Value::Array(blocks) => {
                        for block in blocks {
                            if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                                if !system_text.is_empty() {
                                    system_text.push('\n');
                                }
                                system_text.push_str(t);
                            }
                        }
                    }
                    _ => {}
                }
            }
        } else {
            filtered.push(msg);
        }
    }

    *messages = filtered;

    // Merge extracted inline system text with any pre-existing top-level system
    if !system_text.is_empty() {
        let merged = if existing_system.is_empty() {
            system_text
        } else {
            format!("{}\n{}", system_text, existing_system)
        };
        body["system"] = Value::String(merged);
    }
}

/// Check whether the user's wallet balance is sufficient for this request.
///
/// The wallet account to check. A request charges either the personal user
/// wallet or a team wallet, depending on its team context.
#[allow(dead_code)]
enum WalletAccount<'a> {
    User(&'a str),
    Team(&'a str),
}

/// Check Redis gate status first. PostgreSQL is used only for a cold read when
/// Redis is reachable but has no cached status; Redis errors are propagated.
#[allow(dead_code)]
async fn check_wallet_balance(
    state: &AppState,
    account: WalletAccount<'_>,
) -> Result<(), GatewayError> {
    let key = match &account {
        WalletAccount::User(id) => (*id).to_string(),
        WalletAccount::Team(id) => format!("team:{}", id),
    };
    match state.cache.get_gate_status(&key).await {
        Ok(Some(GateStatus::Blocked)) => {
            return Err(GatewayError::PaymentRequired("Insufficient balance".into()));
        }
        Ok(Some(_)) => return Ok(()), // ok or low — pass through
        Ok(None) => {}                // Redis is healthy; perform a cold read below.
        Err(e) => {
            return Err(GatewayError::Internal(format!(
                "Redis gate status unavailable: {e}"
            )));
        }
    }
    // Cold-start fallback — read from PostgreSQL when Redis has no status.
    let (balance, frozen) = match &account {
        WalletAccount::User(id) => state
            .db
            .get_wallet_balance(id)
            .await
            .map_err(|e| GatewayError::Internal(e.0))?,
        WalletAccount::Team(id) => {
            let (b, f) = state
                .db
                .get_team_wallet(id)
                .await
                .map_err(|e| GatewayError::Internal(e.0))?
                .unwrap_or((0.0, 0.0));
            (
                Decimal::try_from(b).unwrap_or(Decimal::ZERO),
                Decimal::try_from(f).unwrap_or(Decimal::ZERO),
            )
        }
    };
    if balance - frozen <= Decimal::ZERO {
        return Err(GatewayError::PaymentRequired("Insufficient balance".into()));
    }
    Ok(())
}

// ── Reasoning normalization ───────────────────────────────────────

/// Non-standard reasoning field names from various providers to normalize.
pub(crate) const REASONING_ALIASES: &[&str] = &["reasoning", "thinking", "thinking_content"];

pub(crate) fn rename_to_reasoning_content(obj: &mut serde_json::Map<String, Value>) {
    if obj.contains_key("reasoning_content") {
        return;
    }
    for &alias in REASONING_ALIASES {
        if let Some(val) = obj.remove(alias) {
            obj.insert("reasoning_content".into(), val);
            return;
        }
    }
}

pub(crate) fn normalize_reasoning_inner(val: &mut Value) {
    if let Some(choices) = val.get_mut("choices").and_then(|c| c.as_array_mut()) {
        for choice in choices.iter_mut() {
            if let Some(msg) = choice.get_mut("message").and_then(|m| m.as_object_mut()) {
                rename_to_reasoning_content(msg);
            }
            if let Some(delta) = choice.get_mut("delta").and_then(|m| m.as_object_mut()) {
                rename_to_reasoning_content(delta);
            }
        }
    }
}

pub(crate) fn normalize_sse_reasoning(data: &str) -> String {
    let mut out = String::with_capacity(data.len());
    for line in data.lines() {
        let trimmed = line.trim();
        if let Some(json_str) = trimmed.strip_prefix("data: ") {
            if json_str.trim() == "[DONE]" {
                out.push_str(line);
                out.push('\n');
                continue;
            }
            if let Ok(mut val) = serde_json::from_str::<Value>(json_str) {
                normalize_reasoning_inner(&mut val);
                let indent = &line[..line.len() - trimmed.len()];
                out.push_str(indent);
                out.push_str("data: ");
                out.push_str(&serde_json::to_string(&val).unwrap_or_default());
                out.push('\n');
            } else {
                out.push_str(line);
                out.push('\n');
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

pub(crate) fn extract_client_ip(_headers: &HeaderMap, addr: SocketAddr) -> String {
    addr.ip().to_string()
}

#[cfg(test)]
mod normalize_tests {
    use serde_json::json;

    #[test]
    fn trims_whitespace_from_model() {
        let mut body = json!({"model": "  deepseek-v4-flash  ", "messages": []});
        let result = super::trim_model(&mut body);
        assert_eq!(result.unwrap(), "deepseek-v4-flash");
        assert_eq!(body["model"], "deepseek-v4-flash");
    }

    #[test]
    fn keeps_string_system_unchanged() {
        let mut body = json!({
            "model": "m",
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "hi"}]
        });
        super::normalize_messages_body(&mut body);
        assert_eq!(body["system"], "You are a helpful assistant.");
        assert_eq!(body["messages"][0]["content"], "hi");
    }

    #[test]
    fn collapses_system_array_to_string() {
        let mut body = json!({
            "model": "m",
            "system": [
                {"type": "text", "text": "First instruction"},
                {"type": "text", "text": "Second instruction"}
            ],
            "messages": [{"role": "user", "content": "hi"}]
        });
        super::normalize_messages_body(&mut body);
        assert_eq!(body["system"], "First instruction\nSecond instruction");
    }

    #[test]
    fn drops_metadata_field() {
        let mut body = json!({
            "model": "m",
            "metadata": {"user_id": "ezell", "other": "val"},
            "messages": [{"role": "user", "content": "hi"}]
        });
        super::normalize_messages_body(&mut body);
        assert!(body.get("metadata").is_none());
    }

    #[test]
    fn collapses_text_only_content_array() {
        let mut body = json!({
            "model": "m",
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "one"},
                        {"type": "text", "text": "two"}
                    ]
                }
            ]
        });
        super::normalize_messages_body(&mut body);
        assert_eq!(body["messages"][0]["content"], "one\ntwo");
    }

    #[test]
    fn moves_inline_system_messages_to_top_level() {
        let mut body = json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "sys line"},
                {"role": "user", "content": "hi"}
            ]
        });
        super::normalize_messages_body(&mut body);
        assert_eq!(body["system"], "sys line");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
    }
}
