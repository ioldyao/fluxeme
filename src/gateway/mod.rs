use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};

use crate::admin::{require_session_internal, AdminError};
use crate::domain::gateway::GatewayRoute;
use crate::server::AppState;

const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 50 * 1024 * 1024;
const MAX_TIMEOUT_MS: u64 = 60_000;
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];
const ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

#[derive(Debug, Deserialize)]
pub struct GatewayRouteRequest {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    pub path_prefix: String,
    pub upstream_url: String,
    #[serde(default = "default_methods")]
    pub methods: String,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub preserve_query: bool,
    #[serde(default = "default_true")]
    pub strip_prefix: bool,
    #[serde(default)]
    pub upstream_headers: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct GatewayRouteResponse {
    pub id: String,
    pub name: String,
    pub path_prefix: String,
    pub upstream_url: String,
    pub methods: String,
    pub timeout_ms: u64,
    pub enabled: bool,
    pub preserve_query: bool,
    pub strip_prefix: bool,
    pub upstream_headers: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct GatewayPath {
    pub rest: String,
}

fn default_methods() -> String {
    "GET,POST,PUT,PATCH,DELETE".to_string()
}
fn default_timeout() -> u64 {
    30_000
}
fn default_true() -> bool {
    true
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "error": { "message": message, "type": "gateway_error" } })),
    )
        .into_response()
}

async fn validate_route(req: &GatewayRouteRequest) -> Result<(), AdminError> {
    let prefix = req.path_prefix.trim();
    if prefix.is_empty()
        || !prefix.starts_with('/')
        || prefix.contains("..")
        || prefix.contains('?')
    {
        return Err(AdminError::bad_request(
            "path_prefix must be an absolute safe path",
        ));
    }
    if prefix != "/" && prefix.ends_with('/') {
        return Err(AdminError::bad_request(
            "path_prefix must not end with '/': use a path boundary",
        ));
    }
    let url = url::Url::parse(req.upstream_url.trim())
        .map_err(|_| AdminError::bad_request("upstream_url must be a valid URL"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(AdminError::bad_request(
            "upstream_url must be HTTP(S) without credentials",
        ));
    }
    if req.timeout_ms == 0 || req.timeout_ms > MAX_TIMEOUT_MS {
        return Err(AdminError::bad_request(
            "timeout_ms is outside the allowed range",
        ));
    }
    let policy = fluxeme_skill_backing::policy::UpstreamPolicy::default();
    policy
        .validate(req.upstream_url.trim(), Some(req.timeout_ms))
        .await
        .map_err(|_| AdminError::bad_request("upstream_url is blocked by SSRF policy"))?;
    let methods: Vec<&str> = req
        .methods
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .collect();
    if methods.is_empty() || methods.iter().any(|m| !ALLOWED_METHODS.contains(m)) {
        return Err(AdminError::bad_request(
            "methods contains an unsupported HTTP method",
        ));
    }
    for name in req.upstream_headers.keys() {
        let lower = name.to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lower.as_str())
            || matches!(lower.as_str(), "host" | "content-length")
        {
            return Err(AdminError::bad_request(
                "upstream_headers contains a protected header",
            ));
        }
        HeaderName::try_from(name)
            .map_err(|_| AdminError::bad_request("invalid upstream header name"))?;
    }
    Ok(())
}

fn normalize_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn encrypted_headers(
    headers: &HashMap<String, String>,
    secret: &str,
) -> Result<String, AdminError> {
    serde_json::to_string(headers)
        .map(|json| crate::crypto::encrypt_store(&json, secret))
        .map_err(|_| AdminError::bad_request("invalid upstream headers"))
}

fn response_route(route: GatewayRoute, secret: &str) -> GatewayRouteResponse {
    let headers = crate::crypto::decrypt_load(&route.upstream_headers, secret)
        .ok()
        .and_then(|json| serde_json::from_str::<HashMap<String, String>>(&json).ok())
        .map(|m| m.into_keys().collect())
        .unwrap_or_default();
    GatewayRouteResponse {
        id: route.id,
        name: route.name,
        path_prefix: route.path_prefix,
        upstream_url: route.upstream_url,
        methods: route.methods,
        timeout_ms: route.timeout_ms,
        enabled: route.enabled,
        preserve_query: route.preserve_query,
        strip_prefix: route.strip_prefix,
        upstream_headers: headers,
        created_at: route.created_at,
        updated_at: route.updated_at,
    }
}

