//! Qianfan (百度千帆) Token Plan provider adapter.
//!
//! Token Plan 渠道使用固定 Base URL `https://qianfan.baidubce.com`，由后端
//! 根据请求类型自动拼接路径：
//! - `/v1/chat/completions` (OpenAI) → `{base}/v2/tokenplan/personal/v1/chat/completions`（Bearer）
//! - `/v1/messages` (Anthropic) → `{base}/anthropic/tokenplan/personal/v1/messages`（x-api-key）
//!
//! 普通「千帆大模型」渠道走 [`GenericAdapter`]，用户自行填写完整 URL，无需此适配器。
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use futures::stream::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;

use super::{
    classify_reqwest_error, classify_status, default_config, request_timeout, ErrorKind,
    ProviderAdapter, ProviderError, RequestKind, StreamResult,
};
use crate::config::types::EndpointConfig;
use crate::provider::shared_client;

pub struct QianfanTokenPlanAdapter;

/// Upstream endpoint kind; decides the path and auth header style.
#[derive(Debug, Clone, Copy, PartialEq)]
enum UrlKind {
    Messages,
    ChatCompletions,
    CountTokens,
    Responses,
    ResponsesInputTokens,
    /// `/v2/tokenplan/personal/v1/responses/{id}`
    ResponsesRetrieve,
    /// `/v2/tokenplan/personal/v1/responses/{id}/input_items`
    ResponsesInputItems,
}

impl UrlKind {
    /// Paths are relative to the `qianfan.baidubce.com` root; the Token Plan
    /// prefix is already part of the kind-specific path.
    fn path(self) -> &'static str {
        match self {
            UrlKind::Messages => "/anthropic/tokenplan/personal/v1/messages",
            UrlKind::ChatCompletions => "/v2/tokenplan/personal/chat/completions",
            UrlKind::CountTokens => "/anthropic/tokenplan/personal/v1/messages/count_tokens",
            UrlKind::Responses => "/v2/tokenplan/personal/responses",
            UrlKind::ResponsesInputTokens => "/v2/tokenplan/personal/responses/input_tokens",
            UrlKind::ResponsesRetrieve => "/v2/tokenplan/personal/responses/{id}",
            UrlKind::ResponsesInputItems => "/v2/tokenplan/personal/responses/{id}/input_items",
        }
    }

    /// Anthropic-compatible endpoints use `x-api-key` + `anthropic-version`.
    fn is_anthropic(self) -> bool {
        matches!(self, UrlKind::Messages | UrlKind::CountTokens)
    }
}

