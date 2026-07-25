use std::sync::Arc;

use clickhouse::Client;
use clickhouse::Row;
use serde::Serialize;

use crate::config::types::ClickHouseConfig;

/// Row type for the `usage_events` ClickHouse table.
/// Contains observability fields plus pre-computed cost_amount.
/// Billing price snapshots remain in PG's usage_billing table.
#[derive(Debug, Clone, Serialize, Row)]
pub struct UsageEvent {
    pub timestamp: String,
    pub request_id: String,
    pub user_id: String,
    pub user_name: String,
    pub channel_id: String,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub latency_ms: u64,
    pub status_code: u16,
    pub success: u8,
    pub api_key_name: Option<String>,
    pub api_format: String,
    pub stream: u8,
    pub cache_hit_input_tokens: u64,
    pub cost_amount: f64,
    pub client_ip: Option<String>,
    pub endpoint_id: Option<i64>,
}

/// ClickHouse backend for observability data.
/// Handles writes and queries for the usage_events table.
pub struct ClickHouseBackend {
    client: Client,
}

impl ClickHouseBackend {
    /// Create a new ClickHouse client. Returns None when the config is empty
    /// (ClickHouse disabled) or connection fails.
    pub async fn new(cfg: &ClickHouseConfig) -> Result<Option<Arc<Self>>, String> {
        if cfg.host.is_empty() {
            return Ok(None);
        }

        let url = format!("http://{}:{}", cfg.host, cfg.port + 1); // HTTP port = TCP + 1
        let client = Client::default()
            .with_url(&url)
            .with_user(&cfg.user)
            .with_password(&cfg.password)
            .with_database(&cfg.db);

        // Verify connectivity
        match client.query("SELECT 1").fetch_all::<u8>().await {
            Ok(_) => {
                tracing::info!("ClickHouse connected at {}", url);
                Ok(Some(Arc::new(Self { client })))
            }
            Err(e) => {
                tracing::warn!(
                    "ClickHouse not available at {url}: {e}. Observability features disabled."
                );
                Ok(None)
            }
        }
    }

    // ── Schema ────────────────────────────────────────────────────────

    /// DDL for the usage_events table.
    ///
    /// ORDER BY (model, channel_id, timestamp) aligns with the most
    /// frequent aggregation patterns: routing flow snapshot rates model +
    /// channel, health panel groups by channel + model, time-series
    /// queries filter by timestamp.
    ///
    /// PARTITION BY toYYYYMM(timestamp) gives monthly partitions for
    /// efficient TTL-based retention.
    const CREATE_USAGE_EVENTS: &'static str = "\
        CREATE TABLE IF NOT EXISTS usage_events (\
            timestamp DateTime,\
            request_id String,\
            user_id String,\
            user_name String,\
            channel_id String,\
            model String,\
            prompt_tokens UInt64,\
            completion_tokens UInt64,\
            total_tokens UInt64,\
            latency_ms UInt64,\
            status_code UInt16,\
            success UInt8,\
            api_key_name Nullable(String),\
            api_format String,\
            stream UInt8,\
            cache_hit_input_tokens UInt64,\
            cost_amount Float64,\
            client_ip Nullable(String),\
            endpoint_id Nullable(Int64)\
        ) ENGINE = MergeTree()\
        PARTITION BY toYYYYMM(timestamp)\
        ORDER BY (model, channel_id, timestamp)\
        TTL toDateTime(timestamp) + INTERVAL 90 DAY\
        SETTINGS index_granularity = 8192\
    ";

    const CREATE_PROBE_RESULTS: &'static str = "\
        CREATE TABLE IF NOT EXISTS probe_results (\
            id String,\
            channel_id String,\
            model_id String,\
            success UInt8,\
            latency_ms UInt64,\
            error Nullable(String),\
            endpoint_url Nullable(String),\
            probed_at DateTime\
        ) ENGINE = MergeTree()\
        PARTITION BY toYYYYMM(probed_at)\
        ORDER BY (channel_id, model_id, probed_at)\
        TTL toDateTime(probed_at) + INTERVAL 30 DAY\
    ";

    /// Run migrations (idempotent).
    pub async fn migrate(&self, retention_days: u32) -> Result<(), String> {
        self.client
            .query(Self::CREATE_USAGE_EVENTS)
            .execute()
            .await
            .map_err(|e| format!("CH migration usage_events: {e}"))?;

        self.client
            .query(Self::CREATE_PROBE_RESULTS)
            .execute()
            .await
            .map_err(|e| format!("CH migration probe_results: {e}"))?;

        // Update TTL to match config
        let ttl_sql = format!(
            "ALTER TABLE usage_events MODIFY TTL toDateTime(timestamp) + INTERVAL {} DAY",
            retention_days
        );
        let _ = self.client.query(&ttl_sql).execute().await;

        tracing::info!("ClickHouse schema up to date");
        Ok(())
    }

    // ── Writes ────────────────────────────────────────────────────────

    /// Insert a single usage event into ClickHouse.
    pub async fn insert_usage_event(&self, event: &UsageEvent) -> Result<(), String> {
        let mut inserter = self
            .client
            .insert::<UsageEvent>("usage_events")
            .map_err(|e| format!("CH insert: {e}"))?;
        inserter.write(event).await.map_err(|e| format!("CH write: {e}"))?;
        inserter.end().await.map_err(|e| format!("CH end: {e}"))
    }

    /// Batch insert usage events.
    pub async fn insert_usage_events(&self, events: &[UsageEvent]) -> Result<(), String> {
        if events.is_empty() {
            return Ok(());
        }
        let mut inserter = self
            .client
            .insert::<UsageEvent>("usage_events")
            .map_err(|e| format!("CH inserter: {e}"))?;
        for event in events {
            inserter
                .write(event)
                .await
                .map_err(|e| format!("CH insert row: {e}"))?;
        }
        inserter
            .end()
            .await
            .map_err(|e| format!("CH insert batch end: {e}"))
    }

    // ── Raw query access (used by Phase 8 query migration) ───────────

    /// Returns a reference to the underlying ClickHouse client for ad-hoc queries.
    pub fn client(&self) -> &Client {
        &self.client
    }
}
