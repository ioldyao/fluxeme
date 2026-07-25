use std::time::Duration;

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
pub fn compute_gate_status(balance: f64, frozen: f64) -> GateStatus {
    if balance - frozen < 0.0001 {
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
/// When the cache is disabled (`enabled: false` in config) the `noop()`
/// sentinel is used — all operations return `None` / `Ok(())` without
/// touching Redis.
pub struct RedisCache {
    con: Option<redis::aio::MultiplexedConnection>,
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
            con: Some(con),
            default_ttl_secs,
        })
    }

    /// No-op cache — all operations are implicit no-ops.
    pub fn noop() -> Self {
        Self {
            con: None,
            default_ttl_secs: 0,
        }
    }

    #[allow(dead_code)]
    pub fn is_enabled(&self) -> bool {
        self.con.is_some()
    }

    /// Retrieve a cached value for the given tenant.
    ///
    /// The key is constructed as `cache:exact:{tenant_id}:{sha256(cache_key)}`
    /// so the tenant ID is an *enforced part of the key itself*, not metadata
    /// that could be accidentally omitted from the query.
    pub async fn get(&self, tenant_id: &str, cache_key: &str) -> Result<Option<String>, String> {
        let mut con = match self.con.clone() {
            Some(c) => c,
            None => return Ok(None),
        };
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
        let mut con = match self.con.clone() {
            Some(c) => c,
            None => return Ok(()),
        };
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
    /// or cache disabled) — the caller should fall back to PostgreSQL.
    pub async fn get_gate_status(&self, user_id: &str) -> Result<Option<GateStatus>, String> {
        let mut con = match self.con.clone() {
            Some(c) => c,
            None => return Ok(None),
        };
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
        let mut con = match self.con.clone() {
            Some(c) => c,
            None => return Ok(()),
        };
        let key = format!("gate_status:{}", user_id);
        redis::Cmd::set(&key, status.as_str())
            .query_async::<()>(&mut con)
            .await
            .map_err(|e| format!("Redis SET gate_status error: {}", e))
    }

    /// Write the current balance to Redis for fast read by the inspection
    /// task (persistent, no TTL).
    #[allow(dead_code)]
    pub async fn set_balance(&self, user_id: &str, balance: f64) -> Result<(), String> {
        let mut con = match self.con.clone() {
            Some(c) => c,
            None => return Ok(()),
        };
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
        balance: f64,
    ) -> Result<(), String> {
        let mut con = match self.con.clone() {
            Some(c) => c,
            None => return Ok(()),
        };
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
    /// Called when the in-memory billing channel is full.
    ///
    /// # Design boundary
    /// When this function returns `Err`, the billing record never enters the
    /// PG billing worker and therefore never generates a wallet deduction.
    /// This is **revenue loss** (not charged), never an incorrect charge:
    ///
    /// - The user already received their API response (200 OK) before this
    ///   code runs — they are unaware of the lost billing record.
    /// - Bills, wallet balances, and usage history remain correct because
    ///   they all read from PG, which never received this record either.
    /// - The only observable effect is that dashboard / routing panel
    ///   aggregate counts may slightly under-report (1-N missing events).
    ///
    /// Trigger: billing channel full (PG workers saturated) AND Redis
    /// unavailable simultaneously — a compound failure with near-zero
    /// probability in practice (AOF persistence protects against restart).
    ///
    /// To eliminate this loss window entirely, add a local persistence
    /// fallback (e.g. sled) before the Redis XADD call.
    pub async fn backlog_billing_record(&self, record: UsageRecord) -> Result<(), String> {
        let mut con = match self.con.clone() {
            Some(c) => c,
            None => return Err("Redis cache disabled".into()),
        };
        let json = serde_json::to_string(&record)
            .map_err(|e| format!("Backlog serialize: {e}"))?;
        redis::cmd("XADD")
            .arg(Self::BILLING_BACKLOG_KEY)
            .arg("MAXLEN")
            .arg("100000")
            .arg("*")
            .arg("record")
            .arg(&json)
            .query_async::<String>(&mut con)
            .await
            .map_err(|e| format!("Redis XADD error: {e}"))?;
        Ok(())
    }

    /// Read pending billing records from the backlog (non-blocking, up to `count`).
    /// Returns `(entry_id, UsageRecord)` pairs.
    pub async fn read_billing_backlog(
        &self,
        count: usize,
    ) -> Result<Vec<(String, UsageRecord)>, String> {
        let mut con = match self.con.clone() {
            Some(c) => c,
            None => return Ok(Vec::new()),
        };
        let raw: redis::Value = redis::cmd("XREAD")
            .arg("COUNT")
            .arg(count)
            .arg("STREAMS")
            .arg(Self::BILLING_BACKLOG_KEY)
            .arg("0")
            .query_async(&mut con)
            .await
            .map_err(|e| format!("Redis XREAD error: {e}"))?;

        fn as_str_bytes(v: &redis::Value) -> Option<String> {
            match v {
                redis::Value::BulkString(b) => Some(String::from_utf8_lossy(b).into()),
                _ => None,
            }
        }

        let mut records = Vec::new();
        if let redis::Value::Array(streams) = &raw {
            for s in streams {
                if let redis::Value::Array(entries) = s {
                    for entry in entries.iter().skip(1) {
                        if let redis::Value::Array(parts) = entry {
                            if parts.len() >= 2 {
                                let entry_id = as_str_bytes(&parts[0]).unwrap_or_default();
                                if let redis::Value::Array(field_pairs) = &parts[1] {
                                    for ch in field_pairs.chunks(2) {
                                        if ch.len() == 2
                                            && as_str_bytes(&ch[0]).as_deref() == Some("record")
                                        {
                                            if let Some(json) = as_str_bytes(&ch[1]) {
                                                if let Ok(r) =
                                                    serde_json::from_str::<UsageRecord>(&json)
                                                {
                                                    records.push((entry_id.clone(), r));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(records)
    }

    /// Acknowledge and remove processed billing records from the backlog.
    pub async fn ack_billing_backlog(&self, entry_ids: &[String]) -> Result<(), String> {
        let mut con = match self.con.clone() {
            Some(c) => c,
            None => return Ok(()),
        };
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
        let Ok(records) = cache.read_billing_backlog(100).await else { continue };
        if records.is_empty() {
            continue;
        }
        let mut processed = Vec::new();
        for (entry_id, record) in &records {
            // Re-read billing_enabled (the drain runs independently)
            let billing_enabled = db
                .get_gateway_config()
                .await
                .map(|c| c.billing_enabled)
                .unwrap_or(false);
            match db
                .batch_insert_usage_with_billing(&[record.clone()], billing_enabled)
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
