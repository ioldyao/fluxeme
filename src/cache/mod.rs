use std::time::Duration;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};

use crate::domain::usage::UsageRecord;

/// Gate status for a user — used by the billing system to decide whether
/// to accept or reject a request *before* it hits the upstream provider.
///
/// The status is stored in Redis at `gate_status:{user_id}` and is written
/// by the background deduction writer and a periodic inspection task.
/// PostgreSQL is the source of truth; Redis is a read-optimized cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateStatus {
    /// Balance is healthy — requests should pass through.
    Ok,
    /// Balance is low but not yet exhausted — requests pass through,
    /// UI may show a warning.
    Low,
    /// Balance exhausted (balance - frozen <= 0) — handler rejects
    /// with 402 Payment Required.
    Blocked,
}

impl GateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Low => "low",
            Self::Blocked => "blocked",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "blocked" => Self::Blocked,
            "low" => Self::Low,
            _ => Self::Ok,
        }
    }
}

/// Compute gate status from a wallet balance and frozen amount.
pub fn compute_gate_status(balance: Decimal, frozen: Decimal) -> GateStatus {
    if balance - frozen <= Decimal::ZERO {
        GateStatus::Blocked
    } else {
        GateStatus::Ok
    }
}

/// Redis-backed exact-response cache with mandatory tenant isolation.
///
/// Every key is prefixed with the tenant/user ID so that different tenants
/// physically occupy separate keys — there is no shared-namespace look-up
/// that could accidentally return another tenant's cached response.
///
/// Also provides gate-status methods for the billing system (see
/// `get_gate_status`, `set_gate_status`, `set_balance`).
///
pub struct RedisCache {
    client: redis::Client,
    con: redis::aio::MultiplexedConnection,
    default_ttl_secs: u64,
}

