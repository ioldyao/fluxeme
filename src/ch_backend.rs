use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Timelike};
use clickhouse::Client;
use clickhouse::Row;
use rust_decimal::Decimal;
use serde::Serialize;

use crate::admin::routing::{
    FlowMetricsClientIp, FlowMetricsHistorical, FlowMetricsModelShare, FlowMetricsPercentiles,
    FlowMetricsTrend,
};
use crate::config::types::ClickHouseConfig;

/// Row type for the `usage_events` ClickHouse table.
/// Contains observability fields plus pre-computed cost_amount.
/// Billing price snapshots remain in PG's usage_billing table.
#[derive(Debug, Clone, Serialize, Row)]
pub struct UsageEvent {
    pub timestamp: u32,
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
    pub cache_write_tokens: u64,
    pub prompt_price: f64,
    pub completion_price: f64,
    pub cache_read_price: f64,
    pub cache_write_price: f64,
    pub cost_amount: f64,
    pub client_ip: Option<String>,
    pub endpoint_id: Option<i64>,
    pub endpoint_url: Option<String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub reasoning_body: Option<String>,
    pub original_model: String,
    /// Team scope for this usage event. Empty string = personal (non-team).
    #[serde(default)]
    pub team_id: String,
    /// Time to first upstream response data for streaming requests.
    #[serde(default)]
    pub ttft_ms: Option<u64>,
    pub billing_group_id: Option<String>,
    pub billing_group_name: Option<String>,
    pub billing_payment_mode: String,
}

/// ClickHouse backend for observability data.
/// Handles writes and queries for the usage_events table.
/// Row type for the `skill_runtime_calls` ClickHouse table.
/// Skill Runtime 数据面每次调用的可观测记录（高吞吐 append-only → CH）。
/// 财务事实（钱包/账单）不在此表，永远以 PG billing_events 为准。
#[derive(Debug, Clone, Serialize, Row)]
pub struct SkillRuntimeCall {
    pub timestamp: u32,
    pub request_id: String,
    pub skill_id: String,
    pub slug: String,
    pub version: String,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub latency_ms: u64,
    pub user_id: String,
    pub api_key_id: String,
}

/// Row type for the `gateway_calls` ClickHouse table.
/// API 网关（纯 API 网关数据面）每次调用的可观测记录（高吞吐 append-only → CH）。
/// 财务事实不在此表；业务配置在 PG `gateway_routes`。
#[derive(Debug, Clone, Serialize, Row)]
pub struct GatewayCall {
    pub timestamp: u32,
    pub request_id: String,
    pub route_id: String,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub latency_ms: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub user_id: String,
    pub api_key_id: String,
}

pub struct ClickHouseBackend {
    client: Client,
}

#[derive(Debug, Clone, serde::Deserialize, Row)]
struct UsageEventRow {
    timestamp: String,
    request_id: String,
    user_id: String,
    user_name: String,
    channel_id: String,
    model: String,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    latency_ms: u64,
    status_code: u16,
    success: u8,
    api_key_name: Option<String>,
    api_format: String,
    stream: u8,
    cache_hit_input_tokens: u64,
    cache_write_tokens: u64,
    prompt_price: f64,
    completion_price: f64,
    cache_read_price: f64,
    cache_write_price: f64,
    client_ip: Option<String>,
    endpoint_id: Option<i64>,
    endpoint_url: Option<String>,
    original_model: String,
    team_id: String,
    ttft_ms: Option<u64>,
    billing_group_id: Option<String>,
    billing_group_name: Option<String>,
    billing_payment_mode: String,
}

/// Row type for the usage-detail query — includes the request/response bodies
/// that the list queries deliberately omit for payload size.
#[derive(Debug, Clone, serde::Deserialize, Row)]
struct UsageDetailRow {
    timestamp: String,
    request_id: String,
    user_id: String,
    user_name: String,
    channel_id: String,
    model: String,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    latency_ms: u64,
    status_code: u16,
    success: u8,
    api_key_name: Option<String>,
    api_format: String,
    stream: u8,
    cache_hit_input_tokens: u64,
    cache_write_tokens: u64,
    prompt_price: f64,
    completion_price: f64,
    cache_read_price: f64,
    cache_write_price: f64,
    client_ip: Option<String>,
    endpoint_id: Option<i64>,
    endpoint_url: Option<String>,
    original_model: String,
    team_id: String,
    ttft_ms: Option<u64>,
    request_body: Option<String>,
    response_body: Option<String>,
    reasoning_body: Option<String>,
    billing_group_id: Option<String>,
    billing_group_name: Option<String>,
    billing_payment_mode: String,
}

impl From<UsageEventRow> for crate::domain::usage::UsageRecord {
    fn from(row: UsageEventRow) -> Self {
        Self {
            timestamp: row.timestamp,
            request_id: row.request_id,
            user_id: row.user_id,
            user_name: row.user_name,
            channel_id: row.channel_id,
            model: row.model,
            prompt_tokens: row.prompt_tokens,
            completion_tokens: row.completion_tokens,
            total_tokens: row
                .prompt_tokens
                .saturating_add(row.cache_hit_input_tokens)
                .saturating_add(row.completion_tokens),
            latency_ms: row.latency_ms,
            status_code: row.status_code,
            success: row.success != 0,
            request_body: None,
            response_body: None,
            reasoning_body: None,
            api_key_name: row.api_key_name,
            api_format: row.api_format,
            stream: row.stream != 0,
            cache_hit_input_tokens: row.cache_hit_input_tokens,
            cache_write_tokens: row.cache_write_tokens,
            prompt_price: Decimal::try_from(row.prompt_price).unwrap_or(Decimal::ZERO),
            completion_price: Decimal::try_from(row.completion_price).unwrap_or(Decimal::ZERO),
            cache_read_price: Decimal::try_from(row.cache_read_price).unwrap_or(Decimal::ZERO),
            cache_write_price: Decimal::try_from(row.cache_write_price).unwrap_or(Decimal::ZERO),
            client_ip: row.client_ip,
            endpoint_id: row.endpoint_id,
            endpoint_url: row.endpoint_url,
            original_model: row.original_model,
            team_id: (!row.team_id.is_empty()).then_some(row.team_id),
            ttft_ms: row.ttft_ms,
            account_type: None,
            billing_group_id: row.billing_group_id,
            billing_group_name: row.billing_group_name,
            billing_payment_mode: Some(row.billing_payment_mode.clone()),
        }
    }
}

impl From<UsageDetailRow> for crate::domain::usage::UsageRecord {
    fn from(row: UsageDetailRow) -> Self {
        Self {
            timestamp: row.timestamp,
            request_id: row.request_id,
            user_id: row.user_id,
            user_name: row.user_name,
            channel_id: row.channel_id,
            model: row.model,
            prompt_tokens: row.prompt_tokens,
            completion_tokens: row.completion_tokens,
            total_tokens: row
                .prompt_tokens
                .saturating_add(row.cache_hit_input_tokens)
                .saturating_add(row.completion_tokens),
            latency_ms: row.latency_ms,
            status_code: row.status_code,
            success: row.success != 0,
            request_body: row.request_body,
            response_body: row.response_body,
            reasoning_body: row.reasoning_body,
            api_key_name: row.api_key_name,
            api_format: row.api_format,
            stream: row.stream != 0,
            cache_hit_input_tokens: row.cache_hit_input_tokens,
            cache_write_tokens: row.cache_write_tokens,
            prompt_price: Decimal::try_from(row.prompt_price).unwrap_or(Decimal::ZERO),
            completion_price: Decimal::try_from(row.completion_price).unwrap_or(Decimal::ZERO),
            cache_read_price: Decimal::try_from(row.cache_read_price).unwrap_or(Decimal::ZERO),
            cache_write_price: Decimal::try_from(row.cache_write_price).unwrap_or(Decimal::ZERO),
            client_ip: row.client_ip,
            endpoint_id: row.endpoint_id,
            endpoint_url: row.endpoint_url,
            original_model: row.original_model,
            team_id: (!row.team_id.is_empty()).then_some(row.team_id),
            ttft_ms: row.ttft_ms,
            account_type: None,
            billing_group_id: row.billing_group_id,
            billing_group_name: row.billing_group_name,
            billing_payment_mode: Some(row.billing_payment_mode.clone()),
        }
    }
}

