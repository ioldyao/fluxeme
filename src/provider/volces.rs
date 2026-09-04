//! Volces Ark (火山方舟) Plan provider adapter.
//!
//! Agent Plan / Coding Plan 渠道使用固定 Base URL `https://ark.cn-beijing.volces.com`，
//! 由后端根据请求类型与计划类型自动拼接路径：
//!
//! **Agent Plan:**
//! - OpenAI:      `{base}/api/plan/v3/chat/completions`   (Bearer)
//! - Anthropic:   `{base}/api/plan/v1/messages`           (x-api-key)
//! - Responses:   `{base}/api/plan/v3/responses`
//!
//! **Coding Plan:**
//! - OpenAI:      `{base}/api/coding/v3/chat/completions`
//! - Anthropic:   `{base}/api/coding/v1/messages`
//! - Responses:   `{base}/api/coding/v3/responses`
//!
//! 普通「火山方舟」渠道走 [`GenericAdapter`]，用户自行填写含 `/api/v3` 的完整 URL。
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VolcesPlanType {
    AgentPlan,
    CodingPlan,
}

/// Upstream endpoint kind; decides the path and auth header style.
#[derive(Debug, Clone, Copy, PartialEq)]
enum UrlKind {
    Messages,
    ChatCompletions,
    CountTokens,
    Responses,
    ResponsesInputTokens,
    /// `/api/{plan}/v3/responses/{id}`
    ResponsesRetrieve,
    /// `/api/{plan}/v3/responses/{id}/input_items`
    ResponsesInputItems,
}

impl UrlKind {
    /// Anthropic-compatible endpoints use `x-api-key` + `anthropic-version`.
    fn is_anthropic(self) -> bool {
        matches!(self, UrlKind::Messages | UrlKind::CountTokens)
    }
}

impl VolcesPlanType {
    fn anthropic_path(self) -> &'static str {
        match self {
            VolcesPlanType::AgentPlan => "/api/plan/v1/messages",
            VolcesPlanType::CodingPlan => "/api/coding/v1/messages",
        }
    }

    fn chat_completions_path(self) -> &'static str {
        match self {
            VolcesPlanType::AgentPlan => "/api/plan/v3/chat/completions",
            VolcesPlanType::CodingPlan => "/api/coding/v3/chat/completions",
        }
    }

    fn count_tokens_path(self) -> &'static str {
        match self {
            VolcesPlanType::AgentPlan => "/api/plan/v1/messages/count_tokens",
            VolcesPlanType::CodingPlan => "/api/coding/v1/messages/count_tokens",
        }
    }

    fn responses_path(self) -> &'static str {
        match self {
            VolcesPlanType::AgentPlan => "/api/plan/v3/responses",
            VolcesPlanType::CodingPlan => "/api/coding/v3/responses",
        }
    }

    fn responses_input_tokens_path(self) -> &'static str {
        match self {
            VolcesPlanType::AgentPlan => "/api/plan/v3/responses/input_tokens",
            VolcesPlanType::CodingPlan => "/api/coding/v3/responses/input_tokens",
        }
    }

    fn responses_retrieve_path(self) -> &'static str {
        match self {
            VolcesPlanType::AgentPlan => "/api/plan/v3/responses/{id}",
            VolcesPlanType::CodingPlan => "/api/coding/v3/responses/{id}",
        }
    }

    fn responses_input_items_path(self) -> &'static str {
        match self {
            VolcesPlanType::AgentPlan => "/api/plan/v3/responses/{id}/input_items",
            VolcesPlanType::CodingPlan => "/api/coding/v3/responses/{id}/input_items",
        }
    }
}

pub struct VolcesPlanAdapter {
    plan_type: VolcesPlanType,
}

impl VolcesPlanAdapter {
    pub fn new(plan_type: VolcesPlanType) -> Self {
        Self { plan_type }
    }

    pub fn agent_plan() -> Self {
        Self::new(VolcesPlanType::AgentPlan)
    }

    pub fn coding_plan() -> Self {
        Self::new(VolcesPlanType::CodingPlan)
    }