impl QianfanTokenPlanAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Base URL 固定为 `https://qianfan.baidubce.com`；若配置里带上了
    /// `/v2/tokenplan/personal` 或 `/anthropic/tokenplan/personal` 后缀则归一到根域名，
    /// 保证旧配置兼容。
    fn domain_base(endpoint: &EndpointConfig) -> String {
        let base = endpoint.url.trim_end_matches('/');
        base.strip_suffix("/anthropic/tokenplan/personal")
            .or_else(|| base.strip_suffix("/v2/tokenplan/personal"))
            .or_else(|| base.strip_suffix("/tokenplan/personal"))
            .unwrap_or(base)
            .to_string()
    }

    /// Build the full upstream URL for the given request kind.
    async fn build_url(endpoint: &EndpointConfig, kind: UrlKind) -> Result<String, ProviderError> {
        if endpoint.full_url {
            // 完整 URL 直接用（兼容老配置：用户自己填了完整端点）。
            return Ok(endpoint.url.clone());
        }
        super::validate_endpoint_url(&endpoint.url).await?;
        let domain = Self::domain_base(endpoint);
        Ok(format!("{domain}{}", kind.path()))
    }

    /// Build headers for a given kind: Anthropic vs OpenAI-compatible.
    fn build_headers(endpoint: &EndpointConfig, kind: UrlKind) -> Result<HeaderMap, ProviderError> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if kind.is_anthropic() {
            headers.insert(
                "x-api-key",
                HeaderValue::from_str(&endpoint.api_key).map_err(|e| {
                    ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other)
                })?,
            );
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        } else {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", endpoint.api_key)).map_err(|e| {
                    ProviderError::new(format!("Invalid API key: {}", e), ErrorKind::Other)
                })?,
            );
        }
        Ok(headers)
    }

    async fn do_send(
        client: Arc<reqwest::Client>,
        url: &str,
        headers: HeaderMap,
        body: &Value,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        let resp_start = Instant::now();
        let resp = client
            .post(url)
            .headers(headers)
            .json(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                let kind = classify_reqwest_error(&e);
                tracing::error!(
                    url = %url,
                    error = %e,
                    error_kind = ?kind,
                    elapsed_ms = resp_start.elapsed().as_millis(),
                    "Upstream HTTP request failed"
                );
                ProviderError::new(format!("Request failed: {}", e), kind)
            })?;

        let status = resp.status();
        let body_resp = resp.bytes().await.map_err(|e| {
            ProviderError::new(
                format!("Failed to read response body: {}", e),
                ErrorKind::Parse,
            )
        })?;

        if !status.is_success() {
            let resp_text = String::from_utf8_lossy(&body_resp);
            let upstream_msg = serde_json::from_str::<Value>(&resp_text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from))
                .unwrap_or(resp_text.trim().to_string());
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %resp_text, "Upstream request failed");
            return Err(ProviderError::new(
                format!(
                    "Upstream request failed with status {}: {}",
                    status.as_u16(),
                    upstream_msg
                ),
                kind,
            ));
        }

        let resp_body: Value = serde_json::from_slice(&body_resp).map_err(|e| {
            ProviderError::new(format!("Failed to parse response: {}", e), ErrorKind::Parse)
        })?;
        Ok(resp_body)
    }

    async fn do_send_stream(
        client: Arc<reqwest::Client>,
        url: &str,
        headers: HeaderMap,
        body: &Value,
        timeout: Duration,
    ) -> Result<StreamResult, ProviderError> {
        let resp = client
            .post(url)
            .headers(headers)
            .json(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| {
                let kind = classify_reqwest_error(&e);
                tracing::error!(
                    url = %url,
                    error = %e,
                    error_kind = ?kind,
                    "Upstream stream request failed"
                );
                ProviderError::new(format!("Stream request failed: {}", e), kind)
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %body_text, "Upstream stream request failed");
            return Err(ProviderError::new(
                format!(
                    "Stream request failed with status {}: {}",
                    status.as_u16(),
                    body_text.trim()
                ),
                kind,
            ));
        }

        let byte_stream = resp.bytes_stream();
        let mapped = byte_stream.map(|chunk| match chunk {
            Ok(bytes) => String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).to_string()),
            Err(e) => format!("data: {{\"error\":\"{}\"}}\n\n", e),
        });

        Ok(Pin::from(Box::new(mapped)))
    }
}