pub(crate) fn normalize_clickhouse_datetime(value: &str) -> Result<String, String> {
    if let Ok(datetime) = DateTime::parse_from_rfc3339(value) {
        return Ok(datetime.to_rfc3339());
    }

    if let Ok(datetime) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Ok(datetime.format("%Y-%m-%d %H:%M:%S").to_string());
    }

    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Ok(date
            .and_hms_opt(0, 0, 0)
            .expect("midnight is always valid")
            .format("%Y-%m-%d %H:%M:%S")
            .to_string());
    }

    Err("invalid datetime filter".to_string())
}

fn flow_metrics_bucket_granularity(start: &str, end: &str) -> Result<(&'static str, i64), String> {
    let start_dt =
        DateTime::parse_from_rfc3339(start).map_err(|_| "invalid start datetime".to_string())?;
    let end_dt =
        DateTime::parse_from_rfc3339(end).map_err(|_| "invalid end datetime".to_string())?;
    let seconds = (end_dt - start_dt).num_seconds();
    if seconds <= 3600 {
        Ok(("minute", 60))
    } else {
        Ok(("hour", 3600))
    }
}

impl ClickHouseBackend {
    /// Run a sanitized connectivity check without exposing ClickHouse errors.
    pub async fn ping(&self) -> bool {
        self.client
            .query("SELECT 1")
            .fetch_all::<u8>()
            .await
            .is_ok()
    }

    /// Create a new ClickHouse client. Returns None when the config is empty
    /// (ClickHouse disabled) or connection fails.
    pub async fn new(cfg: &ClickHouseConfig) -> Result<Option<Arc<Self>>, String> {
        if cfg.host.is_empty() {
            return Ok(None);
        }

        let url = format!("http://{}:{}", cfg.host, cfg.port);
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
            cache_write_tokens UInt64,\
            prompt_price Float64,\
            completion_price Float64,\
            cache_read_price Float64,\
            cache_write_price Float64 DEFAULT 0,\
            cost_amount Float64,\
            client_ip Nullable(String),\
            endpoint_id Nullable(Int64),\
            request_body Nullable(String),\
            response_body Nullable(String),\
            reasoning_body Nullable(String),\
            original_model String,\
            team_id String DEFAULT '',\
            billing_group_id Nullable(String),\
            billing_group_name Nullable(String),\
            billing_payment_mode String DEFAULT 'metered'\
        ) ENGINE = MergeTree()\
        PARTITION BY toYYYYMM(timestamp)\
        ORDER BY (model, channel_id, timestamp)\
        TTL toDateTime(timestamp) + INTERVAL 90 DAY \
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
    const CREATE_SKILL_RUNTIME_CALLS: &'static str = "\
        CREATE TABLE IF NOT EXISTS skill_runtime_calls (\
            timestamp DateTime,\
            request_id String,\
            skill_id String,\
            slug String,\
            version String,\
            method String,\
            path String,\
            status_code UInt16,\
            latency_ms UInt64,\
            user_id String,\
            api_key_id String\
        ) ENGINE = MergeTree()\
        PARTITION BY toYYYYMM(timestamp)\
        ORDER BY (slug, timestamp)\
        TTL toDateTime(timestamp) + INTERVAL 90 DAY \
        SETTINGS index_granularity = 8192\
    ";

    /// API 网关数据面调用观测（纯 API 网关，非 AI 流量）。
    const CREATE_GATEWAY_CALLS: &'static str = "\
        CREATE TABLE IF NOT EXISTS gateway_calls (\
            timestamp DateTime,\
            request_id String,\
            route_id String,\
            method String,\
            path String,\
            status_code UInt16,\
            latency_ms UInt64,\
            bytes_in UInt64,\
            bytes_out UInt64,\
            user_id String,\
            api_key_id String\
        ) ENGINE = MergeTree()\
        PARTITION BY toYYYYMM(timestamp)\
        ORDER BY (route_id, timestamp)\
        TTL toDateTime(timestamp) + INTERVAL 90 DAY \
        SETTINGS index_granularity = 8192\
    ";

    pub async fn migrate(&self, retention_days: u32) -> Result<(), String> {
        self.client
            .query(Self::CREATE_USAGE_EVENTS)
            .execute()
            .await
            .map_err(|e| format!("CH migration usage_events: {e}"))?;

        for alter in [
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS prompt_price Float64",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS completion_price Float64",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS cache_read_price Float64",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS cache_write_price Float64 DEFAULT 0",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS request_body Nullable(String)",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS response_body Nullable(String)",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS reasoning_body Nullable(String)",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS original_model String DEFAULT ''",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS endpoint_url Nullable(String)",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS cache_write_tokens UInt64 DEFAULT 0",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS team_id String DEFAULT ''",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS ttft_ms Nullable(UInt64)",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS billing_group_id Nullable(String)",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS billing_group_name Nullable(String)",
            "ALTER TABLE usage_events ADD COLUMN IF NOT EXISTS billing_payment_mode String DEFAULT 'metered'",
            "ALTER TABLE usage_events UPDATE billing_payment_mode = if(billing_payment_mode = 'postpaid', 'prepaid', if(billing_payment_mode = 'prepaid', 'metered', billing_payment_mode)) WHERE billing_payment_mode IN ('postpaid', 'prepaid')",
        ] {
            self.client
                .query(alter)
                .execute()
                .await
                .map_err(|e| format!("CH migration usage_events alter: {e}"))?;
        }

        self.client
            .query(Self::CREATE_PROBE_RESULTS)
            .execute()
            .await
            .map_err(|e| format!("CH migration probe_results: {e}"))?;

        self.client
            .query(Self::CREATE_SKILL_RUNTIME_CALLS)
            .execute()
            .await
            .map_err(|e| format!("CH migration skill_runtime_calls: {e}"))?;

        self.client
            .query(Self::CREATE_GATEWAY_CALLS)
            .execute()
            .await
            .map_err(|e| format!("CH migration gateway_calls: {e}"))?;

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

    /// Batch insert health-probe results (observability → ClickHouse).
    pub async fn insert_probe_results(
        &self,
        rows: &[crate::db::ProbeResultRow],
    ) -> Result<(), String> {
        if rows.is_empty() {
            return Ok(());
        }
        #[derive(clickhouse::Row, serde::Serialize)]
        struct ChProbeRow {
            id: String,
            channel_id: String,
            model_id: String,
            success: u8,
            latency_ms: u64,
            error: Option<String>,
            endpoint_url: Option<String>,
            probed_at: u32,
        }
        let mut inserter = self
            .client
            .insert::<ChProbeRow>("probe_results")
            .map_err(|e| format!("CH probe inserter: {e}"))?;
        for r in rows {
            let ts = chrono::DateTime::parse_from_rfc3339(&r.probed_at)
                .map(|d| d.timestamp() as u32)
                .unwrap_or_else(|_| chrono::Utc::now().timestamp() as u32);
            inserter
                .write(&ChProbeRow {
                    id: r.id.clone(),
                    channel_id: r.channel_id.clone(),
                    model_id: r.model_id.clone(),
                    success: if r.success { 1 } else { 0 },
                    latency_ms: r.latency_ms,
                    error: r.error.clone(),
                    endpoint_url: r.endpoint_url.clone(),
                    probed_at: ts,
                })
                .await
                .map_err(|e| format!("CH probe insert row: {e}"))?;
        }
        inserter
            .end()
            .await
            .map_err(|e| format!("CH probe insert batch end: {e}"))
    }

    /// Latest probe result per (model, channel, endpoint_url) from ClickHouse.
    pub async fn all_latest_probe_results(&self) -> Result<Vec<crate::db::ProbeResultRow>, String> {
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct Row {
            id: String,
            channel_id: String,
            model_id: String,
            success: u8,
            latency_ms: u64,
            error: Option<String>,
            endpoint_url: Option<String>,
            probed_at: u32,
        }
        // Use ROW_NUMBER() with id as tiebreaker so that two probes for the
        // same (model, channel, url) within the same second return exactly
        // one row – the latest by probed_at then by id.
        let rows = self
            .client
            .query(
                "SELECT id, channel_id, model_id, success, latency_ms, error, endpoint_url, probed_at \
                 FROM ( \
                   SELECT id, channel_id, model_id, success, latency_ms, error, endpoint_url, \
                          toUInt32(probed_at) AS probed_at, \
                          ROW_NUMBER() OVER ( \
                            PARTITION BY model_id, channel_id, COALESCE(endpoint_url, '') \
                            ORDER BY probed_at DESC, id DESC \
                          ) AS rn \
                   FROM probe_results \
                 ) WHERE rn = 1 \
                 ORDER BY probed_at DESC",
            )
            .fetch_all::<Row>()
            .await
            .map_err(|e| format!("CH all_latest_probe_results: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| crate::db::ProbeResultRow {
                id: r.id,
                channel_id: r.channel_id,
                model_id: r.model_id,
                success: r.success != 0,
                latency_ms: r.latency_ms,
                error: r.error,
                probed_at: chrono::DateTime::from_timestamp(r.probed_at as i64, 0)
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_default(),
                endpoint_url: r.endpoint_url,
            })
            .collect())
    }