    /// Normalize stored URL back to pure domain.
    /// 若配置里带上了 `/api/plan/v3`, `/api/plan`, `/api/coding/v3`, `/api/coding`
    /// 等旧格式后缀，则归一到根域名。
    fn domain_base(endpoint: &EndpointConfig) -> String {
        let base = endpoint.url.trim_end_matches('/');
        base.strip_suffix("/api/plan/v3")
            .or_else(|| base.strip_suffix("/api/plan"))
            .or_else(|| base.strip_suffix("/api/coding/v3"))
            .or_else(|| base.strip_suffix("/api/coding"))
            .unwrap_or(base)
            .to_string()
    }

    /// Build the full upstream URL for the given request kind.
    async fn build_url(
        &self,
        endpoint: &EndpointConfig,
        kind: UrlKind,
    ) -> Result<String, ProviderError> {
        if endpoint.full_url {
            return Ok(endpoint.url.clone());
        }
        super::validate_endpoint_url(&endpoint.url).await?;
        let base = Self::domain_base(endpoint);
        let path = match kind {
            UrlKind::Messages => self.plan_type.anthropic_path(),
            UrlKind::ChatCompletions => self.plan_type.chat_completions_path(),
            UrlKind::CountTokens => self.plan_type.count_tokens_path(),
            UrlKind::Responses => self.plan_type.responses_path(),
            UrlKind::ResponsesInputTokens => self.plan_type.responses_input_tokens_path(),
            UrlKind::ResponsesRetrieve => self.plan_type.responses_retrieve_path(),
            UrlKind::ResponsesInputItems => self.plan_type.responses_input_items_path(),
        };
        Ok(format!("{}{}", base, path))
    }

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
                    "VolcesPlan upstream request failed"
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
            tracing::error!(%status, body = %resp_text, "VolcesPlan upstream request failed");
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
                    "VolcesPlan upstream stream request failed"
                );
                ProviderError::new(format!("Stream request failed: {}", e), kind)
            })?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            let kind = classify_status(status.as_u16());
            tracing::error!(%status, body = %body_text, "VolcesPlan upstream stream request failed");
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
impl ProviderAdapter for VolcesPlanAdapter {
    async fn chat_complete(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let url = self.build_url(endpoint, UrlKind::ChatCompletions).await?;
        let headers = Self::build_headers(endpoint, UrlKind::ChatCompletions)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, "VolcesPlan(plan={:?}) chat → {}", self.plan_type, url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn chat_complete_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let url = self.build_url(endpoint, UrlKind::ChatCompletions).await?;
        let headers = Self::build_headers(endpoint, UrlKind::ChatCompletions)?;
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(endpoint = %endpoint.url, "VolcesPlan(plan={:?}) chat_stream → {}", self.plan_type, url);
        Self::do_send_stream(shared_client(), &url, headers, &body, timeout).await
    }

    async fn messages(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let url = self.build_url(endpoint, UrlKind::Messages).await?;
        let headers = Self::build_headers(endpoint, UrlKind::Messages)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, "VolcesPlan(plan={:?}) messages → {}", self.plan_type, url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn messages_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let url = self.build_url(endpoint, UrlKind::Messages).await?;
        let headers = Self::build_headers(endpoint, UrlKind::Messages)?;
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(endpoint = %endpoint.url, "VolcesPlan(plan={:?}) messages_stream → {}", self.plan_type, url);
        Self::do_send_stream(shared_client(), &url, headers, &body, timeout).await
    }

