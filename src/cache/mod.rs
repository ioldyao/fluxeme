use sha2::{Digest, Sha256};

/// Redis-backed exact-response cache with mandatory tenant isolation.
///
/// Every key is prefixed with the tenant/user ID so that different tenants
/// physically occupy separate keys — there is no shared-namespace look-up
/// that could accidentally return another tenant's cached response.
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