    /// Raw probe results from the last `minutes` minutes (newest first).
    pub async fn recent_probe_results(
        &self,
        minutes: i64,
    ) -> Result<Vec<crate::db::ProbeResultRow>, String> {
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct Row {
            id: String,
            channel_id: String,
            model_id: String,
            success: u8,
            latency_ms: u64,
            error: Option<String>,
            endpoint_url: Option<String>,
            probed_at: u32,
        }
        let rows = self
            .client
            .query(
                "SELECT id, channel_id, model_id, success, latency_ms, error, endpoint_url, toUInt32(probed_at) AS probed_at \
                 FROM probe_results \
                 WHERE probed_at >= now() - INTERVAL ? MINUTE \
                 ORDER BY probed_at DESC LIMIT 1000",
            )
            .bind(minutes as u32)
            .fetch_all::<Row>()
            .await
            .map_err(|e| format!("CH recent_probe_results: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| crate::db::ProbeResultRow {
                id: r.id,
                channel_id: r.channel_id,
                model_id: r.model_id,
                success: r.success != 0,
                latency_ms: r.latency_ms,
                error: r.error,
                probed_at: chrono::DateTime::from_timestamp(r.probed_at as i64, 0)
                    .map(|d| d.to_rfc3339())
                    .unwrap_or_default(),
                endpoint_url: r.endpoint_url,
            })
            .collect())
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