    async fn count_tokens(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let url = self.build_url(endpoint, UrlKind::CountTokens).await?;
        let headers = Self::build_headers(endpoint, UrlKind::CountTokens)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, "VolcesPlan(plan={:?}) count_tokens → {}", self.plan_type, url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn responses_input_tokens(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let url = self
            .build_url(endpoint, UrlKind::ResponsesInputTokens)
            .await?;
        let headers = Self::build_headers(endpoint, UrlKind::ResponsesInputTokens)?;
        let timeout = request_timeout(
            &RequestKind::Unary {
                body_size: body.to_string().len(),
            },
            endpoint,
            &default_config(),
        );
        tracing::info!(endpoint = %endpoint.url, "VolcesPlan(plan={:?}) responses_input_tokens → {}", self.plan_type, url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }

    async fn responses_stream(
        &self,
        endpoint: &EndpointConfig,
        body: Value,
    ) -> Result<StreamResult, ProviderError> {
        let url = self.build_url(endpoint, UrlKind::Responses).await?;
        let headers = Self::build_headers(endpoint, UrlKind::Responses)?;
        let timeout = request_timeout(&RequestKind::Streaming, endpoint, &default_config());
        tracing::info!(endpoint = %endpoint.url, "VolcesPlan(plan={:?}) responses_stream → {}", self.plan_type, url);
        Self::do_send_stream(shared_client(), &url, headers, &body, timeout).await
    }

    async fn relay(
        &self,
        endpoint: &EndpointConfig,
        path: &str,
        body: Value,
    ) -> Result<Value, ProviderError> {
        let path = path.trim_end_matches('/');
        let url = if path == "/v1/responses" {
            self.build_url(endpoint, UrlKind::Responses).await?
        } else if let Some(id) = extract_response_id(path, "/input_items") {
            let mut url = self
                .build_url(endpoint, UrlKind::ResponsesInputItems)
                .await?;
            url = url.replacen("{id}", &id, 1);
            url
        } else if let Some(id) = extract_response_id(path, "") {
            let mut url = self.build_url(endpoint, UrlKind::ResponsesRetrieve).await?;
            url = url.replacen("{id}", &id, 1);
            url
        } else {
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
        tracing::info!(endpoint = %endpoint.url, "VolcesPlan(plan={:?}) relay → {}", self.plan_type, url);
        Self::do_send(shared_client(), &url, headers, &body, timeout).await
    }
}

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
            api_key: "sk-test".to_string(),
            weight: 1,
            timeout_secs: None,
            max_tokens: None,
            enabled: true,
            full_url,
        }
    }

    #[tokio::test]
    async fn agent_plan_chat_builds_plan_v3_path() {
        let adapter = VolcesPlanAdapter::agent_plan();
        let e = ep("https://ark.cn-beijing.volces.com", false);
        let url = adapter
            .build_url(&e, UrlKind::ChatCompletions)
            .await
            .unwrap();
        assert_eq!(
            url,
            "https://ark.cn-beijing.volces.com/api/plan/v3/chat/completions"
        );
    }

    #[tokio::test]
    async fn agent_plan_messages_builds_plan_v1_path() {
        let adapter = VolcesPlanAdapter::agent_plan();
        let e = ep("https://ark.cn-beijing.volces.com", false);
        let url = adapter.build_url(&e, UrlKind::Messages).await.unwrap();
        assert_eq!(
            url,
            "https://ark.cn-beijing.volces.com/api/plan/v1/messages"
        );
    }

    #[tokio::test]
    async fn agent_plan_responses_builds_plan_v3_path() {
        let adapter = VolcesPlanAdapter::agent_plan();
        let e = ep("https://ark.cn-beijing.volces.com", false);
        let url = adapter.build_url(&e, UrlKind::Responses).await.unwrap();
        assert_eq!(
            url,
            "https://ark.cn-beijing.volces.com/api/plan/v3/responses"
        );
    }

    #[tokio::test]
    async fn coding_plan_chat_builds_coding_v3_path() {
        let adapter = VolcesPlanAdapter::coding_plan();
        let e = ep("https://ark.cn-beijing.volces.com", false);
        let url = adapter
            .build_url(&e, UrlKind::ChatCompletions)
            .await
            .unwrap();
        assert_eq!(
            url,
            "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions"
        );
    }

    #[tokio::test]
    async fn coding_plan_messages_builds_coding_v1_path() {
        let adapter = VolcesPlanAdapter::coding_plan();
        let e = ep("https://ark.cn-beijing.volces.com", false);
        let url = adapter.build_url(&e, UrlKind::Messages).await.unwrap();
        assert_eq!(
            url,
            "https://ark.cn-beijing.volces.com/api/coding/v1/messages"
        );
    }

    #[tokio::test]
    async fn legacy_plan_suffix_is_normalized() {
        let adapter = VolcesPlanAdapter::agent_plan();
        let e = ep("https://ark.cn-beijing.volces.com/api/plan/v3", false);
        let url = adapter.build_url(&e, UrlKind::Messages).await.unwrap();
        assert_eq!(
            url,
            "https://ark.cn-beijing.volces.com/api/plan/v1/messages"
        );
    }
}