async fn admin_session(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<crate::domain::user::SessionInfo, AdminError> {
    let session = require_session_internal(&state.admin, headers).await?;
    if !state.authz.enforce(&session.role, "admin:gateway").await {
        return Err(AdminError::forbidden("Insufficient permissions"));
    }
    Ok(session)
}

pub async fn list_routes(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<GatewayRouteResponse>>, AdminError> {
    admin_session(&state, &headers).await?;
    let routes = state.db.list_gateway_routes().await.map_err(|e| {
        tracing::error!("list gateway routes: {e}");
        AdminError::internal("Internal server error")
    })?;
    Ok(Json(
        routes
            .into_iter()
            .map(|route| response_route(route, state.admin.encryption_key()))
            .collect(),
    ))
}

pub async fn create_route(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<GatewayRouteRequest>,
) -> Result<Json<GatewayRouteResponse>, AdminError> {
    admin_session(&state, &headers).await?;
    validate_route(&req).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let route = GatewayRoute {
        id: req
            .id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        name: req.name.trim().to_string(),
        path_prefix: normalize_prefix(&req.path_prefix),
        upstream_url: req.upstream_url.trim_end_matches('/').to_string(),
        methods: req.methods,
        timeout_ms: req.timeout_ms,
        enabled: req.enabled,
        preserve_query: req.preserve_query,
        strip_prefix: req.strip_prefix,
        upstream_headers: encrypted_headers(&req.upstream_headers, state.admin.encryption_key())?,
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.create_gateway_route(&route).await.map_err(|e| {
        tracing::error!("create gateway route: {e}");
        AdminError::conflict("Gateway route could not be created")
    })?;
    Ok(Json(response_route(route, state.admin.encryption_key())))
}

pub async fn update_route(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<GatewayRouteRequest>,
) -> Result<Json<GatewayRouteResponse>, AdminError> {
    admin_session(&state, &headers).await?;
    validate_route(&req).await?;
    let old = state
        .db
        .get_gateway_route(&id)
        .await
        .map_err(|_| AdminError::internal("Internal server error"))?
        .ok_or_else(|| AdminError::not_found("Gateway route not found"))?;
    let route = GatewayRoute {
        id,
        name: req.name.trim().to_string(),
        path_prefix: normalize_prefix(&req.path_prefix),
        upstream_url: req.upstream_url.trim_end_matches('/').to_string(),
        methods: req.methods,
        timeout_ms: req.timeout_ms,
        enabled: req.enabled,
        preserve_query: req.preserve_query,
        strip_prefix: req.strip_prefix,
        upstream_headers: if req.upstream_headers.is_empty() {
            old.upstream_headers
        } else {
            encrypted_headers(&req.upstream_headers, state.admin.encryption_key())?
        },
        created_at: old.created_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    state
        .db
        .update_gateway_route(&route)
        .await
        .map_err(|_| AdminError::conflict("Gateway route could not be updated"))?;
    Ok(Json(response_route(route, state.admin.encryption_key())))
}

pub async fn delete_route(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, AdminError> {
    admin_session(&state, &headers).await?;
    state
        .db
        .delete_gateway_route(&id)
        .await
        .map_err(|_| AdminError::internal("Internal server error"))?;
    Ok(StatusCode::NO_CONTENT)
}

fn path_matches(prefix: &str, path: &str) -> bool {
    prefix == "/"
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn join_upstream(base: &str, prefix: &str, path: &str, strip: bool) -> Result<String, String> {
    let mut url = url::Url::parse(base).map_err(|_| "invalid upstream")?;
    let suffix = if strip {
        path.strip_prefix(prefix).unwrap_or("")
    } else {
        path
    };
    let suffix = suffix.trim_start_matches('/');
    let base_path = url.path().trim_end_matches('/').to_string();
    let joined = if suffix.is_empty() {
        base_path
    } else if base_path.is_empty() {
        format!("/{suffix}")
    } else {
        format!("{base_path}/{suffix}")
    };
    url.set_path(&joined);
    Ok(url.to_string())
}

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
}

pub async fn gateway_proxy(
    State(state): State<Arc<AppState>>,
    Path(path): Path<GatewayPath>,
    method: Method,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let started = Instant::now();
    if body.len() > MAX_BODY_BYTES {
        return json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "request body exceeds gateway limit",
        );
    }
    let path = format!("/{rest}", rest = path.rest.trim_start_matches('/'));
    // 鉴权先行：先验证 API Key，再匹配路由，避免向未认证调用方泄露路由存在性。
    let bearer = match extract_bearer(&headers) {
        Some(v) if !v.is_empty() => v,
        _ => {
            return json_error(
                StatusCode::UNAUTHORIZED,
                "missing Authorization: Bearer <api-key>",
            )
        }
    };
    let (user, key) = match state.db.backend.lookup_key(bearer).await {
        Ok(Some(v)) => v,
        _ => return json_error(StatusCode::UNAUTHORIZED, "invalid API key"),
    };
    if !key.enabled || user.status != crate::domain::user::USER_STATUS_ACTIVE {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "API key is not authorized for gateway access",
        );
    }
    if let Some(expires_at) = key.expires_at.as_deref() {
        match chrono::DateTime::parse_from_rfc3339(expires_at) {
            Ok(expiry) if chrono::Utc::now() >= expiry => {
                return json_error(StatusCode::UNAUTHORIZED, "API key has expired");
            }
            Ok(_) => {}
            Err(_) => return json_error(StatusCode::UNAUTHORIZED, "invalid API key"),
        }
    }
    // scope 校验用通配符查询（`*`），具体路由匹配放在鉴权全部通过之后。
    let has_scope = state
        .db
        .api_key_has_resource_scope(&key.key, "gateway", "*", "invoke")
        .await
        .unwrap_or(false)
        || state
            .db
            .api_key_has_resource_scope(&key.key, "gateway", &path, "invoke")
            .await
            .unwrap_or(false);
    if !has_scope {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "API key is not authorized for gateway access",
        );
    }
    let routes = match state.db.list_gateway_routes().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("gateway route lookup: {e}");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "gateway unavailable");
        }
    };
    let route = match routes
        .into_iter()
        .filter(|r| {
            r.enabled
                && path_matches(&r.path_prefix, &path)
                && r.methods.split(',').any(|m| m.trim() == method.as_str())
        })
        .max_by_key(|r| r.path_prefix.len())
    {
        Some(r) => r,
        None => return json_error(StatusCode::NOT_FOUND, "gateway route not found"),
    };
    let policy = fluxeme_skill_backing::policy::UpstreamPolicy::default();
    if policy
        .validate(&route.upstream_url, Some(route.timeout_ms))
        .await
        .is_err()
    {
        return json_error(StatusCode::BAD_GATEWAY, "gateway upstream is unavailable");
    }
    let mut upstream_url = match join_upstream(
        &route.upstream_url,
        &route.path_prefix,
        &path,
        route.strip_prefix,
    ) {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_GATEWAY, "invalid gateway upstream"),
    };
    if route.preserve_query {
        if let Some(query) = uri.query() {
            upstream_url.push('?');
            upstream_url.push_str(query);
        }
    }
    let client = match reqwest::Client::builder().redirect(Policy::none()).build() {
        Ok(c) => c,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "gateway unavailable"),
    };
    let mut req = client
        .request(method.clone(), upstream_url)
        .timeout(Duration::from_millis(route.timeout_ms))
        .body(body.clone());
    for (name, value) in &headers {
        let n = name.as_str().to_ascii_lowercase();
        if HOP_BY_HOP.contains(&n.as_str())
            || matches!(n.as_str(), "authorization" | "host" | "content-length")
        {
            continue;
        }
        req = req.header(name, value);
    }
    if let Some(json) =
        crate::crypto::decrypt_load(&route.upstream_headers, state.admin.encryption_key())
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, String>>(&s).ok())
    {
        for (name, value) in json {
            if let (Ok(n), Ok(v)) = (HeaderName::try_from(name), HeaderValue::from_str(&value)) {
                req = req.header(n, v);
            }
        }
    }
    let upstream = match req.send().await {
        Ok(r) => r,
        Err(_) => return json_error(StatusCode::BAD_GATEWAY, "gateway upstream request failed"),
    };
    let status = upstream.status();
    let out_headers = upstream.headers().clone();
    let bytes = match upstream.bytes().await {
        Ok(b) if b.len() <= MAX_RESPONSE_BYTES => b,
        Ok(_) => return json_error(StatusCode::BAD_GATEWAY, "gateway response exceeds limit"),
        Err(_) => return json_error(StatusCode::BAD_GATEWAY, "gateway upstream response failed"),
    };
    if let Some(ch) = &state.ch {
        let row = crate::ch_backend::GatewayCall {
            timestamp: chrono::Utc::now().timestamp() as u32,
            request_id: uuid::Uuid::new_v4().to_string(),
            route_id: route.id.clone(),
            method: method.to_string(),
            path: path.clone(),
            status_code: status.as_u16(),
            latency_ms: started.elapsed().as_millis() as u64,
            bytes_in: body.len() as u64,
            bytes_out: bytes.len() as u64,
            user_id: key.user_id,
            api_key_id: key.key,
        };
        if let Err(e) = ch.insert_gateway_calls(&[row]).await {
            tracing::warn!("gateway meter failed: {e}");
        }
    }
    let mut response = Response::new(axum::body::Body::from(bytes));
    *response.status_mut() = status;
    for (name, value) in &out_headers {
        if !HOP_BY_HOP.contains(&name.as_str()) {
            response.headers_mut().insert(name, value.clone());
        }
    }
    response
}
