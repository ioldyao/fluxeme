use std::sync::Arc;
use std::time::Duration;

use crate::ch_backend::{ClickHouseBackend, UsageEvent};
use crate::db::Database;

/// Compensation task: periodically scans `usage_billing` for records not yet
/// written to ClickHouse (`written_to_ch = false`), converts them to
/// `UsageEvent`, batch-inserts into ClickHouse, and marks them as written.
///
/// Runs every 30 seconds. When ClickHouse is disabled (`ch` is `None`),
/// the task silently returns — no data is lost because `usage_billing`
/// retains the records until ClickHouse comes online.
pub async fn start_compensation_loop(
    ch: Option<Arc<ClickHouseBackend>>,
    db: Arc<Database>,
) {
    let ch = match ch {
        Some(c) => c,
        None => {
            tracing::info!("ClickHouse disabled — compensation task skipped");
            return;
        }
    };

    tracing::info!("Compensation task started (every 30s)");
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;

        let records = match db.find_pending_usage_billing(500).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Compensation: find_pending_usage_billing failed: {e}");
                continue;
            }
        };

        if records.is_empty() {
            continue;
        }

        let events: Vec<UsageEvent> = records
            .iter()
            .map(|r| {
                let total_tokens = if r.total_tokens > 0 {
                    r.total_tokens as u64
                } else {
                    (r.prompt_tokens.max(0) + r.completion_tokens.max(0)) as u64
                };
                let cost_amount = r.prompt_tokens.max(0) as f64 / 1_000_000.0 * r.prompt_price
                    + r.completion_tokens.max(0) as f64 / 1_000_000.0 * r.completion_price
                    + r.cache_hit_input_tokens.max(0) as f64 / 1_000_000.0 * r.cache_read_price;
                UsageEvent {
                    timestamp: r.timestamp.clone(),
                    request_id: r.request_id.clone(),
                    user_id: r.user_id.clone(),
                    user_name: r.user_name.clone(),
                    channel_id: r.channel_id.clone(),
                    model: r.model.clone(),
                    prompt_tokens: r.prompt_tokens.max(0) as u64,
                    completion_tokens: r.completion_tokens.max(0) as u64,
                    total_tokens,
                    latency_ms: r.latency_ms.max(0) as u64,
                    status_code: r.status_code.max(0) as u16,
                    success: r.success as u8,
                    api_key_name: r.api_key_name.clone(),
                    api_format: r.api_format.clone(),
                    stream: r.stream as u8,
                    cache_hit_input_tokens: r.cache_hit_input_tokens.max(0) as u64,
                    cost_amount,
                    client_ip: r.client_ip.clone(),
                    endpoint_id: r.endpoint_id,
                }
            })
            .collect();

        let request_ids: Vec<String> = records.iter().map(|r| r.request_id.clone()).collect();

        match ch.insert_usage_events(&events).await {
            Ok(()) => {
                if let Err(e) = db.mark_usage_billing_written(&request_ids).await {
                    tracing::error!(
                        count = records.len(),
                        error = %e.0,
                        "Compensation: mark_usage_billing_written failed"
                    );
                } else {
                    tracing::info!(
                        count = records.len(),
                        "Compensation: wrote {} events to ClickHouse",
                        events.len()
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    count = records.len(),
                    error = e,
                    "Compensation: ClickHouse insert failed — retry next cycle"
                );
            }
        }
    }
}
