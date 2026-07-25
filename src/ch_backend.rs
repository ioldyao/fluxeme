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
            cost_amount Float64,\
            client_ip Nullable(String),\
            endpoint_id Nullable(Int64)\
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

    // ── Raw query access ─────────────────────────────────────────────

    /// Returns a reference to the underlying ClickHouse client for ad-hoc queries.
    pub fn client(&self) -> &Client {
        &self.client
    }

    // ── Observability queries (Phase 8, routed from admin handlers) ──

    /// 24h channel usage: (channel_id, model, requests, successes, avg_latency, p95).
    pub async fn query_channel_usage_24h(
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
            .map_err(|e| format!("CH channel_usage_24h: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.channel_id, r.model, r.requests, r.successes, r.avg_latency, r.p95_latency,
                )
            })
            .collect())
    }

    /// 24h routing flow snapshot: (model, channel_id, endpoint_id, count).
    pub async fn query_routing_flow_snapshot(
        &self,
        hours: u32,
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
                 WHERE timestamp >= now() - INTERVAL {h:UInt32} HOUR \
                 GROUP BY model, channel_id, endpoint_id",
            )
            .bind(hours)
            .fetch_all::<SnapRow>()
            .await
            .map_err(|e| format!("CH routing_flow_snapshot: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| (r.model, r.channel_id, r.endpoint_id, r.cnt))
            .collect())
    }

    /// Routing history buckets: (bucket, channel_id, endpoint_id, requests, successes, avg_latency).
    pub async fn query_routing_history_buckets(
        &self,
        start: &str,
        end: &str,
    ) -> Result<Vec<super::db::RoutingHistoryBucket>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct BktRow {
            bucket: String,
            channel_id: String,
            endpoint_id: Option<i64>,
            requests: u64,
            successes: u64,
            avg_latency: f64,
        }
        let rows = self
            .client
            .query(
                "SELECT toStartOfHour(timestamp)::String AS bucket, \
                 channel_id, endpoint_id, \
                 count()::UInt64 AS requests, \
                 countIf(success = 1)::UInt64 AS successes, \
                 avg(latency_ms)::Float64 AS avg_latency \
                 FROM usage_events \
                 WHERE timestamp >= {s:String} AND timestamp <= {e:String} \
                 GROUP BY bucket, channel_id, endpoint_id \
                 ORDER BY bucket ASC",
            )
            .bind(start)
            .bind(end)
            .fetch_all::<BktRow>()
            .await
            .map_err(|e| format!("CH routing_history_buckets: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| super::db::RoutingHistoryBucket {
                bucket: r.bucket,
                channel_id: r.channel_id,
                endpoint_id: r.endpoint_id,
                requests: r.requests,
                successes: r.successes,
                avg_latency: r.avg_latency,
            })
            .collect())
    }

    /// Recent request paths: (timestamp, model, channel_id, endpoint_id, latency_ms, success).
    pub async fn query_recent_request_paths(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, String, Option<i64>, u64, bool)>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct PathRow {
            timestamp: String,
            model: String,
            channel_id: String,
            endpoint_id: Option<i64>,
            latency_ms: u64,
            success: u8,
        }
        let rows = self
            .client
            .query(
                "SELECT toString(timestamp) AS timestamp, model, channel_id, \
                 endpoint_id, latency_ms, success \
                 FROM usage_events \
                 ORDER BY timestamp DESC \
                 LIMIT {l:UInt64}",
            )
            .bind(limit as u64)
            .fetch_all::<PathRow>()
            .await
            .map_err(|e| format!("CH recent_request_paths: {e}"))?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.timestamp, r.model, r.channel_id, r.endpoint_id, r.latency_ms,
                    r.success != 0,
                )
            })
            .collect())
    }

    /// Funnel stats: per-status-count breakdown + latency percentiles.
    pub async fn query_funnel_stats(
        &self,
        since: &str,
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
        let row = self
            .client
            .query(
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
                 WHERE timestamp >= {s:String}",
            )
            .bind(since)
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
    ) -> Result<Vec<(String, u64)>, String> {
        #[derive(clickhouse::Row, serde::Serialize, serde::Deserialize)]
        struct CountRow {
            date: String,
            count: u64,
        }
        let rows = if let Some(uid) = user_id {
            self.client
                .query(
                    "SELECT toDate(timestamp)::String AS date, count()::UInt64 AS count \
                     FROM usage_events \
                     WHERE timestamp >= {s:String} AND user_id = {u:String} \
                     GROUP BY date ORDER BY date ASC",
                )
                .bind(since)
                .bind(uid)
                .fetch_all::<CountRow>()
                .await
                .map_err(|e| format!("CH daily_usage_counts: {e}"))?
        } else {
            self.client
                .query(
                    "SELECT toDate(timestamp)::String AS date, count()::UInt64 AS count \
                     FROM usage_events \
                     WHERE timestamp >= {s:String} \
                     GROUP BY date ORDER BY date ASC",
                )
                .bind(since)
                .fetch_all::<CountRow>()
                .await
                .map_err(|e| format!("CH daily_usage_counts: {e}"))?
        };
        Ok(rows.into_iter().map(|r| (r.date, r.count)).collect())
    }

    /// Daily usage stats: (date, count, prompt_tokens, completion_tokens, total_tokens, success_count, latency_ms, cache_hit_tokens).
    pub async fn query_daily_usage_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
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
        let sql = "SELECT toDate(timestamp)::String AS date, \
                   count()::UInt64 AS count, \
                   sum(prompt_tokens)::UInt64 AS prompt_tokens, \
                   sum(completion_tokens)::UInt64 AS completion_tokens, \
                   sum(total_tokens)::UInt64 AS total_tokens, \
                   countIf(success = 1)::UInt64 AS success_count, \
                   sum(latency_ms)::UInt64 AS latency_ms, \
                   sum(cache_hit_input_tokens)::UInt64 AS cache_hit_tokens \
                   FROM usage_events WHERE timestamp >= {s:String}";
        let rows = if let Some(uid) = user_id {
            self.client
                .query(&format!("{} AND user_id = {{u:String}} GROUP BY date ORDER BY date ASC", sql))
                .bind(since)
                .bind(uid)
                .fetch_all::<StatRow>()
                .await
                .map_err(|e| format!("CH daily_usage_stats: {e}"))?
        } else {
            self.client
                .query(&format!("{} GROUP BY date ORDER BY date ASC", sql))
                .bind(since)
                .fetch_all::<StatRow>()
                .await
                .map_err(|e| format!("CH daily_usage_stats: {e}"))?
        };
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.date, r.count, r.prompt_tokens, r.completion_tokens, r.total_tokens,
                    r.success_count, r.latency_ms, r.cache_hit_tokens,
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
                   FROM usage_events WHERE timestamp >= {s:String}";
        let rows = if let Some(uid) = user_id {
            self.client
                .query(&format!("{} AND user_id = {{u:String}} GROUP BY model ORDER BY total_requests DESC", sql))
                .bind(since)
                .bind(uid)
                .fetch_all::<ActRow>()
                .await
                .map_err(|e| format!("CH model_activity: {e}"))?
        } else {
            self.client
                .query(&format!("{} GROUP BY model ORDER BY total_requests DESC", sql))
                .bind(since)
                .fetch_all::<ActRow>()
                .await
                .map_err(|e| format!("CH model_activity: {e}"))?
        };
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.model, r.total_requests, r.prompt_tokens, r.completion_tokens,
                    r.success_count, r.failure_count, r.cache_hit_tokens,
                )
            })
            .collect())
    }

    /// Routing history endpoint stats: (channel_id, endpoint_id, requests, successes, avg_latency, p95).
    pub async fn query_routing_history_stats(
        &self,
        start: &str,
        end: &str,
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
        let rows = self
            .client
            .query(
                "SELECT channel_id, endpoint_id, \
                 count()::UInt64 AS requests, \
                 countIf(success = 1)::UInt64 AS successes, \
                 avg(latency_ms)::Float64 AS avg_latency, \
                 quantileExact(0.95)(latency_ms)::Float64 AS p95_latency \
                 FROM usage_events \
                 WHERE timestamp >= {s:String} AND timestamp <= {e:String} \
                 GROUP BY channel_id, endpoint_id",
            )
            .bind(start)
            .bind(end)
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
}