#[async_trait::async_trait]
impl ProviderAdapter for QianfanTokenPlanAdapter {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let url = Self::build_url(endpoint, UrlKind::ChatCompletions).await?;
        let headers = Self::build_headers(endpoint, UrlKind::ChatCompletions)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, "QianfanTokenPlan chat_completions → {}", url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let url = Self::build_url(endpoint, UrlKind::ChatCompletions).await?;
        let headers = Self::build_headers(endpoint, UrlKind::ChatCompletions)?;
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(endpoint = %endpoint.url, "QianfanTokenPlan chat_completions_stream → {}", url);
        Self::do_send_stream(shared_client(), &url, headers, &body, timeout).await
    }

    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let url = Self::build_url(endpoint, UrlKind::Messages).await?;
        let headers = Self::build_headers(endpoint, UrlKind::Messages)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, "QianfanTokenPlan messages → {}", url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let url = Self::build_url(endpoint, UrlKind::Messages).await?;
        let headers = Self::build_headers(endpoint, UrlKind::Messages)?;
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(endpoint = %endpoint.url, "QianfanTokenPlan messages_stream → {}", url);
        Self::do_send_stream(shared_client(), &url, headers, &body, timeout).await
    }

    async fn count_tokens(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let url = Self::build_url(endpoint, UrlKind::CountTokens).await?;
        let headers = Self::build_headers(endpoint, UrlKind::CountTokens)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, "QianfanTokenPlan count_tokens → {}", url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn responses_input_tokens(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let url = Self::build_url(endpoint, UrlKind::ResponsesInputTokens).await?;
        let headers = Self::build_headers(endpoint, UrlKind::ResponsesInputTokens)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, "QianfanTokenPlan responses_input_tokens → {}", url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn responses_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let url = Self::build_url(endpoint, UrlKind::Responses).await?;
        let headers = Self::build_headers(endpoint, UrlKind::Responses)?;
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(endpoint = %endpoint.url, "QianfanTokenPlan responses_stream → {}", url);
        Self::do_send_stream(shared_client(), &url, headers, &body, timeout).await
    }

    async fn relay(
        &self,
        endpoint: &EndpointConfig,
        path: &str,
        body: Value,
    ) -> Result<Value, ProviderError> {
        // 非流式 /v1/responses → /v2/tokenplan/personal/v1/responses
        let path = path.trim_end_matches('/');
        let url = if path == "/v1/responses" {
            Self::build_url(endpoint, UrlKind::Responses).await?
        } else if let Some(id) = extract_response_id(path, "/input_items") {
            let mut url = Self::build_url(endpoint, UrlKind::ResponsesInputItems).await?;
            url = url.replacen("{id}", &id, 1);
            url
        } else if let Some(id) = extract_response_id(path, "") {
            let mut url = Self::build_url(endpoint, UrlKind::ResponsesRetrieve).await?;
            url = url.replacen("{id}", &id, 1);
            url
        } else {
            // 其它路径：domain + 剥掉 /v1 前缀的剩余路径（兼容通用转发）。
            let domain = Self::domain_base(endpoint);
            let rest = path.strip_prefix("/v1").unwrap_or(path);
            format!("{domain}{rest}")
        };
        let headers = Self::build_headers(endpoint, UrlKind::Responses)?;
        let timeout = request_timeout(
            &RequestKind::Unary { body_size: 0 },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, "QianfanTokenPlan relay → {}", url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }
}

/// Extract a response id from a path like `/v1/responses/{id}/input_items`
/// (suffix is `/input_items` or empty for the plain retrieve path).
fn extract_response_id(path: &str, suffix: &str) -> Option<String> {
    let rest = path.strip_prefix("/v1/responses/")?;
    let rest = rest.strip_suffix(suffix).unwrap_or(rest);
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(url: &str, full_url: bool) -> EndpointConfig {
        EndpointConfig {
            id: None,
            url: url.to_string(),
            api_key: "sk-test-synthetic".to_string(),
            weight: 1,
            timeout_secs: None,
            enabled: true,
            full_url,
        }
    }

    #[tokio::test]
    async fn chat_builds_openai_tokenplan_path() {
        let e = ep("https://qianfan.baidubce.com", false);
        assert_eq!(
            QianfanTokenPlanAdapter::build_url(&e, UrlKind::ChatCompletions)
                .await
                .unwrap(),
            "https://qianfan.baidubce.com/v2/tokenplan/personal/chat/completions"
        );
    }

    #[tokio::test]
    async fn messages_builds_anthropic_tokenplan_path() {
        let e = ep("https://qianfan.baidubce.com", false);
        assert_eq!(
            QianfanTokenPlanAdapter::build_url(&e, UrlKind::Messages)
                .await
                .unwrap(),
            "https://qianfan.baidubce.com/anthropic/tokenplan/personal/v1/messages"
        );
    }

    #[tokio::test]
    async fn legacy_tokenplan_suffix_is_normalized() {
        let e = ep("https://qianfan.baidubce.com/v2/tokenplan/personal", false);
        assert_eq!(
            QianfanTokenPlanAdapter::build_url(&e, UrlKind::Messages)
                .await
                .unwrap(),
            "https://qianfan.baidubce.com/anthropic/tokenplan/personal/v1/messages"
        );
        let e2 = ep(
            "https://qianfan.baidubce.com/anthropic/tokenplan/personal",
            false,
        );
        assert_eq!(
            QianfanTokenPlanAdapter::build_url(&e2, UrlKind::ChatCompletions)
                .await
                .unwrap(),
            "https://qianfan.baidubce.com/v2/tokenplan/personal/chat/completions"
        );
    }

    #[tokio::test]
    async fn full_url_is_used_verbatim() {
        let e = ep("https://example.com/custom/path", true);
        assert_eq!(
            QianfanTokenPlanAdapter::build_url(&e, UrlKind::Messages)
                .await
                .unwrap(),
            "https://example.com/custom/path"
        );
    }

    #[test]
    fn extracts_response_ids() {
        assert_eq!(
            extract_response_id("/v1/responses/resp_123/input_items", "/input_items"),
            Some("resp_123".to_string())
        );
        assert_eq!(
            extract_response_id("/v1/responses/resp_123", ""),
            Some("resp_123".to_string())
        );
        assert_eq!(extract_response_id("/v1/responses", ""), None);
        assert_eq!(extract_response_id("/v1/responses/a/b", ""), None);
    }

    #[test]
    fn anthropic_kinds_use_x_api_key_style() {
        assert!(UrlKind::Messages.is_anthropic());
        assert!(UrlKind::CountTokens.is_anthropic());
        assert!(!UrlKind::ChatCompletions.is_anthropic());
        assert!(!UrlKind::Responses.is_anthropic());
    }
}