impl RedisCache {
    /// Create a new cache backed by the given Redis URL.
    pub async fn new(redis_url: &str, default_ttl_secs: u64) -> Result<Self, String> {
        let client =
            redis::Client::open(redis_url).map_err(|e| format!("Redis URL error: {}", e))?;
        let con = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| format!("Redis connection failed: {}", e))?;
        Ok(Self {
            client,
            con,
            default_ttl_secs,
        })
    }

    /// Connectivity check (PING). Returns Ok only if Redis is reachable.
    /// Used by readiness probes.
    pub async fn ping(&self) -> Result<(), String> {
        let mut con = self.con.clone();
        redis::Cmd::ping()
            .query_async::<String>(&mut con)
            .await
            .map_err(|e| format!("Redis PING error: {e}"))?;
        Ok(())
    }

    /// Distributed fixed-window RPM check (Lua-atomic).
    /// Returns Ok(true) if the request is allowed, Ok(false) if rate-limited.
    /// Redis errors are returned to the caller.
    pub async fn rate_limit_rpm(&self, key: &str, limit: u64) -> Result<bool, String> {
        let mut con = self.con.clone();
        let allowed: i64 = redis::Script::new(
            "local cur = tonumber(redis.call('GET', KEYS[1]) or '0')
             if cur >= tonumber(ARGV[1]) then return 0 end
             local new = redis.call('INCR', KEYS[1])
             if new == 1 then redis.call('EXPIRE', KEYS[1], 60) end
             return 1",
        )
        .key(key)
        .arg(limit)
        .invoke_async(&mut con)
        .await
        .map_err(|e| format!("Redis rate_limit_rpm error: {e}"))?;
        Ok(allowed == 1)
    }

    /// Distributed fixed-window TPM check (Lua-atomic).
    /// Returns Ok(true) if the request is allowed, Ok(false) if rate-limited.
    /// Redis errors are returned to the caller.
    pub async fn rate_limit_tpm(&self, key: &str, limit: u64, tokens: u64) -> Result<bool, String> {
        let mut con = self.con.clone();
        let allowed: i64 = redis::Script::new(
            "local cur = tonumber(redis.call('GET', KEYS[1]) or '0')
             if cur + tonumber(ARGV[2]) > tonumber(ARGV[1]) then return 0 end
             local new = redis.call('INCRBY', KEYS[1], ARGV[2])
             if new == tonumber(ARGV[2]) then redis.call('EXPIRE', KEYS[1], 60) end
             return 1",
        )
        .key(key)
        .arg(limit)
        .arg(tokens)
        .invoke_async(&mut con)
        .await
        .map_err(|e| format!("Redis rate_limit_tpm error: {e}"))?;
        Ok(allowed == 1)
    }

    /// Publish a message to a Redis pub/sub channel.
    /// Redis errors are returned to the caller.
    pub async fn publish(&self, channel: &str, payload: &str) -> Result<(), String> {
        let mut con = self.con.clone();
        redis::Cmd::publish(channel, payload)
            .query_async::<i64>(&mut con)
            .await
            .map_err(|e| format!("Redis PUBLISH error: {e}"))?;
        Ok(())
    }

    /// Open a dedicated pub/sub subscription.
    pub async fn subscribe(&self, channel: &str) -> Result<redis::aio::PubSub, String> {
        let client = &self.client;
        let mut pubsub = client
            .get_async_pubsub()
            .await
            .map_err(|e| format!("Redis pubsub connection failed: {e}"))?;
        pubsub
            .subscribe(channel)
            .await
            .map_err(|e| format!("Redis pubsub subscribe failed: {e}"))?;
        Ok(pubsub)
    }

    /// Retrieve a cached value for the given tenant.
    ///
    /// The key is constructed as `cache:exact:{tenant_id}:{sha256(cache_key)}`
    /// so the tenant ID is an *enforced part of the key itself*, not metadata
    /// that could be accidentally omitted from the query.
    pub async fn get(&self, tenant_id: &str, cache_key: &str) -> Result<Option<String>, String> {
        let mut con = self.con.clone();
        let redis_key = build_redis_key(tenant_id, cache_key);
        redis::Cmd::get(&redis_key)
            .query_async::<Option<String>>(&mut con)
            .await
            .map_err(|e| format!("Redis GET error: {}", e))
    }

    /// Store a value in the cache for the given tenant.
    pub async fn set(
        &self,
        tenant_id: &str,
        cache_key: &str,
        value: &str,
        ttl_secs: u64,
    ) -> Result<(), String> {
        let mut con = self.con.clone();
        let redis_key = build_redis_key(tenant_id, cache_key);
        let ttl = if ttl_secs > 0 {
            ttl_secs
        } else {
            self.default_ttl_secs
        };
        redis::Cmd::set_ex(&redis_key, value, ttl)
            .query_async::<()>(&mut con)
            .await
            .map_err(|e| format!("Redis SET error: {}", e))
    }

    #[allow(dead_code)]
    pub fn default_ttl(&self) -> u64 {
        self.default_ttl_secs
    }

    // ── Billing gate status ─────────────────────────────────────────

    /// Read the gate status for a user from Redis.
    ///
    /// Returns `None` when no status has been set (e.g., first request,
    /// ; the caller may use PostgreSQL for a cold read.
    pub async fn get_gate_status(&self, user_id: &str) -> Result<Option<GateStatus>, String> {
        let mut con = self.con.clone();
        let key = format!("gate_status:{}", user_id);
        let val: Option<String> = redis::Cmd::get(&key)
            .query_async(&mut con)
            .await
            .map_err(|e| format!("Redis GET gate_status error: {}", e))?;
        Ok(val.as_deref().map(GateStatus::from_str))
    }

    /// Set the gate status for a user in Redis (persistent, no TTL).
    #[allow(dead_code)]
    pub async fn set_gate_status(&self, user_id: &str, status: GateStatus) -> Result<(), String> {
        let mut con = self.con.clone();
        let key = format!("gate_status:{}", user_id);
        redis::Cmd::set(&key, status.as_str())
            .query_async::<()>(&mut con)
            .await
            .map_err(|e| format!("Redis SET gate_status error: {}", e))
    }

    /// Write the current balance to Redis for fast read by the inspection
    /// task (persistent, no TTL).
    #[allow(dead_code)]
    pub async fn set_balance(&self, user_id: &str, balance: Decimal) -> Result<(), String> {
        let mut con = self.con.clone();
        let key = format!("balance:{}", user_id);
        redis::Cmd::set(&key, balance.to_string())
            .query_async::<()>(&mut con)
            .await
            .map_err(|e| format!("Redis SET balance error: {}", e))
    }

    /// Atomically update gate_status and balance for a user in one shot.
    pub async fn set_gate_and_balance(
        &self,
        user_id: &str,
        status: GateStatus,
        balance: Decimal,
    ) -> Result<(), String> {
        let mut con = self.con.clone();
        let gate_key = format!("gate_status:{}", user_id);
        let bal_key = format!("balance:{}", user_id);
        redis::pipe()
            .set(&gate_key, status.as_str())
            .set(&bal_key, balance.to_string())
            .query_async::<()>(&mut con)
            .await
            .map_err(|e| format!("Redis pipeline SET error: {}", e))
    }

    // ── Billing backlog (Phase 5: Redis Stream overflow) ──────────────

    const BILLING_BACKLOG_KEY: &'static str = "billing:backlog";

    /// Push a billing record to the Redis Stream backlog.
    /// Called when the in-memory billing channel is full or a PG operation
    /// fails after the worker has taken records out of its channel.
    ///
    /// The stream is deliberately not trimmed: a trim can delete an
    /// unacknowledged billing record and turn a transient outage into lost
    /// revenue. Records are removed only by `ack_billing_backlog` after the
    /// PG transaction has committed.
    pub async fn backlog_billing_record(&self, record: UsageRecord) -> Result<(), String> {
        let mut con = self.con.clone();
        let json = serde_json::to_string(&record).map_err(|e| format!("Backlog serialize: {e}"))?;
        redis::cmd("XADD")
            .arg(Self::BILLING_BACKLOG_KEY)
            .arg("*")
            .arg("record")
            .arg(&json)
            .query_async::<String>(&mut con)
            .await
            .map_err(|e| format!("Redis XADD error: {e}"))?;
        Ok(())
    }

    /// Keep retrying a billing backlog write until Redis accepts it.
    ///
    /// This is the at-least-once handoff used after an in-memory record has
    /// been removed from a worker channel. A failed XADD must not be treated
    /// as a completed handoff; the record remains owned by this task until a
    /// successful XADD. The PG writer is idempotent by `request_id`, so an
    /// ambiguous XADD error can safely result in duplicate stream entries.
    pub async fn backlog_billing_record_reliably(&self, record: UsageRecord) {
        loop {
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                self.backlog_billing_record(record.clone()),
            )
            .await
            .map_err(|_| "Redis XADD timed out".to_string())
            .and_then(|result| result);
            match result {
                Ok(()) => return,
                Err(error) => {
                    tracing::error!(
                        request_id = %record.request_id,
                        error = %error,
                        "Billing backlog XADD failed — retrying"
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Read pending billing records from the backlog (non-blocking, up to `count`).
    /// Returns `(entry_id, UsageRecord)` pairs.
    pub async fn read_billing_backlog(
        &self,
        count: usize,
    ) -> Result<Vec<(String, UsageRecord)>, String> {
        let mut con = self.con.clone();
        let raw: redis::Value = redis::cmd("XREAD")
            .arg("COUNT")
            .arg(count)
            .arg("STREAMS")
            .arg(Self::BILLING_BACKLOG_KEY)
            .arg("0")
            .query_async(&mut con)
            .await
            .map_err(|e| format!("Redis XREAD error: {e}"))?;

        Ok(parse_usage_stream_records(&raw, "billing:backlog"))
    }

    /// Acknowledge and remove processed billing records from the backlog.
    pub async fn ack_billing_backlog(&self, entry_ids: &[String]) -> Result<(), String> {
        let mut con = self.con.clone();
        for id in entry_ids {
            redis::cmd("XDEL")
                .arg(Self::BILLING_BACKLOG_KEY)
                .arg(id)
                .query_async::<()>(&mut con)
                .await
                .map_err(|e| format!("Redis XDEL error: {e}"))?;
        }
        Ok(())
    }

    // ── Observability event stream (decoupled from PG) ───────────────

    const OBS_EVENTS_KEY: &'static str = "obs:events";

    /// Push an observability event to the Redis Stream.
    /// Called after every request completion — fire-and-forget via spawn.
    pub async fn push_obs_event(&self, record: UsageRecord) -> Result<(), String> {
        let mut con = self.con.clone();
        let json =
            serde_json::to_string(&record).map_err(|e| format!("Obs event serialize: {e}"))?;
        redis::cmd("XADD")
            .arg(Self::OBS_EVENTS_KEY)
            .arg("MAXLEN")
            .arg("100000")
            .arg("*")
            .arg("record")
            .arg(&json)
            .query_async::<String>(&mut con)
            .await
            .map_err(|e| format!("Redis XADD obs: {e}"))?;
        Ok(())
    }

    /// Read pending observability events from the stream (up to `count`).
    pub async fn read_obs_events(
        &self,
        count: usize,
    ) -> Result<Vec<(String, UsageRecord)>, String> {
        let mut con = self.con.clone();
        let raw: redis::Value = redis::cmd("XREAD")
            .arg("COUNT")
            .arg(count)
            .arg("STREAMS")
            .arg(Self::OBS_EVENTS_KEY)
            .arg("0")
            .query_async(&mut con)
            .await
            .map_err(|e| format!("Redis XREAD obs: {e}"))?;

        Ok(parse_usage_stream_records(&raw, "obs:events"))
    }

    /// Acknowledge (delete) processed obs events from the stream.
    pub async fn ack_obs_events(&self, entry_ids: &[String]) -> Result<(), String> {
        let mut con = self.con.clone();
        for id in entry_ids {
            redis::cmd("XDEL")
                .arg(Self::OBS_EVENTS_KEY)
                .arg(id)
                .query_async::<()>(&mut con)
                .await
                .map_err(|e| format!("Redis XDEL obs: {e}"))?;
        }
        Ok(())
    }
}

/// Background task: drains the billing backlog Redis Stream and retries
/// billing via PG. Runs every 5 seconds.
pub async fn start_billing_backlog_drain(
    cache: std::sync::Arc<RedisCache>,
    db: std::sync::Arc<crate::db::Database>,
) {
    tracing::info!("Billing backlog drain started");
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        let Ok(records) = cache.read_billing_backlog(100).await else {
            continue;
        };
        if records.is_empty() {
            continue;
        }
        let mut processed = Vec::new();
        for (entry_id, record) in &records {
            let billing_enabled = match db.get_gateway_config().await {
                Ok(config) => config.billing_enabled,
                Err(error) => {
                    tracing::warn!(
                        request_id = record.request_id,
                        error = %error.0,
                        "Backlog drain: failed to read gateway config — leaving record pending"
                    );
                    continue;
                }
            };
            match db
                .batch_insert_usage_with_billing(std::slice::from_ref(record), billing_enabled)
                .await
            {
                Ok(_) => processed.push(entry_id.clone()),
                Err(e) => {
                    tracing::warn!(
                        request_id = record.request_id,
                        error = %e.0,
                        "Backlog drain retry failed — will retry next cycle"
                    );
                }
            }
        }
        if !processed.is_empty() {
            let _ = cache.ack_billing_backlog(&processed).await;
        }
    }
}

/// Background task: consumes the observability event stream (obs:events)
/// and writes to ClickHouse. Decouples CH availability from the gateway.
pub async fn start_obs_consumer(
    ch: Option<std::sync::Arc<crate::ch_backend::ClickHouseBackend>>,
    cache: std::sync::Arc<RedisCache>,
    db: std::sync::Arc<crate::db::Database>,
) {
    let ch = match ch {
        Some(c) => c,
        None => {
            tracing::info!("ClickHouse disabled — obs consumer skipped");
            return;
        }
    };

    tracing::info!("Obs consumer started (every 5s)");
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;

        let records = match cache.read_obs_events(500).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Obs consumer: XREAD failed: {e}");
                continue;
            }
        };

        if records.is_empty() {
            continue;
        }

        let mut events = Vec::with_capacity(records.len());
        let mut entry_ids = Vec::with_capacity(records.len());
        for (eid, r) in &records {
            let total_tokens = if r.total_tokens > 0 {
                r.total_tokens
            } else {
                r.prompt_tokens + r.completion_tokens
            };
            let ts = chrono::DateTime::parse_from_rfc3339(&r.timestamp)
                .map(|dt| dt.timestamp() as u32)
                .unwrap_or_else(|_| chrono::Utc::now().timestamp() as u32);
            let pricing = if r.prompt_price > Decimal::ZERO
                || r.completion_price > Decimal::ZERO
                || r.cache_read_price > Decimal::ZERO
                || r.cache_write_tokens > 0
            {
                Some((
                    r.prompt_price,
                    r.completion_price,
                    r.cache_read_price,
                    Decimal::ZERO,
                ))
            } else {
                match db.lookup_model_pricing(&r.model).await {
                    Ok((pp, cp, crp, cwp)) => Some((pp, cp, crp, cwp)),
                    Err(error) => {
                        tracing::warn!(request_id = r.request_id, model = r.model, error = %error.0, "Obs consumer: pricing lookup failed — leaving record pending");
                        None
                    }
                }
            };
            let Some((prompt_price, completion_price, cache_read_price, cache_write_price)) =
                pricing
            else {
                continue;
            };
            let package_billing = db
                .token_request_billing_amount(&r.request_id)
                .await
                .ok()
                .flatten();
            let package_covered = package_billing
                .as_ref()
                .map(|(covered, _, _, _, _)| *covered)
                .unwrap_or(false);
            let billing_payment_mode = package_billing
                .as_ref()
                .map(|(_, _, mode, _, _)| mode.as_str())
                .or(r.billing_payment_mode.as_deref())
                .unwrap_or("prepaid");
            let billing_group_id = package_billing
                .as_ref()
                .and_then(|(_, _, _, group_id, _)| group_id.clone())
                .or_else(|| r.billing_group_id.clone());
            let billing_group_name = package_billing
                .as_ref()
                .and_then(|(_, _, _, _, group_name)| group_name.clone())
                .or_else(|| r.billing_group_name.clone());
            let package_wallet_amount =
                package_billing.as_ref().map(|(_, amount, _, _, _)| *amount);
            let cost_amount = package_wallet_amount
                .unwrap_or_else(|| {
                    Decimal::from(r.prompt_tokens) / Decimal::from(1000000) * prompt_price
                        + Decimal::from(r.completion_tokens) / Decimal::from(1000000)
                            * completion_price
                        + Decimal::from(r.cache_hit_input_tokens) / Decimal::from(1000000)
                            * cache_read_price
                        + Decimal::from(r.cache_write_tokens) / Decimal::from(1000000)
                            * cache_write_price
                })
                .to_f64()
                .unwrap_or(0.0);
            events.push(crate::ch_backend::UsageEvent {
                timestamp: ts,
                request_id: r.request_id.clone(),
                user_id: r.user_id.clone(),
                user_name: r.user_name.clone(),
                channel_id: r.channel_id.clone(),
                model: r.model.clone(),
                prompt_tokens: r.prompt_tokens,
                completion_tokens: r.completion_tokens,
                total_tokens,
                latency_ms: r.latency_ms,
                status_code: r.status_code,
                success: if r.success { 1 } else { 0 },
                api_key_name: r.api_key_name.clone(),
                api_format: r.api_format.clone(),
                stream: if r.stream { 1 } else { 0 },
                cache_hit_input_tokens: r.cache_hit_input_tokens,
                cache_write_tokens: r.cache_write_tokens,
                prompt_price: if package_covered {
                    0.0
                } else {
                    prompt_price.to_f64().unwrap_or(0.0)
                },
                completion_price: if package_covered {
                    0.0
                } else {
                    completion_price.to_f64().unwrap_or(0.0)
                },
                cache_read_price: if package_covered {
                    0.0
                } else {
                    cache_read_price.to_f64().unwrap_or(0.0)
                },
                cache_write_price: if package_covered {
                    0.0
                } else {
                    cache_write_price.to_f64().unwrap_or(0.0)
                },
                cost_amount,
                client_ip: r.client_ip.clone(),
                endpoint_id: r.endpoint_id,
                endpoint_url: r.endpoint_url.clone(),
                request_body: r.request_body.clone(),
                response_body: r.response_body.clone(),
                reasoning_body: r.reasoning_body.clone(),
                original_model: r.original_model.clone(),
                team_id: r.team_id.clone().unwrap_or_default(),
                ttft_ms: r.ttft_ms,
                billing_group_id,
                billing_group_name,
                billing_payment_mode: billing_payment_mode.to_string(),
            });
            entry_ids.push(eid.clone());
        }

        match ch.insert_usage_events(&events).await {
            Ok(()) => {
                if let Err(e) = cache.ack_obs_events(&entry_ids).await {
                    tracing::warn!("Obs consumer: XDEL failed: {e}");
                }
            }
            Err(e) => {
                tracing::warn!(
                    count = events.len(),
                    error = e,
                    "Obs consumer: CH write failed — data stays in Redis"
                );
            }
        }
    }
}