    /// Batch insert skill-runtime call records (observability → ClickHouse).
    pub async fn insert_skill_runtime_calls(
        &self,
        rows: &[SkillRuntimeCall],
    ) -> Result<(), String> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut inserter = self
            .client
            .insert::<SkillRuntimeCall>("skill_runtime_calls")
            .map_err(|e| format!("CH inserter: {e}"))?;
        for row in rows {
            inserter
                .write(row)
                .await
                .map_err(|e| format!("CH insert row: {e}"))?;
        }
        inserter
            .end()
            .await
            .map_err(|e| format!("CH insert batch end: {e}"))
    }

    /// Batch insert API-gateway call records (observability → ClickHouse).
    pub async fn insert_gateway_calls(&self, rows: &[GatewayCall]) -> Result<(), String> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut inserter = self
            .client
            .insert::<GatewayCall>("gateway_calls")
            .map_err(|e| format!("CH inserter: {e}"))?;
        for row in rows {
            inserter
                .write(row)
                .await
                .map_err(|e| format!("CH insert row: {e}"))?;
        }
        inserter
            .end()
            .await
            .map_err(|e| format!("CH insert batch end: {e}"))
    }

    // ── Raw query access ─────────────────────────────────────────────

    // ── Observability queries (Phase 8, routed from admin handlers) ──

    /// 24h channel usage: (channel_id, model, requests, successes, avg_latency, p95).
    pub async fn query_channel_usage_24h(
        &self,
        published_models: &[String],
    ) -> Result<Vec<(String, String, u64, u64, f64, f64)>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct ChUsageRow {
            channel_id: String,
            model: String,
            requests: u64,
            successes: u64,
            avg_latency: f64,
            p95_latency: f64,
        }
        let rows = self
            .client
            .query(
                "SELECT channel_id, model, \
                 count()::UInt64 AS requests, \
                 countIf(success = 1)::UInt64 AS successes, \
                 avg(latency_ms)::Float64 AS avg_latency, \
                 quantileExact(0.95)(latency_ms)::Float64 AS p95_latency \
                 FROM usage_events \
                 WHERE timestamp >= now() - INTERVAL 24 HOUR \
                   AND has(?, model) \
                 GROUP BY channel_id, model \
                 ORDER BY requests DESC",
            )
            .bind(published_models)
            .fetch_all::<ChUsageRow>()
            .await
            .map_err(|e| format!("CH channel_usage_24h: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.channel_id,
                    r.model,
                    r.requests,
                    r.successes,
                    r.avg_latency,
                    r.p95_latency,
                )
            })
            .collect())
    }

    /// 24h channel usage across all observed models. Observability-only query;
    /// callers must apply any presentation filtering after the query.
    pub async fn query_channel_usage_24h_all(
        &self,
    ) -> Result<Vec<(String, String, u64, u64, f64, f64)>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct ChUsageRow {
            channel_id: String,
            model: String,
            requests: u64,
            successes: u64,
            avg_latency: f64,
            p95_latency: f64,
        }
        let rows = self
            .client
            .query(
                "SELECT channel_id, model, \
                 count()::UInt64 AS requests, \
                 countIf(success = 1)::UInt64 AS successes, \
                 avg(latency_ms)::Float64 AS avg_latency, \
                 quantileExact(0.95)(latency_ms)::Float64 AS p95_latency \
                 FROM usage_events \
                 WHERE timestamp >= now() - INTERVAL 24 HOUR \
                 GROUP BY channel_id, model \
                 ORDER BY requests DESC",
            )
            .fetch_all::<ChUsageRow>()
            .await
            .map_err(|e| format!("CH channel_usage_24h_all: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.channel_id,
                    r.model,
                    r.requests,
                    r.successes,
                    r.avg_latency,
                    r.p95_latency,
                )
            })
            .collect())
    }

    /// 24h routing flow snapshot: (model, channel_id, endpoint_id, count).
    pub async fn query_routing_flow_snapshot(
        &self,
        hours: u32,
        published_models: &[String],
    ) -> Result<Vec<(String, String, Option<i64>, u64)>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct SnapRow {
            model: String,
            channel_id: String,
            endpoint_id: Option<i64>,
            cnt: u64,
        }
        let rows = self
            .client
            .query(
                "SELECT model, channel_id, endpoint_id, count()::UInt64 AS cnt \
                 FROM usage_events \
                 WHERE timestamp >= now() - INTERVAL ? HOUR \
                   AND has(?, model) \
                 GROUP BY model, channel_id, endpoint_id",
            )
            .bind(hours)
            .bind(published_models)
            .fetch_all::<SnapRow>()
            .await
            .map_err(|e| format!("CH routing_flow_snapshot: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| (r.model, r.channel_id, r.endpoint_id, r.cnt))
            .collect())
    }

    /// Routing history buckets: (bucket, channel_id, endpoint_id, requests, successes, avg_latency).
    pub async fn query_routing_history_buckets_filtered(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
        published_models: &[String],
        bucket_unit: &str,
    ) -> Result<Vec<super::db::RoutingHistoryBucket>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct BktRow {
            bucket: String,
            channel_id: String,
            requests: u64,
            successes: u64,
            avg_latency: f64,
        }
        let bucket_expr = match bucket_unit {
            "day" => {
                "formatDateTime(toTimeZone(toStartOfDay(timestamp), 'UTC'), '%Y-%m-%dT00:00:00Z')"
            }
            "hour" => {
                "formatDateTime(toTimeZone(toStartOfHour(timestamp), 'UTC'), '%Y-%m-%dT%H:00:00Z')"
            }
            _ => {
                return Err(format!(
                    "unsupported routing history bucket unit: {bucket_unit}"
                ))
            }
        };
        let sql = if model.is_some() {
            format!("SELECT {bucket_expr} AS bucket, channel_id, count()::UInt64 AS requests, countIf(success = 1)::UInt64 AS successes, avg(latency_ms)::Float64 AS avg_latency FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND model = ? AND has(?, model) GROUP BY bucket, channel_id ORDER BY bucket ASC, channel_id ASC")
        } else {
            format!("SELECT {bucket_expr} AS bucket, channel_id, count()::UInt64 AS requests, countIf(success = 1)::UInt64 AS successes, avg(latency_ms)::Float64 AS avg_latency FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND has(?, model) GROUP BY bucket, channel_id ORDER BY bucket ASC, channel_id ASC")
        };
        let mut query = self.client.query(&sql).bind(start).bind(end);
        if let Some(model) = model {
            query = query.bind(model);
        }
        let rows = query
            .bind(published_models)
            .fetch_all::<BktRow>()
            .await
            .map_err(|e| format!("CH routing_history_buckets: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| super::db::RoutingHistoryBucket {
                bucket: r.bucket,
                channel_id: r.channel_id,
                endpoint_id: None,
                requests: r.requests,
                successes: r.successes,
                avg_latency: r.avg_latency,
            })
            .collect())
    }

    /// Recent request paths: (timestamp, model, channel_id, endpoint_id, endpoint_url, latency_ms, success).
    pub async fn query_recent_request_paths(
        &self,
        limit: usize,
        published_models: &[String],
    ) -> Result<
        Vec<(
            String,
            String,
            String,
            Option<i64>,
            Option<String>,
            u64,
            bool,
        )>,
        String,
    > {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct PathRow {
            timestamp: String,
            model: String,
            channel_id: String,
            endpoint_id: Option<i64>,
            endpoint_url: Option<String>,
            latency_ms: u64,
            success: u8,
        }
        let rows = self
            .client
            .query(
                "SELECT formatDateTime(timestamp, '%Y-%m-%dT%H:%M:%SZ') AS timestamp, model, channel_id, \
                 endpoint_id, endpoint_url, latency_ms, success \
                 FROM usage_events \
                 WHERE has(?, model) \
                 ORDER BY timestamp DESC \
                 LIMIT ?",
            )
            .bind(published_models)
            .bind(limit as u64)
            .fetch_all::<PathRow>()
            .await
            .map_err(|e| format!("CH recent_request_paths: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.timestamp,
                    r.model,
                    r.channel_id,
                    r.endpoint_id,
                    r.endpoint_url,
                    r.latency_ms,
                    r.success != 0,
                )
            })
            .collect())
    }

    /// Funnel stats: per-status-count breakdown + latency percentiles.
    /// When `user_id` is `Some`, only counts events for that user.
    pub async fn query_funnel_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<crate::db::FunnelStats, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct FunnelRow {
            total: u64,
            success_count: u64,
            auth_fail_count: u64,
            rate_limit_count: u64,
            bad_request_count: u64,
            upstream_error_count: u64,
            timeout_count: u64,
            other_error_count: u64,
            p50_latency: f64,
            p95_latency: f64,
            p99_latency: f64,
            avg_latency: f64,
        }
        let sql = if user_id.is_some() {
            "SELECT \
             count()::UInt64 AS total, \
             countIf(success = 1)::UInt64 AS success_count, \
             countIf(success = 0 AND status_code IN (401, 403))::UInt64 AS auth_fail_count, \
             countIf(success = 0 AND status_code = 429)::UInt64 AS rate_limit_count, \
             countIf(success = 0 AND status_code = 400)::UInt64 AS bad_request_count, \
             countIf(success = 0 AND status_code IN (502, 503))::UInt64 AS upstream_error_count, \
             countIf(success = 0 AND status_code = 504)::UInt64 AS timeout_count, \
             countIf(success = 0 AND status_code NOT IN (400, 401, 403, 429, 502, 503, 504))::UInt64 AS other_error_count, \
             quantileExact(0.50)(latency_ms)::Float64 AS p50_latency, \
             quantileExact(0.95)(latency_ms)::Float64 AS p95_latency, \
             quantileExact(0.99)(latency_ms)::Float64 AS p99_latency, \
             avg(latency_ms)::Float64 AS avg_latency \
             FROM usage_events \
             WHERE timestamp >= ? AND user_id = ?"
        } else {
            "SELECT \
             count()::UInt64 AS total, \
             countIf(success = 1)::UInt64 AS success_count, \
             countIf(success = 0 AND status_code IN (401, 403))::UInt64 AS auth_fail_count, \
             countIf(success = 0 AND status_code = 429)::UInt64 AS rate_limit_count, \
             countIf(success = 0 AND status_code = 400)::UInt64 AS bad_request_count, \
             countIf(success = 0 AND status_code IN (502, 503))::UInt64 AS upstream_error_count, \
             countIf(success = 0 AND status_code = 504)::UInt64 AS timeout_count, \
             countIf(success = 0 AND status_code NOT IN (400, 401, 403, 429, 502, 503, 504))::UInt64 AS other_error_count, \
             quantileExact(0.50)(latency_ms)::Float64 AS p50_latency, \
             quantileExact(0.95)(latency_ms)::Float64 AS p95_latency, \
             quantileExact(0.99)(latency_ms)::Float64 AS p99_latency, \
             avg(latency_ms)::Float64 AS avg_latency \
             FROM usage_events \
             WHERE timestamp >= ?"
        };
        let mut query = self.client.query(sql).bind(since);
        if let Some(uid) = user_id {
            query = query.bind(uid);
        }
        let row = query
            .fetch_one::<FunnelRow>()
            .await
            .map_err(|e| format!("CH funnel_stats: {e}"))?;
        Ok(crate::db::FunnelStats {
            total: row.total,
            success_count: row.success_count,
            auth_fail_count: row.auth_fail_count,
            rate_limit_count: row.rate_limit_count,
            bad_request_count: row.bad_request_count,
            upstream_error_count: row.upstream_error_count,
            timeout_count: row.timeout_count,
            other_error_count: row.other_error_count,
            p50_latency: row.p50_latency,
            p95_latency: row.p95_latency,
            p99_latency: row.p99_latency,
            avg_latency: row.avg_latency,
        })
    }

    /// Daily usage counts: (date, count).
    pub async fn query_daily_usage_counts(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, u64)>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct CountRow {
            date: String,
            count: u64,
        }
        let date_expr = if tz_offset_seconds >= 0 {
            format!(
                "toDate(timestamp + toIntervalSecond({}))::String AS date",
                tz_offset_seconds
            )
        } else {
            format!(
                "toDate(timestamp - toIntervalSecond({}))::String AS date",
                -tz_offset_seconds
            )
        };
        let sql = if user_id.is_some() {
            format!(
                "SELECT {}, count()::UInt64 AS count \
                 FROM usage_events \
                 WHERE timestamp >= ? AND user_id = ? \
                 GROUP BY date ORDER BY date ASC",
                date_expr
            )
        } else {
            format!(
                "SELECT {}, count()::UInt64 AS count \
                 FROM usage_events \
                 WHERE timestamp >= ? \
                 GROUP BY date ORDER BY date ASC",
                date_expr
            )
        };
        let mut query = self.client.query(&sql).bind(since);
        if let Some(uid) = user_id {
            query = query.bind(uid);
        }
        let rows = query
            .fetch_all::<CountRow>()
            .await
            .map_err(|e| format!("CH daily_usage_counts: {e}"))?;
        Ok(rows.into_iter().map(|r| (r.date, r.count)).collect())
    }

    /// Daily usage stats: (date, count, prompt_tokens, completion_tokens, total_tokens, success_count, latency_ms, cache_hit_tokens).
    pub async fn query_daily_usage_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64, u64)>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct StatRow {
            date: String,
            count: u64,
            prompt_tokens: u64,
            completion_tokens: u64,
            total_tokens: u64,
            success_count: u64,
            latency_ms: u64,
            cache_hit_tokens: u64,
        }
        let date_expr = if tz_offset_seconds >= 0 {
            format!(
                "toDate(timestamp + toIntervalSecond({}))::String AS date",
                tz_offset_seconds
            )
        } else {
            format!(
                "toDate(timestamp - toIntervalSecond({}))::String AS date",
                -tz_offset_seconds
            )
        };
        let base_sql = format!(
            "SELECT {}, \
             count()::UInt64 AS count, \
             sum(prompt_tokens)::UInt64 AS prompt_tokens, \
             sum(completion_tokens)::UInt64 AS completion_tokens, \
             sum(total_tokens)::UInt64 AS total_tokens, \
             countIf(success = 1)::UInt64 AS success_count, \
             sum(latency_ms)::UInt64 AS latency_ms, \
             sum(cache_hit_input_tokens)::UInt64 AS cache_hit_tokens \
             FROM usage_events WHERE timestamp >= ?",
            date_expr
        );
        let sql = if user_id.is_some() {
            format!(
                "{} AND user_id = ? GROUP BY date ORDER BY date ASC",
                base_sql
            )
        } else {
            format!("{} GROUP BY date ORDER BY date ASC", base_sql)
        };
        let mut query = self.client.query(&sql).bind(since);
        if let Some(uid) = user_id {
            query = query.bind(uid);
        }
        let rows = query
            .fetch_all::<StatRow>()
            .await
            .map_err(|e| format!("CH daily_usage_stats: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.date,
                    r.count,
                    r.prompt_tokens,
                    r.completion_tokens,
                    r.total_tokens,
                    r.success_count,
                    r.latency_ms,
                    r.cache_hit_tokens,
                )
            })
            .collect())
    }

    /// Model activity: (model, total_requests, prompt_tokens, completion_tokens, success_count, failure_count, cache_hit_tokens).
    pub async fn query_model_activity(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64)>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct ActRow {
            model: String,
            total_requests: u64,
            prompt_tokens: u64,
            completion_tokens: u64,
            success_count: u64,
            failure_count: u64,
            cache_hit_tokens: u64,
        }
        let sql = "SELECT model, \
                   count()::UInt64 AS total_requests, \
                   sum(prompt_tokens)::UInt64 AS prompt_tokens, \
                   sum(completion_tokens)::UInt64 AS completion_tokens, \
                   countIf(success = 1)::UInt64 AS success_count, \
                   countIf(success = 0)::UInt64 AS failure_count, \
                   sum(cache_hit_input_tokens)::UInt64 AS cache_hit_tokens \
                   FROM usage_events WHERE timestamp >= ?";
        let rows = if let Some(uid) = user_id {
            self.client
                .query(&format!(
                    "{} AND user_id = ? GROUP BY model ORDER BY total_requests DESC",
                    sql
                ))
                .bind(since)
                .bind(uid)
                .fetch_all::<ActRow>()
                .await
                .map_err(|e| format!("CH model_activity: {e}"))?
        } else {
            self.client
                .query(&format!(
                    "{} GROUP BY model ORDER BY total_requests DESC",
                    sql
                ))
                .bind(since)
                .fetch_all::<ActRow>()
                .await
                .map_err(|e| format!("CH model_activity: {e}"))?
        };
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.model,
                    r.total_requests,
                    r.prompt_tokens,
                    r.completion_tokens,
                    r.success_count,
                    r.failure_count,
                    r.cache_hit_tokens,
                )
            })
            .collect())
    }

    pub async fn query_usage(
        &self,
        limit: usize,
        offset: usize,
        filter: &crate::domain::usage::UsageFilter,
    ) -> Result<Vec<crate::domain::usage::UsageRecord>, String> {
        let mut conditions = Vec::new();
        let mut binds = Vec::new();
        if let Some(user_id) = filter.user_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("user_id = ?");
            binds.push(user_id.to_string());
        }
        if let Some(team_id) = filter.team_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("team_id = ?");
            binds.push(team_id.to_string());
        }
        if let Some(model) = filter.model.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("model = ?");
            binds.push(model.to_string());
        }
        if let Some(api_key_name) = filter
            .api_key_name
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("api_key_name = ?");
            binds.push(api_key_name.to_string());
        }
        if let Some(api_format) = filter
            .api_format
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("api_format = ?");
            binds.push(api_format.to_string());
        }
        if let Some(start_date) = filter
            .start_date
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("usage_events.timestamp >= parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(start_date)?);
        }
        if let Some(end_date) = filter.end_date.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("usage_events.timestamp < parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(end_date)?);
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT \
             toString(usage_events.timestamp) AS timestamp, request_id, user_id, user_name, channel_id, model, \
             prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
             api_key_name, api_format, stream, \
             cache_hit_input_tokens, cache_write_tokens, prompt_price, completion_price, cache_read_price, cache_write_price, client_ip, endpoint_id, endpoint_url, original_model, team_id, ttft_ms, billing_group_id, billing_group_name, billing_payment_mode \
             FROM usage_events AS usage_events \
             {} \
             ORDER BY usage_events.timestamp DESC \
             LIMIT ? OFFSET ?",
            where_clause,
        );
        let mut query = self.client.query(&sql);
        for bind in &binds {
            query = query.bind(bind.as_str());
        }
        let rows = query
            .bind(limit as u64)
            .bind(offset as u64)
            .fetch_all::<UsageEventRow>()
            .await
            .map_err(|e| format!("CH query_usage: {e}"))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn count_usage(
        &self,
        filter: &crate::domain::usage::UsageFilter,
    ) -> Result<usize, String> {
        let mut conditions = Vec::new();
        let mut binds = Vec::new();
        if let Some(user_id) = filter.user_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("user_id = ?");
            binds.push(user_id.to_string());
        }
        if let Some(team_id) = filter.team_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("team_id = ?");
            binds.push(team_id.to_string());
        }
        if let Some(model) = filter.model.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("model = ?");
            binds.push(model.to_string());
        }
        if let Some(api_key_name) = filter
            .api_key_name
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("api_key_name = ?");
            binds.push(api_key_name.to_string());
        }
        if let Some(api_format) = filter
            .api_format
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("api_format = ?");
            binds.push(api_format.to_string());
        }
        if let Some(start_date) = filter
            .start_date
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("usage_events.timestamp >= parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(start_date)?);
        }
        if let Some(end_date) = filter.end_date.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("usage_events.timestamp < parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(end_date)?);
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT count()::UInt64 AS count FROM usage_events {}",
            where_clause
        );
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct CountRow {
            count: u64,
        }
        let mut query = self.client.query(&sql);
        for bind in &binds {
            query = query.bind(bind.as_str());
        }
        let row = query
            .fetch_one::<CountRow>()
            .await
            .map_err(|e| format!("CH count_usage: {e}"))?;
        Ok(row.count as usize)
    }

    pub async fn query_api_key_activity(
        &self,
        filter: &crate::domain::usage::UsageFilter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(Option<String>, u64, u64, String)>, String> {
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct ApiKeyRow {
            api_key_name: Option<String>,
            total_requests: u64,
            total_tokens: u64,
            last_request_at: String,
        }

        let mut conditions = Vec::new();
        let mut binds = Vec::new();
        if let Some(user_id) = filter.user_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("user_id = ?");
            binds.push(user_id.to_string());
        }
        if let Some(team_id) = filter.team_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("team_id = ?");
            binds.push(team_id.to_string());
        }
        if let Some(model) = filter.model.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("model = ?");
            binds.push(model.to_string());
        }
        if let Some(api_format) = filter
            .api_format
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("api_format = ?");
            binds.push(api_format.to_string());
        }
        if let Some(start_date) = filter
            .start_date
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("usage_events.timestamp >= parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(start_date)?);
        }
        if let Some(end_date) = filter.end_date.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("usage_events.timestamp < parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(end_date)?);
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT \
             api_key_name, \
             count()::UInt64 AS total_requests, \
             sum(total_tokens)::UInt64 AS total_tokens, \
             max(toString(timestamp)) AS last_request_at \
             FROM usage_events \
             {} \
             GROUP BY api_key_name \
             ORDER BY total_requests DESC \
             LIMIT ? OFFSET ?",
            where_clause,
        );
        let mut query = self.client.query(&sql);
        for bind in &binds {
            query = query.bind(bind.as_str());
        }
        let rows = query
            .bind(limit as u64)
            .bind(offset as u64)
            .fetch_all::<ApiKeyRow>()
            .await
            .map_err(|e| format!("CH query_api_key_activity: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    row.api_key_name,
                    row.total_requests,
                    row.total_tokens,
                    row.last_request_at,
                )
            })
            .collect())
    }

    pub async fn count_api_key_activity(
        &self,
        filter: &crate::domain::usage::UsageFilter,
    ) -> Result<usize, String> {
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct CountRow {
            count: u64,
        }

        let mut conditions = Vec::new();
        let mut binds = Vec::new();
        if let Some(user_id) = filter.user_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("user_id = ?");
            binds.push(user_id.to_string());
        }
        if let Some(team_id) = filter.team_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("team_id = ?");
            binds.push(team_id.to_string());
        }
        if let Some(model) = filter.model.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("model = ?");
            binds.push(model.to_string());
        }
        if let Some(api_format) = filter
            .api_format
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("api_format = ?");
            binds.push(api_format.to_string());
        }
        if let Some(start_date) = filter
            .start_date
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("usage_events.timestamp >= parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(start_date)?);
        }
        if let Some(end_date) = filter.end_date.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("usage_events.timestamp < parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(end_date)?);
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT count()::UInt64 AS count FROM ( \
             SELECT api_key_name FROM usage_events {} GROUP BY api_key_name \
            )",
            where_clause,
        );
        let mut query = self.client.query(&sql);
        for bind in &binds {
            query = query.bind(bind.as_str());
        }
        let row = query
            .fetch_one::<CountRow>()
            .await
            .map_err(|e| format!("CH count_api_key_activity: {e}"))?;
        Ok(row.count as usize)
    }

    pub async fn get_usage_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<crate::domain::usage::UsageRecord>, String> {
        let row = self.client
            .query(
                "SELECT \
                 toString(timestamp) AS timestamp, request_id, user_id, user_name, channel_id, model, \
                 prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
                 api_key_name, api_format, stream, \
                 cache_hit_input_tokens, cache_write_tokens, prompt_price, completion_price, cache_read_price, cache_write_price, client_ip, endpoint_id, endpoint_url, original_model, team_id, ttft_ms, \
                 request_body, response_body, reasoning_body, billing_group_id, billing_group_name, billing_payment_mode \
                 FROM usage_events \
                 WHERE request_id = ? \
                 ORDER BY timestamp DESC LIMIT 1",
            )
            .bind(request_id)
            .fetch_optional::<UsageDetailRow>()
            .await
            .map_err(|e| format!("CH get_usage_detail: {e}"))?;
        Ok(row.map(Into::into))
    }

    pub async fn query_api_key_detail(
        &self,
        filter: &crate::domain::usage::UsageFilter,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            u64,
            u64,
            Vec<(String, u64, u64)>,
            Vec<(String, u64)>,
            Vec<crate::domain::usage::UsageRecord>,
        ),
        String,
    > {
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct SummaryRow {
            total_requests: u64,
            total_tokens: u64,
        }
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct ModelRow {
            model: String,
            total_requests: u64,
            total_tokens: u64,
        }
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct ChannelRow {
            channel_id: String,
            total_requests: u64,
        }

        let mut conditions = Vec::new();
        let mut binds = Vec::new();
        if let Some(user_id) = filter.user_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("user_id = ?");
            binds.push(user_id.to_string());
        }
        if let Some(team_id) = filter.team_id.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("team_id = ?");
            binds.push(team_id.to_string());
        }
        if let Some(model) = filter.model.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("model = ?");
            binds.push(model.to_string());
        }
        if let Some(api_key_name) = filter
            .api_key_name
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("api_key_name = ?");
            binds.push(api_key_name.to_string());
        }
        if let Some(api_format) = filter
            .api_format
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("api_format = ?");
            binds.push(api_format.to_string());
        }
        if let Some(start_date) = filter
            .start_date
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            conditions.push("usage_events.timestamp >= parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(start_date)?);
        }
        if let Some(end_date) = filter.end_date.as_deref().filter(|value| !value.is_empty()) {
            conditions.push("usage_events.timestamp < parseDateTimeBestEffort(?)");
            binds.push(normalize_clickhouse_datetime(end_date)?);
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let summary_sql = format!(
            "SELECT count()::UInt64 AS total_requests, sum(total_tokens)::UInt64 AS total_tokens FROM usage_events {}",
            where_clause,
        );
        let mut summary_query = self.client.query(&summary_sql);
        for bind in &binds {
            summary_query = summary_query.bind(bind.as_str());
        }
        let summary = summary_query
            .fetch_one::<SummaryRow>()
            .await
            .map_err(|e| format!("CH query_api_key_detail summary: {e}"))?;

        let model_sql = format!(
            "SELECT model, count()::UInt64 AS total_requests, sum(total_tokens)::UInt64 AS total_tokens \
             FROM usage_events {} GROUP BY model ORDER BY total_requests DESC, total_tokens DESC LIMIT 5",
            where_clause,
        );
        let mut model_query = self.client.query(&model_sql);
        for bind in &binds {
            model_query = model_query.bind(bind.as_str());
        }
        let top_models = model_query
            .fetch_all::<ModelRow>()
            .await
            .map_err(|e| format!("CH query_api_key_detail models: {e}"))?
            .into_iter()
            .map(|row| (row.model, row.total_requests, row.total_tokens))
            .collect();

        let channel_sql = format!(
            "SELECT channel_id, count()::UInt64 AS total_requests \
             FROM usage_events {} GROUP BY channel_id ORDER BY total_requests DESC LIMIT 5",
            where_clause,
        );
        let mut channel_query = self.client.query(&channel_sql);
        for bind in &binds {
            channel_query = channel_query.bind(bind.as_str());
        }
        let top_channels = channel_query
            .fetch_all::<ChannelRow>()
            .await
            .map_err(|e| format!("CH query_api_key_detail channels: {e}"))?
            .into_iter()
            .map(|row| (row.channel_id, row.total_requests))
            .collect();

        let requests = self.query_usage(limit, offset, filter).await?;

        Ok((
            summary.total_requests,
            summary.total_tokens,
            top_models,
            top_channels,
            requests,
        ))
    }

    pub async fn query_usage_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<crate::domain::usage::UsageRecord>, String> {
        let sql = if user_id.is_some() {
            "SELECT \
             toString(timestamp) AS timestamp, request_id, user_id, user_name, channel_id, model, \
             prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
             api_key_name, api_format, stream, \
             cache_hit_input_tokens, cache_write_tokens, prompt_price, completion_price, cache_read_price, cache_write_price, client_ip, endpoint_id, endpoint_url, original_model, team_id, ttft_ms, billing_group_id, billing_group_name, billing_payment_mode \
             FROM usage_events WHERE timestamp >= ? AND user_id = ? ORDER BY timestamp ASC"
        } else {
            "SELECT \
             toString(timestamp) AS timestamp, request_id, user_id, user_name, channel_id, model, \
             prompt_tokens, completion_tokens, total_tokens, latency_ms, status_code, success, \
             api_key_name, api_format, stream, \
             cache_hit_input_tokens, cache_write_tokens, prompt_price, completion_price, cache_read_price, cache_write_price, client_ip, endpoint_id, endpoint_url, original_model, team_id, ttft_ms, billing_group_id, billing_group_name, billing_payment_mode \
             FROM usage_events WHERE timestamp >= ? ORDER BY timestamp ASC"
        };
        let mut query = self.client.query(sql).bind(since);
        if let Some(uid) = user_id {
            query = query.bind(uid);
        }
        let rows = query
            .fetch_all::<UsageEventRow>()
            .await
            .map_err(|e| format!("CH query_usage_since: {e}"))?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn query_usage_stats_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<(u64, u64, u64, u64), String> {
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct StatsRow {
            total: u64,
            success_count: u64,
            total_latency: u64,
            total_tokens: u64,
        }
        let sql = if user_id.is_some() {
            "SELECT \
             count()::UInt64 AS total, \
             countIf(success = 1)::UInt64 AS success_count, \
             sum(latency_ms)::UInt64 AS total_latency, \
             sum(total_tokens)::UInt64 AS total_tokens \
             FROM usage_events WHERE timestamp >= ? AND user_id = ?"
        } else {
            "SELECT \
             count()::UInt64 AS total, \
             countIf(success = 1)::UInt64 AS success_count, \
             sum(latency_ms)::UInt64 AS total_latency, \
             sum(total_tokens)::UInt64 AS total_tokens \
             FROM usage_events WHERE timestamp >= ?"
        };
        let mut query = self.client.query(sql).bind(since);
        if let Some(uid) = user_id {
            query = query.bind(uid);
        }
        let row = query
            .fetch_one::<StatsRow>()
            .await
            .map_err(|e| format!("CH query_usage_stats_since: {e}"))?;
        Ok((
            row.total,
            row.success_count,
            row.total_latency,
            row.total_tokens,
        ))
    }

    /// Routing history endpoint stats: (channel_id, endpoint_id, requests, successes, avg_latency, p95).
    pub async fn query_routing_history_stats_filtered(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
        published_models: &[String],
    ) -> Result<Vec<super::db::RoutingEndpointStat>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct StatRow {
            channel_id: String,
            endpoint_id: Option<i64>,
            requests: u64,
            successes: u64,
            avg_latency: f64,
            p95_latency: f64,
        }
        let sql = if model.is_some() {
            "SELECT channel_id, CAST(NULL AS Nullable(Int64)) AS endpoint_id, count()::UInt64 AS requests, countIf(success = 1)::UInt64 AS successes, avg(latency_ms)::Float64 AS avg_latency, quantileExact(0.95)(latency_ms)::Float64 AS p95_latency FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND model = ? AND has(?, model) GROUP BY channel_id"
        } else {
            "SELECT channel_id, CAST(NULL AS Nullable(Int64)) AS endpoint_id, count()::UInt64 AS requests, countIf(success = 1)::UInt64 AS successes, avg(latency_ms)::Float64 AS avg_latency, quantileExact(0.95)(latency_ms)::Float64 AS p95_latency FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND has(?, model) GROUP BY channel_id"
        };
        let mut query = self.client.query(sql).bind(start).bind(end);
        if let Some(model) = model {
            query = query.bind(model);
        }
        let rows = query
            .bind(published_models)
            .fetch_all::<StatRow>()
            .await
            .map_err(|e| format!("CH routing_history_stats: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| super::db::RoutingEndpointStat {
                channel_id: r.channel_id,
                endpoint_id: r.endpoint_id,
                requests: r.requests,
                successes: r.successes,
                avg_latency: r.avg_latency,
                p95_latency: r.p95_latency,
            })
            .collect())
    }

    /// Overall routing history statistics from observability data.
    pub async fn query_routing_history_overall_stats_filtered(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
        published_models: &[String],
    ) -> Result<(u64, u64, f64, f64), String> {
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct OverallRow {
            requests: u64,
            successes: u64,
            avg_latency: f64,
            p95_latency: f64,
        }
        let sql = if model.is_some() {
            "SELECT count()::UInt64 AS requests, countIf(success = 1)::UInt64 AS successes, avg(latency_ms)::Float64 AS avg_latency, quantileExact(0.95)(latency_ms)::Float64 AS p95_latency FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND model = ? AND has(?, model)"
        } else {
            "SELECT count()::UInt64 AS requests, countIf(success = 1)::UInt64 AS successes, avg(latency_ms)::Float64 AS avg_latency, quantileExact(0.95)(latency_ms)::Float64 AS p95_latency FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND has(?, model)"
        };
        let mut query = self.client.query(sql).bind(start).bind(end);
        if let Some(model) = model {
            query = query.bind(model);
        }
        let row = query
            .bind(published_models)
            .fetch_one::<OverallRow>()
            .await
            .map_err(|e| format!("CH routing_history_overall_stats: {e}"))?;
        Ok((
            row.requests,
            row.successes,
            row.avg_latency,
            row.p95_latency,
        ))
    }

    /// Endpoint-level routing history details from observability data.
    pub async fn query_routing_history_endpoint_details(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
        published_models: &[String],
    ) -> Result<Vec<(String, Option<i64>, Option<String>, u64, u64, u64, f64, f64)>, String> {
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct DetailRow {
            channel_id: String,
            endpoint_id: Option<i64>,
            endpoint_url: Option<String>,
            url_variant_count: u64,
            requests: u64,
            successes: u64,
            avg_latency: f64,
            p95_latency: f64,
        }
        let sql = if model.is_some() {
            "SELECT ue.channel_id, ue.endpoint_id, anyIf(ue.endpoint_url, ue.endpoint_url != '') AS endpoint_url, uniqExactIf(ue.endpoint_url, ue.endpoint_url != '')::UInt64 AS url_variant_count, count()::UInt64 AS requests, countIf(ue.success = 1)::UInt64 AS successes, avg(ue.latency_ms)::Float64 AS avg_latency, quantileExact(0.95)(ue.latency_ms)::Float64 AS p95_latency FROM usage_events AS ue WHERE ue.timestamp >= parseDateTimeBestEffort(?) AND ue.timestamp <= parseDateTimeBestEffort(?) AND ue.model = ? AND has(?, ue.model) GROUP BY ue.channel_id, ue.endpoint_id"
        } else {
            "SELECT ue.channel_id, ue.endpoint_id, anyIf(ue.endpoint_url, ue.endpoint_url != '') AS endpoint_url, uniqExactIf(ue.endpoint_url, ue.endpoint_url != '')::UInt64 AS url_variant_count, count()::UInt64 AS requests, countIf(ue.success = 1)::UInt64 AS successes, avg(ue.latency_ms)::Float64 AS avg_latency, quantileExact(0.95)(ue.latency_ms)::Float64 AS p95_latency FROM usage_events AS ue WHERE ue.timestamp >= parseDateTimeBestEffort(?) AND ue.timestamp <= parseDateTimeBestEffort(?) AND has(?, ue.model) GROUP BY ue.channel_id, ue.endpoint_id"
        };
        let mut query = self.client.query(sql).bind(start).bind(end);
        if let Some(model) = model {
            query = query.bind(model);
        }
        let rows = query
            .bind(published_models)
            .fetch_all::<DetailRow>()
            .await
            .map_err(|e| format!("CH routing_history_endpoint_details: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.channel_id,
                    r.endpoint_id,
                    r.endpoint_url,
                    r.url_variant_count,
                    r.requests,
                    r.successes,
                    r.avg_latency,
                    r.p95_latency,
                )
            })
            .collect())
    }

    pub async fn query_flow_metrics(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
        published_models: &[String],
    ) -> Result<FlowMetricsHistorical, String> {
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct CompletionRow {
            total_completed: u64,
            success_completed: u64,
            failed_completed: u64,
            latency_p50: Option<f64>,
            latency_p90: Option<f64>,
            latency_p99: Option<f64>,
            latency_samples: u64,
            ttft_p50: Option<f64>,
            ttft_p90: Option<f64>,
            ttft_p99: Option<f64>,
            ttft_samples: u64,
        }
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct ModelRow {
            model: String,
            requests: u64,
        }
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct IpRow {
            ip: String,
            requests: u64,
        }
        #[derive(clickhouse::Row, serde::Deserialize)]
        struct TrendRow {
            bucket: String,
            success_completed: u64,
            failed_completed: u64,
        }

        let completion_sql = if model.is_some() {
            "SELECT count()::UInt64 AS total_completed, countIf(success = 1)::UInt64 AS success_completed, countIf(success = 0)::UInt64 AS failed_completed, quantileExact(0.50)(latency_ms)::Nullable(Float64) AS latency_p50, quantileExact(0.90)(latency_ms)::Nullable(Float64) AS latency_p90, quantileExact(0.99)(latency_ms)::Nullable(Float64) AS latency_p99, count()::UInt64 AS latency_samples, quantileExactIf(0.50)(ttft_ms, ttft_ms IS NOT NULL)::Nullable(Float64) AS ttft_p50, quantileExactIf(0.90)(ttft_ms, ttft_ms IS NOT NULL)::Nullable(Float64) AS ttft_p90, quantileExactIf(0.99)(ttft_ms, ttft_ms IS NOT NULL)::Nullable(Float64) AS ttft_p99, countIf(ttft_ms IS NOT NULL)::UInt64 AS ttft_samples FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND model = ? AND has(?, model)"
        } else {
            "SELECT count()::UInt64 AS total_completed, countIf(success = 1)::UInt64 AS success_completed, countIf(success = 0)::UInt64 AS failed_completed, quantileExact(0.50)(latency_ms)::Nullable(Float64) AS latency_p50, quantileExact(0.90)(latency_ms)::Nullable(Float64) AS latency_p90, quantileExact(0.99)(latency_ms)::Nullable(Float64) AS latency_p99, count()::UInt64 AS latency_samples, quantileExactIf(0.50)(ttft_ms, ttft_ms IS NOT NULL)::Nullable(Float64) AS ttft_p50, quantileExactIf(0.90)(ttft_ms, ttft_ms IS NOT NULL)::Nullable(Float64) AS ttft_p90, quantileExactIf(0.99)(ttft_ms, ttft_ms IS NOT NULL)::Nullable(Float64) AS ttft_p99, countIf(ttft_ms IS NOT NULL)::UInt64 AS ttft_samples FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND has(?, model)"
        };
        let mut completion_query = self.client.query(completion_sql).bind(start).bind(end);
        if let Some(model) = model {
            completion_query = completion_query.bind(model);
        }
        let completion = completion_query
            .bind(published_models)
            .fetch_one::<CompletionRow>()
            .await
            .map_err(|e| format!("CH flow_metrics completion: {e}"))?;

        let model_sql = if model.is_some() {
            "SELECT model, count()::UInt64 AS requests FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND model = ? AND has(?, model) GROUP BY model ORDER BY requests DESC"
        } else {
            "SELECT model, count()::UInt64 AS requests FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND has(?, model) GROUP BY model ORDER BY requests DESC"
        };
        let mut model_query = self.client.query(model_sql).bind(start).bind(end);
        if let Some(model) = model {
            model_query = model_query.bind(model);
        }
        let model_rows = model_query
            .bind(published_models)
            .fetch_all::<ModelRow>()
            .await
            .map_err(|e| format!("CH flow_metrics models: {e}"))?;
        let model_total = model_rows
            .iter()
            .map(|row| row.requests)
            .sum::<u64>()
            .max(1);
        let model_share = model_rows
            .into_iter()
            .map(|row| FlowMetricsModelShare {
                model: row.model,
                requests: row.requests,
                share: ((row.requests as f64 / model_total as f64) * 1000.0).round() / 10.0,
            })
            .collect();

        let ip_sql = if model.is_some() {
            "SELECT assumeNotNull(client_ip) AS ip, count()::UInt64 AS requests FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND model = ? AND has(?, model) AND client_ip IS NOT NULL AND client_ip != '' GROUP BY client_ip ORDER BY requests DESC LIMIT 20"
        } else {
            "SELECT assumeNotNull(client_ip) AS ip, count()::UInt64 AS requests FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND has(?, model) AND client_ip IS NOT NULL AND client_ip != '' GROUP BY client_ip ORDER BY requests DESC LIMIT 20"
        };
        let mut ip_query = self.client.query(ip_sql).bind(start).bind(end);
        if let Some(model) = model {
            ip_query = ip_query.bind(model);
        }
        let client_ips = ip_query
            .bind(published_models)
            .fetch_all::<IpRow>()
            .await
            .map_err(|e| format!("CH flow_metrics ips: {e}"))?
            .into_iter()
            .map(|row| FlowMetricsClientIp {
                ip: row.ip,
                requests: row.requests,
            })
            .collect();

        let (bucket_unit, bucket_seconds) = flow_metrics_bucket_granularity(start, end)?;
        let bucket_expr = if bucket_unit == "minute" {
            "formatDateTime(toTimeZone(toStartOfMinute(timestamp), 'UTC'), '%Y-%m-%dT%H:%i:%SZ')"
        } else {
            "formatDateTime(toTimeZone(toStartOfHour(timestamp), 'UTC'), '%Y-%m-%dT%H:%i:%SZ')"
        };
        let trend_sql = if model.is_some() {
            format!(
                "SELECT {bucket_expr} AS bucket, countIf(success = 1)::UInt64 AS success_completed, countIf(success = 0)::UInt64 AS failed_completed FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND model = ? AND has(?, model) GROUP BY bucket ORDER BY bucket ASC"
            )
        } else {
            format!(
                "SELECT {bucket_expr} AS bucket, countIf(success = 1)::UInt64 AS success_completed, countIf(success = 0)::UInt64 AS failed_completed FROM usage_events WHERE timestamp >= parseDateTimeBestEffort(?) AND timestamp <= parseDateTimeBestEffort(?) AND has(?, model) GROUP BY bucket ORDER BY bucket ASC"
            )
        };
        let mut trend_query = self.client.query(&trend_sql).bind(start).bind(end);
        if let Some(model) = model {
            trend_query = trend_query.bind(model);
        }
        let trend_rows = trend_query
            .bind(published_models)
            .fetch_all::<TrendRow>()
            .await
            .map_err(|e| format!("CH flow_metrics trend: {e}"))?;
        let trend_map = trend_rows
            .into_iter()
            .map(|row| (row.bucket, (row.success_completed, row.failed_completed)))
            .collect::<std::collections::HashMap<_, _>>();

        let start_dt = DateTime::parse_from_rfc3339(start)
            .map_err(|_| "invalid start datetime".to_string())?
            .with_timezone(&chrono::Utc);
        let end_dt = DateTime::parse_from_rfc3339(end)
            .map_err(|_| "invalid end datetime".to_string())?
            .with_timezone(&chrono::Utc);
        let mut cursor = if bucket_unit == "minute" {
            start_dt
                .with_second(0)
                .and_then(|dt| dt.with_nanosecond(0))
                .expect("valid minute alignment")
        } else {
            start_dt
                .with_minute(0)
                .and_then(|dt| dt.with_second(0))
                .and_then(|dt| dt.with_nanosecond(0))
                .expect("valid hour alignment")
        };

        let mut buckets = Vec::new();
        let mut success_completed = Vec::new();
        let mut failed_completed = Vec::new();
        while cursor <= end_dt {
            let bucket = cursor.format("%Y-%m-%dT%H:%M:%SZ").to_string();
            let (succ, fail) = trend_map.get(&bucket).copied().unwrap_or((0, 0));
            buckets.push(bucket);
            success_completed.push(succ);
            failed_completed.push(fail);
            cursor += chrono::Duration::seconds(bucket_seconds);
        }

        Ok(FlowMetricsHistorical {
            total_completed: completion.total_completed,
            success_completed: completion.success_completed,
            failed_completed: completion.failed_completed,
            model_share,
            client_ips,
            latency_ms: FlowMetricsPercentiles {
                p50: completion.latency_p50,
                p90: completion.latency_p90,
                p99: completion.latency_p99,
                sample_count: completion.latency_samples,
            },
            ttft_ms: FlowMetricsPercentiles {
                p50: completion.ttft_p50,
                p90: completion.ttft_p90,
                p99: completion.ttft_p99,
                sample_count: completion.ttft_samples,
            },
            trend: FlowMetricsTrend {
                bucket_unit,
                buckets,
                success_completed,
                failed_completed,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_clickhouse_datetime;

    #[test]
    fn preserves_rfc3339_datetime_for_clickhouse() {
        let normalized = normalize_clickhouse_datetime("2026-07-26T16:00:00.000Z").unwrap();
        assert_eq!(normalized, "2026-07-26T16:00:00+00:00");
    }

    #[test]
    fn preserves_offset_datetime() {
        let normalized = normalize_clickhouse_datetime("2026-07-27T00:00:00+08:00").unwrap();
        assert_eq!(normalized, "2026-07-27T00:00:00+08:00");
    }

    #[test]
    fn accepts_space_separated_datetime() {
        let normalized = normalize_clickhouse_datetime("2026-07-26 16:00:00").unwrap();
        assert_eq!(normalized, "2026-07-26 16:00:00");
    }

    #[test]
    fn accepts_date_only_filter() {
        let normalized = normalize_clickhouse_datetime("2026-07-26").unwrap();
        assert_eq!(normalized, "2026-07-26 00:00:00");
    }

    #[test]
    fn rejects_invalid_datetime() {
        let error = normalize_clickhouse_datetime("not-a-datetime").unwrap_err();
        assert_eq!(error, "invalid datetime filter");
    }
}
