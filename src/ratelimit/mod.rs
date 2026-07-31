use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;

use crate::cache::RedisCache;

const WINDOW_SECS: u64 = 60;

/// Rate limiter with distributed (Redis) fixed-window counting and an
/// in-memory (DashMap) fallback when Redis is unavailable or disabled.
///
/// The Redis key is namespaced as `rl:{key}` / `rl:t:{key}` with a 60s TTL,
/// so all gateway instances share the same counter — a user cannot bypass
/// limits by spreading requests across instances.
#[derive(Clone)]
pub struct RateLimiter {
    rpm_counters: Arc<DashMap<String, Vec<Instant>>>,
    tpm_counters: Arc<DashMap<String, Vec<(Instant, u64)>>>,
    redis: Option<Arc<RedisCache>>,
}

impl RateLimiter {
    /// `redis` is `Some` only when the shared Redis cache is enabled.
    /// When `None` (or on Redis errors), counting falls back to local DashMap.
    pub fn new(redis: Option<Arc<RedisCache>>) -> Self {
        Self {
            rpm_counters: Arc::new(DashMap::new()),
            tpm_counters: Arc::new(DashMap::new()),
            redis,
        }
    }

    /// Spawn a background task that periodically removes stale entries
    /// from the DashMap counters to prevent unbounded memory growth.
    /// Only meaningful when the local fallback is in use.
    pub fn start_cleanup_task(self: &Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            // Delay first cleanup to avoid startup overhead
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                this.cleanup();
            }
        });
    }

    fn cleanup(&self) {
        let now = Instant::now();
        let window = std::time::Duration::from_secs(WINDOW_SECS);

        self.rpm_counters.retain(|_, timestamps| {
            timestamps.retain(|t| now.duration_since(*t) < window);
            !timestamps.is_empty()
        });

        self.tpm_counters.retain(|_, entries| {
            entries.retain(|(t, _)| now.duration_since(*t) < window);
            !entries.is_empty()
        });
    }

    pub async fn check_rpm(&self, key: &str, limit: u64) -> Result<(), RateLimitError> {
        if limit == u64::MAX {
            return Ok(());
        }

        // Distributed path: share the counter across instances via Redis
        if let Some(redis) = &self.redis {
            match redis.rate_limit_rpm(&format!("rl:{key}"), limit).await {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    return Err(RateLimitError(format!(
                        "Rate limit exceeded: {} requests per {}s window",
                        limit, WINDOW_SECS
                    )))
                }
                Err(e) => {
                    tracing::warn!("Redis RPM rate limit failed, using local fallback: {}", e);
                }
            }
        }

        // Local fallback path
        let now = Instant::now();
        let mut entry = self.rpm_counters.entry(key.to_string()).or_default();

        entry.retain(|t| now.duration_since(*t).as_secs() < WINDOW_SECS);

        if entry.len() as u64 >= limit {
            return Err(RateLimitError(format!(
                "Rate limit exceeded: {} requests per {}s window",
                limit, WINDOW_SECS
            )));
        }

        entry.push(now);
        Ok(())
    }

    pub async fn check_tpm(
        &self,
        key: &str,
        limit: u64,
        estimated_tokens: u64,
    ) -> Result<(), RateLimitError> {
        if limit == u64::MAX {
            return Ok(());
        }

        // Distributed path: share the counter across instances via Redis
        if let Some(redis) = &self.redis {
            match redis
                .rate_limit_tpm(&format!("rl:t:{key}"), limit, estimated_tokens)
                .await
            {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    return Err(RateLimitError(format!(
                        "Token rate limit exceeded: {} tokens per {}s window",
                        limit, WINDOW_SECS
                    )))
                }
                Err(e) => {
                    tracing::warn!("Redis TPM rate limit failed, using local fallback: {}", e);
                }
            }
        }

        // Local fallback path
        let now = Instant::now();
        let mut entry = self.tpm_counters.entry(key.to_string()).or_default();

        entry.retain(|(t, _)| now.duration_since(*t).as_secs() < WINDOW_SECS);

        let current_tokens: u64 = entry.iter().map(|(_, t)| t).sum();

        if current_tokens + estimated_tokens > limit {
            return Err(RateLimitError(format!(
                "Token rate limit exceeded: {} tokens per {}s window",
                limit, WINDOW_SECS
            )));
        }

        entry.push((now, estimated_tokens));
        Ok(())
    }
}

#[derive(Debug)]
pub struct RateLimitError(pub String);

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Rate limited: {}", self.0)
    }
}