fn parse_usage_stream_records(raw: &redis::Value, stream_name: &str) -> Vec<(String, UsageRecord)> {
    fn as_str_bytes(v: &redis::Value) -> Option<String> {
        match v {
            redis::Value::BulkString(b) => Some(String::from_utf8_lossy(b).into()),
            _ => None,
        }
    }

    let mut records = Vec::new();
    match raw {
        redis::Value::Nil => return records,
        redis::Value::Array(_) => {}
        _ => {
            tracing::warn!(stream = stream_name, "Redis stream payload is not an array");
            return records;
        }
    }
    let redis::Value::Array(streams) = raw else {
        return records;
    };

    for stream in streams {
        let redis::Value::Array(stream_parts) = stream else {
            tracing::warn!(
                stream = stream_name,
                "Redis stream entry wrapper is not an array"
            );
            continue;
        };
        if stream_parts.len() < 2 {
            tracing::warn!(
                stream = stream_name,
                parts = stream_parts.len(),
                "Redis stream wrapper is too short"
            );
            continue;
        }

        let actual_stream_name =
            as_str_bytes(&stream_parts[0]).unwrap_or_else(|| stream_name.to_string());
        let redis::Value::Array(entries) = &stream_parts[1] else {
            tracing::warn!(
                stream = actual_stream_name,
                "Redis stream entries payload is not an array"
            );
            continue;
        };

        for entry in entries {
            let redis::Value::Array(parts) = entry else {
                tracing::warn!(
                    stream = actual_stream_name,
                    "Redis stream item is not an array"
                );
                continue;
            };
            if parts.len() < 2 {
                tracing::warn!(
                    stream = actual_stream_name,
                    parts = parts.len(),
                    "Redis stream item is too short"
                );
                continue;
            }

            let Some(entry_id) = as_str_bytes(&parts[0]) else {
                tracing::warn!(
                    stream = actual_stream_name,
                    "Redis stream item is missing entry id"
                );
                continue;
            };
            let redis::Value::Array(field_pairs) = &parts[1] else {
                tracing::warn!(
                    stream = actual_stream_name,
                    entry_id,
                    "Redis stream fields are not an array"
                );
                continue;
            };

            let mut found_record = false;
            for pair in field_pairs.chunks(2) {
                if pair.len() != 2 {
                    tracing::warn!(
                        stream = actual_stream_name,
                        entry_id,
                        "Redis stream field pair is incomplete"
                    );
                    continue;
                }
                if as_str_bytes(&pair[0]).as_deref() != Some("record") {
                    continue;
                }
                found_record = true;
                let Some(json) = as_str_bytes(&pair[1]) else {
                    tracing::warn!(
                        stream = actual_stream_name,
                        entry_id,
                        "Redis stream record payload is not a string"
                    );
                    continue;
                };
                match serde_json::from_str::<UsageRecord>(&json) {
                    Ok(record) => records.push((entry_id.clone(), record)),
                    Err(error) => {
                        tracing::warn!(stream = actual_stream_name, entry_id, error = %error, "Redis stream record JSON could not be parsed as UsageRecord")
                    }
                }
            }

            if !found_record {
                tracing::warn!(
                    stream = actual_stream_name,
                    entry_id,
                    "Redis stream item did not contain a record field"
                );
            }
        }
    }

    records
}

/// Build a tenant-isolated Redis key.
///
/// Format: `cache:exact:{tenant_id}:{hex(sha256(cache_key))}`
///
/// The tenant_id is part of the key itself so there is *no way* for a
/// caller to accidentally retrieve another tenant's cached data — the
/// isolation is structural, not advisory.
fn build_redis_key(tenant_id: &str, cache_key: &str) -> String {
    let hash = hex::encode(Sha256::digest(cache_key.as_bytes()));
    format!("cache:exact:{}:{}", tenant_id, hash)
}
