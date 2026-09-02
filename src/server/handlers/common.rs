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
        let body = serde_json::json!({
            "error": {
                "message": self.message(),
                "type": "gateway_error",
            }
        });
        (self.status(), Json(body)).into_response()
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

impl From<FilterBlocked> for GatewayError {
    fn from(e: FilterBlocked) -> Self {
        Self::BadRequest(e.0)
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn authorize_effective_model(
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

fn trim_model(body: &mut Value) -> Result<String, GatewayError> {
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
/// field. Claude Code occasionally sends system prompts as inline messages
/// with role="system", which SGLang's /v1/messages rejects (only "user" and
/// "assistant" are allowed in the messages array).
fn normalize_messages_body(body: &mut Value) {
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
enum WalletAccount<'a> {
    User(&'a str),
    Team(&'a str),
}

/// Check Redis gate status first. PostgreSQL is used only for a cold read when
/// Redis is reachable but has no cached status; Redis errors are propagated.
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

/// Non-standard reasoning field names from various providers to normalize.
const REASONING_ALIASES: &[&str] = &["reasoning", "thinking", "thinking_content"];

fn rename_to_reasoning_content(obj: &mut serde_json::Map<String, Value>) {
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

fn normalize_reasoning_inner(val: &mut Value) {
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

fn normalize_sse_reasoning(data: &str) -> String {
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

// ── Helpers ─────────────────────────────────────────────────────────

fn extract_client_ip(_headers: &HeaderMap, addr: SocketAddr) -> String {
    addr.ip().to_string()
}

struct RouteTarget {
    channel_id: String,
    endpoint: EndpointConfig,
    /// Index of the current endpoint in the balancer's endpoint list.
    /// Circuit-breaker feedback (record_success / record_failure) is keyed
    /// by this index so live traffic updates real-time endpoint health.
    endpoint_idx: usize,
    adapter: Arc<dyn crate::provider::ProviderAdapter>,
    balancer: Arc<LoadBalancer>,
    routing: Arc<crate::service::routing::RoutingService>,
    providers: Arc<crate::provider::ProviderRegistry>,
    model: String,
    upstream_model: Option<String>,
    /// Endpoint identities already attempted by this request. A retry must
    /// never immediately revisit the same binding endpoint.
    attempted_endpoint_ids: std::collections::HashSet<i64>,
    attempted_endpoint_indexes: std::collections::HashSet<usize>,
    attempted_channels: std::collections::HashSet<String>,
}

impl RouteTarget {
    /// Try the next available endpoint from the balancer.
    /// Returns `false` if no more endpoints available.
    fn retry_next(&mut self) -> bool {
        if let Some(id) = self.endpoint.id {
            self.attempted_endpoint_ids.insert(id);
        }
        self.attempted_endpoint_indexes.insert(self.endpoint_idx);
        if let Some((idx, ep)) = self
            .balancer
            .as_health_aware()
            .select_healthy_excluding_indexes(
                &self.attempted_endpoint_ids,
                &self.attempted_endpoint_indexes,
            )
        {
            self.endpoint_idx = idx;
            self.endpoint = ep.clone();
            return true;
        }
        self.attempted_channels.insert(self.channel_id.clone());
        let Ok(plan) = self.routing.route_model_binding_for_model_excluding_channels(
            &self.model,
            self.upstream_model.as_deref(),
            &[],
            &self.attempted_channels,
        ) else {
            return false;
        };
        let Some(provider_name) = self.routing.get_route(&plan.channel_id).map(|(name, _)| name)
        else {
            return false;
        };
        let Some(adapter) = self.providers.get(&provider_name) else {
            return false;
        };
        self.channel_id = plan.channel_id;
        self.endpoint_idx = plan.endpoint_idx;
        self.endpoint = plan.endpoint;
        self.balancer = plan.balancer;
        self.adapter = adapter;
        self.attempted_endpoint_ids.clear();
        self.attempted_endpoint_indexes.clear();
        true
    }

    /// Feed a successful upstream call into the circuit breaker (closes it).
    fn report_success(&self) {
        self.balancer
            .as_health_aware()
            .record_success(self.endpoint_idx);
    }

    /// Feed an upstream failure (connect / 5xx / timeout) into the breaker.
    fn report_failure(&mut self) {
        self.balancer
            .as_health_aware()
            .record_failure(self.endpoint_idx);
    }
}

fn resolve_route(state: &AppState, channel_id: &str) -> Result<RouteTarget, GatewayError> {
    let (provider_name, balancer) = state
        .routing
        .get_route(channel_id)
        .ok_or_else(|| GatewayError::Internal("Channel route unavailable".into()))?;

    let adapter = state
        .providers
        .get(provider_name.as_str())
        .ok_or_else(|| GatewayError::Internal("Provider not available".into()))?;

    let (idx, endpoint) = balancer
        .as_health_aware()
        .select()
        .ok_or_else(|| GatewayError::Internal("No available endpoints".into()))?;

    Ok(RouteTarget {
        channel_id: channel_id.to_string(),
        endpoint: endpoint.clone(),
        endpoint_idx: idx,
        adapter,
        balancer,
        routing: state.routing.clone(),
        providers: state.providers.clone(),
        model: String::new(),
        upstream_model: None,
        attempted_endpoint_ids: std::collections::HashSet::new(),
        attempted_endpoint_indexes: std::collections::HashSet::new(),
        attempted_channels: std::collections::HashSet::new(),
    })
}

/// Resolve the endpoint from the model-binding pool. System-rule targets
/// without a model binding retain a channel-level compatibility fallback.
fn resolve_route_for_model(
    state: &AppState,
    model: &str,
    channel_id: &str,
    upstream_model: Option<&str>,
) -> Result<RouteTarget, GatewayError> {
    // Resolve from the model binding pool first. If a rule-selected binding
    // has become unhealthy, choose another healthy binding for this request.
    let plan = state
        .routing
        .route_model_binding_for_channel_and_upstream(model, channel_id, upstream_model, &[])
        .or_else(|_| state.routing.route_model_binding_for_model(model, upstream_model, &[]))
        .map_err(GatewayError::from)?;
    let provider_name = plan.provider_name.clone();
    let adapter = state
        .providers
        .get(provider_name.as_str())
        .ok_or_else(|| GatewayError::Internal("Provider not available".into()))?;
    Ok(RouteTarget {
        channel_id: plan.channel_id,
        endpoint: plan.endpoint,
        endpoint_idx: plan.endpoint_idx,
        adapter,
        balancer: plan.balancer,
        routing: state.routing.clone(),
        providers: state.providers.clone(),
        model: model.to_string(),
        upstream_model: upstream_model.map(str::to_string),
        attempted_endpoint_ids: std::collections::HashSet::new(),
        attempted_endpoint_indexes: std::collections::HashSet::new(),
        attempted_channels: std::collections::HashSet::new(),
    })
}


