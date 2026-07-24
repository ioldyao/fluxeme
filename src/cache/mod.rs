use sha2::{Digest, Sha256};

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
        let client = redis::Client::open(redis_url).map_err(|e| format!("Redis URL error: {}", e))?;
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
        Self { con: None, default_ttl_secs: 0 }
    }

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
        let ttl = if ttl_secs > 0 { ttl_secs } else { self.default_ttl_secs };
        redis::Cmd::set_ex(&redis_key, value, ttl)
            .query_async::<()>(&mut con)
            .await
            .map_err(|e| format!("Redis SET error: {}", e))
    }

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
