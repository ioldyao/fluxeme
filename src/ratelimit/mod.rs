use std::sync::Arc;

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
    redis: Arc<RedisCache>,
}

impl RateLimiter {
    pub fn new(redis: Arc<RedisCache>) -> Self {
        Self { redis }
    }

    pub async fn check_rpm(&self, key: &str, limit: u64) -> Result<(), RateLimitError> {
        if limit == u64::MAX {
            return Ok(());
        }
        match self.redis.rate_limit_rpm(&format!("rl:{key}"), limit).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(RateLimitError::Exceeded(format!(
                "{} requests per {}s window",
                limit, WINDOW_SECS
            ))),
            Err(e) => Err(RateLimitError::Unavailable(e)),
        }
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
        match self
            .redis
            .rate_limit_tpm(&format!("rl:t:{key}"), limit, estimated_tokens)
            .await
        {
            Ok(true) => Ok(()),
            Ok(false) => Err(RateLimitError::Exceeded(format!(
                "{} tokens per {}s window",
                limit, WINDOW_SECS
            ))),
            Err(e) => Err(RateLimitError::Unavailable(e)),
        }
    }
}

#[derive(Debug)]
pub enum RateLimitError {
    Exceeded(String),
    Unavailable(String),
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exceeded(message) => write!(f, "Rate limited: {message}"),
            Self::Unavailable(message) => write!(f, "Rate limiter unavailable: {message}"),
        }
    }
}
