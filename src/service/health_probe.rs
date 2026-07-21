use std::sync::Arc;
use std::time::Instant;

use uuid::Uuid;

use crate::db::{Database, ProbeResultRow};
use crate::provider::ProviderRegistry;
use crate::service::routing::RoutingService;

/// Unified health probe service.
///
/// Sends real chat completion requests to each channel binding of a model,
/// records success/latency in the database for persistence across restarts.
pub struct HealthProbeService {
    db: Arc<Database>,
    providers: Arc<ProviderRegistry>,
    routing: Arc<RoutingService>,
}

impl HealthProbeService {
    pub fn new(
        db: Arc<Database>,
        providers: Arc<ProviderRegistry>,
        routing: Arc<RoutingService>,
    ) -> Self {
        Self { db, providers, routing }
    }

    /// Probe all channel bindings for a model and return per-channel results.
    pub async fn probe_model(&self, model_id: &str) -> Result<Vec<ProbeResultRow>, String> {
        let model = self
            .db
            .get_model(model_id)
            .await
            .map_err(|e| e.0)?
            .ok_or_else(|| format!("Model '{}' not found", model_id))?;

        let all_channels = self.db.list_channels().await.map_err(|e| e.0)?;
        let channel_map: std::collections::HashMap<_, _> = all_channels
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();

        let mut bindings = model.channels.clone();
        bindings.sort_by_key(|b| b.priority);
        let mut results = Vec::new();

        for binding in &bindings {
            let _ch_name = channel_map
                .get(&binding.channel_id)
                .map(|c| if c.name.is_empty() { c.id.clone() } else { c.name.clone() })
                .unwrap_or_else(|| binding.channel_id.clone());

            let route = match self.routing.get_route(&binding.channel_id) {
                Some(r) => r,
                None => {
                    let row = self.make_row(&binding.channel_id, model_id, false, 0, Some("Route not available"));
                    self.db.insert_probe_result(&row).await.map_err(|e| e.0)?;
                    results.push(row);
                    continue;
                }
            };
            let provider_name = route.0.clone();
            let adapter = match self.providers.get(&provider_name) {
                Some(a) => a,
                None => {
                    let row = self.make_row(&binding.channel_id, model_id, false, 0, Some("Provider adapter not found"));
                    self.db.insert_probe_result(&row).await.map_err(|e| e.0)?;
                    results.push(row);
                    continue;
                }
            };
            let (endpoint_idx, endpoint) = match route.1.as_health_aware().select() {
                Some(r) => r,
                None => {
                    let row = self.make_row(&binding.channel_id, model_id, false, 0, Some("No available endpoints"));
                    self.db.insert_probe_result(&row).await.map_err(|e| e.0)?;
                    results.push(row);
                    continue;
                }
            };

            let test_body = serde_json::json!({
                "model": model.id,
                "messages": [{"role": "user", "content": "hi"}],
                "temperature": 0.01,
                "max_tokens": 512,
                "top_p": 0.01,
                "stream": false,
            });

            let start = Instant::now();
            let result = if provider_name == "anthropic" {
                let body = serde_json::json!({
                    "model": model.id,
                    "messages": [{"role": "user", "content": "hi"}],
                    "max_tokens": 512,
                });
                adapter.messages(endpoint, body).await
            } else {
                adapter.chat_complete(endpoint, test_body).await
            };
            let latency_ms = start.elapsed().as_millis() as u64;

            match result {
                Ok(_) => {
                    route.1.as_health_aware().record_success(endpoint_idx);
                    let row = self.make_row(&binding.channel_id, model_id, true, latency_ms, None);
                    self.db.insert_probe_result(&row).await.map_err(|e| e.0)?;
                    results.push(row);
                }
                Err(e) => {
                    route.1.as_health_aware().record_failure(endpoint_idx);
                    let row = self.make_row(&binding.channel_id, model_id, false, latency_ms, Some(&e.0));
                    self.db.insert_probe_result(&row).await.map_err(|e| e.0)?;
                    results.push(row);
                }
            }
        }

        Ok(results)
    }

    /// Get the most recent probe result for each channel.
    pub async fn all_latest_probes(&self) -> Result<Vec<ProbeResultRow>, String> {
        self.db.all_latest_probe_results().await.map_err(|e| e.0)
    }

    fn make_row(
        &self,
        channel_id: &str,
        model_id: &str,
        success: bool,
        latency_ms: u64,
        error: Option<&str>,
    ) -> ProbeResultRow {
        ProbeResultRow {
            id: Uuid::new_v4().to_string(),
            channel_id: channel_id.to_string(),
            model_id: model_id.to_string(),
            success,
            latency_ms,
            error: error.map(|s| s.to_string()),
            probed_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}
