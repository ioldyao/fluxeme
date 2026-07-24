use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::service::routing::match_pattern;

#[derive(Debug, Clone, Serialize)]
pub struct UpstreamModelInfo {
    pub id: String,
    pub max_model_len: Option<i64>,
}

#[derive(Serialize)]
pub struct ChannelHealthResult {
    pub channel_id: String,
    pub endpoints: Vec<EndpointHealth>,
}

#[derive(Serialize)]
pub struct EndpointHealth {
    pub url: String,
    pub reachable: bool,
    pub models_found: usize,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct UpstreamModel {
    id: String,
    #[allow(dead_code)]
    object: Option<String>,
    #[allow(dead_code)]
    created: Option<i64>,
    #[allow(dead_code)]
    owned_by: Option<String>,
    max_model_len: Option<i64>,
}

#[derive(Deserialize)]
struct UpstreamModelsResponse {
    data: Vec<UpstreamModel>,
}

pub struct HealthService {
    db: std::sync::Arc<Database>,
    client: reqwest::Client,
    enc_key: String,
}

impl HealthService {
    pub fn new(db: std::sync::Arc<Database>, enc_key: &str) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
        Ok(Self { db, client, enc_key: enc_key.to_string() })
    }

    /// Check a single channel by ID. Returns per-endpoint health results.
    pub async fn check_channel(&self, channel_id: &str) -> Result<ChannelHealthResult, String> {
        let channels = self.db.list_channels().await.map_err(|e| e.0)?;
        let ch = channels.iter().find(|c| c.id == channel_id)
            .ok_or_else(|| format!("Channel '{}' not found", channel_id))?;

        let mut ep_results = Vec::new();
        for ep in &ch.endpoints {
            let base = ep.url.trim_end_matches('/').trim_end_matches("/v1");
            let url = format!("{}/v1/models", base);
            let api_key = crate::crypto::decrypt_load(&ep.api_key, &self.enc_key).map_err(|e| {
                format!(
                    "Failed to decrypt API key for channel '{}' endpoint {:?}: {}",
                    channel_id, ep.id, e
                )
            })?;
            match self.update_models_from_endpoint(&url, &api_key).await {
                Ok(count) => ep_results.push(EndpointHealth {
                    url: ep.url.clone(),
                    reachable: true,
                    models_found: count,
                    error: None,
                }),
                Err(e) => ep_results.push(EndpointHealth {
                    url: ep.url.clone(),
                    reachable: false,
                    models_found: 0,
                    error: Some(e),
                }),
            }
        }

        Ok(ChannelHealthResult {
            channel_id: channel_id.to_string(),
            endpoints: ep_results,
        })
    }

    /// Query all channel endpoints' /v1/models and update context_length for matching models.
    /// Returns (total_models_updated, channel_count_checked, channels_with_failures).
    pub async fn check_all_channels(&self) -> Result<(usize, usize, usize), String> {
        let channels = self.db.list_channels().await.map_err(|e| e.0)?;
        let mut total_updated = 0;
        let mut channels_checked = 0;
        let mut channels_failed = 0;

        for ch in &channels {
            if !ch.enabled || ch.endpoints.is_empty() {
                continue;
            }
            let mut has_failure = false;
            for ep in &ch.endpoints {
                let base = ep.url.trim_end_matches('/').trim_end_matches("/v1");
                let url = format!("{}/v1/models", base);
                let api_key =
                    crate::crypto::decrypt_load(&ep.api_key, &self.enc_key).map_err(|e| {
                        format!(
                            "Failed to decrypt API key for channel '{}' endpoint {:?}: {}",
                            ch.id, ep.id, e
                        )
                    })?;
                match self.update_models_from_endpoint(&url, &api_key).await {
                    Ok(updated) => total_updated += updated,
                    Err(e) => {
                        tracing::warn!("Health check failed for {}: {}", url, e);
                        has_failure = true;
                    }
                }
            }
            channels_checked += 1;
            if has_failure {
                channels_failed += 1;
            }
        }

        Ok((total_updated, channels_checked, channels_failed))
    }

    /// Fetch raw upstream model list from a channel's endpoint.
    /// Returns deduplicated model info, keeping the max max_model_len across endpoints.
    pub async fn list_upstream_models(&self, channel_id: &str) -> Result<Vec<UpstreamModelInfo>, String> {
        let channels = self.db.list_channels().await.map_err(|e| e.0)?;
        let ch = channels.iter()
            .find(|c| c.id == channel_id)
            .ok_or_else(|| format!("Channel '{}' not found", channel_id))?;

        let mut seen: std::collections::HashMap<String, Option<i64>> = std::collections::HashMap::new();
        for ep in &ch.endpoints {
            let base = ep.url.trim_end_matches('/').trim_end_matches("/v1");
            let url = format!("{}/v1/models", base);
            let api_key = crate::crypto::decrypt_load(&ep.api_key, &self.enc_key).map_err(|e| {
                format!(
                    "Failed to decrypt API key for channel '{}' endpoint {:?}: {}",
                    channel_id, ep.id, e
                )
            })?;
            match self.fetch_upstream_models(&url, &api_key).await {
                Ok(models) => {
                    for m in models {
                        let len = m.max_model_len;
                        seen.entry(m.id.clone())
                            .and_modify(|existing| {
                                if let (Some(e), Some(n)) = (existing.as_mut(), len) {
                                    if n > *e {
                                        *e = n;
                                    }
                                }
                            })
                            .or_insert(len);
                    }
                }
                Err(e) => tracing::warn!("Failed to fetch models from {}: {}", url, e),
            }
        }

        let mut result: Vec<UpstreamModelInfo> = seen
            .into_iter()
            .map(|(id, max_model_len)| UpstreamModelInfo { id, max_model_len })
            .collect();
        result.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(result)
    }

    /// Fetch /v1/models from a single endpoint and return raw upstream models.
    async fn fetch_upstream_models(&self, url: &str, api_key: &str) -> Result<Vec<UpstreamModel>, String> {
        let mut req = self.client.get(url);
        if !api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", api_key));
        }
        let resp = req.send().await.map_err(|e| format!("HTTP request failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let body: UpstreamModelsResponse = resp.json().await.map_err(|e| format!("JSON parse failed: {}", e))?;
        Ok(body.data)
    }

    /// Query a single endpoint /v1/models and update context_length for matching models.
    /// Returns number of models whose context_length was updated.
    async fn update_models_from_endpoint(&self, url: &str, api_key: &str) -> Result<usize, String> {
        let body = self.fetch_upstream_models(url, api_key).await?;

        // Get all gateway models to match against
        let models = self.db.list_models().await.map_err(|e| e.0)?;

        let mut updated = 0;
        for upstream in &body {
            let Some(len) = upstream.max_model_len else { continue };

            // Match upstream model ID against gateway model_patterns
            for m in &models {
                if match_pattern(&upstream.id, &m.model_pattern) {
                    let current = m.context_length.unwrap_or(0);
                    if len > current {
                        self.db.set_model_context_length(&m.id, len).await.map_err(|e| e.0)?;
                        updated += 1;
                    }
                }
            }
        }

        Ok(updated)
    }
}
