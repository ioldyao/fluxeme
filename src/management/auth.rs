use std::sync::Arc;

use crate::admin::management_keys::authenticate_management_key_metadata;
use crate::server::AppState;
use axum::extract::Request;
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ManagementPrincipal {
    pub key_id: String,
    pub creator_id: String,
    pub creator_name: String,
}

#[derive(Debug)]
pub(crate) struct ManagementError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
}

impl ManagementError {
    pub(crate) fn unauthorized(message: &'static str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message,
        }
    }

    pub(crate) fn internal() -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: "Management API request failed",
        }
    }

    pub(crate) fn unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "dependency_unavailable",
            message: "Management API dependency unavailable",
        }
    }

    pub(crate) fn rate_limited() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            message: "Too many management API requests",
        }
    }
}

impl IntoResponse for ManagementError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            axum::Json(serde_json::json!({
                "error": {
                    "type": self.code,
                    "message": self.message,
                }
            })),
        )
            .into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                axum::http::HeaderValue::from_static("Bearer"),
            );
        }
        response
    }
}

fn presented_bearer(headers: &HeaderMap) -> Result<&str, ManagementError> {
    if headers.contains_key(header::COOKIE) || headers.contains_key("x-api-key") {
        return Err(ManagementError::unauthorized(
            "Management API requires a Bearer key",
        ));
    }
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values
        .next()
        .ok_or(ManagementError::unauthorized("Missing management API key"))?;
    if values.next().is_some() {
        return Err(ManagementError::unauthorized(
            "Multiple Authorization headers are not allowed",
        ));
    }
    let value = value
        .to_str()
        .map_err(|_| ManagementError::unauthorized("Invalid Authorization header"))?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or(ManagementError::unauthorized(
            "Management API requires Bearer authentication",
        ))?;
    if !token.starts_with("mk-")
        || token.len() > 128
        || token
            .chars()
            .any(|ch| ch.is_ascii_whitespace() || ch.is_control())
    {
        return Err(ManagementError::unauthorized("Invalid management API key"));
    }
    Ok(token)
}

pub(crate) async fn require_management_key(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ManagementError> {
    if request.uri().query().is_some() {
        return Err(ManagementError::unauthorized(
            "Management API credentials must not be supplied in the query string",
        ));
    }
    let token = presented_bearer(request.headers())?;
    let (key, creator) = authenticate_management_key_metadata(&state.admin, token)
        .await
        .map_err(|error| match error {
            crate::admin::AdminError::TooManyRequests(_) => ManagementError::rate_limited(),
            _ => ManagementError::unauthorized("Invalid management API key"),
        })?;
    request.extensions_mut().insert(ManagementPrincipal {
        key_id: key.id,
        creator_id: creator.id,
        creator_name: creator.name,
    });
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_bearer_management_keys() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer mk-synthetic".parse().unwrap(),
        );
        assert_eq!(presented_bearer(&headers).unwrap(), "mk-synthetic");

        headers.insert(header::AUTHORIZATION, "Basic mk-synthetic".parse().unwrap());
        assert!(presented_bearer(&headers).is_err());
    }

    #[test]
    fn rejects_cookie_and_data_plane_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer mk-synthetic".parse().unwrap(),
        );
        headers.insert(header::COOKIE, "session_token=synthetic".parse().unwrap());
        assert!(presented_bearer(&headers).is_err());

        headers.remove(header::COOKIE);
        headers.insert("x-api-key", "sk-synthetic".parse().unwrap());
        assert!(presented_bearer(&headers).is_err());
    }

    #[test]
    fn rejects_whitespace_control_and_oversized_tokens() {
        for token in ["mk- with-space", &format!("mk-{}", "x".repeat(130))] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                format!("Bearer {token}").parse().unwrap(),
            );
            assert!(presented_bearer(&headers).is_err());
        }
        assert!(
            axum::http::HeaderValue::from_bytes(b"Bearer mk-\ncontrol").is_err(),
            "HTTP rejects control characters before management parsing",
        );
    }
}
