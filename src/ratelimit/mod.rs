use std::sync::Arc;
use std::time::Instant;

use dashmap::DashMap;

const WINDOW_SECS: u64 = 60;

#[derive(Clone)]
pub struct RateLimiter {
    rpm_counters: Arc<DashMap<String, Vec<Instant>>>,
    tpm_counters: Arc<DashMap<String, Vec<(Instant, u64)>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            rpm_counters: Arc::new(DashMap::new()),
            tpm_counters: Arc::new(DashMap::new()),
        }
    }

    /// Spawn a background task that periodically removes stale entries
    /// from the DashMap counters to prevent unbounded memory growth.
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

    pub fn check_rpm(&self, key: &str, limit: u64) -> Result<(), RateLimitError> {
        if limit == u64::MAX {
            return Ok(());
        }
        self.check_window(&self.rpm_counters, key, limit, WINDOW_SECS)
    }

    pub fn check_tpm(
        &self,
        key: &str,
        limit: u64,
        estimated_tokens: u64,
    ) -> Result<(), RateLimitError> {
        if limit == u64::MAX {
            return Ok(());
        }
        self.check_window_tokens(
            &self.tpm_counters,
            key,
            limit,
            WINDOW_SECS,
            estimated_tokens,
        )
    }

    fn check_window(
        &self,
        counters: &DashMap<String, Vec<Instant>>,
        key: &str,
        limit: u64,
        window_secs: u64,
    ) -> Result<(), RateLimitError> {
        let now = Instant::now();
        let mut entry = counters.entry(key.to_string()).or_default();

        entry.retain(|t| now.duration_since(*t).as_secs() < window_secs);

        if entry.len() as u64 >= limit {
            return Err(RateLimitError(format!(
                "Rate limit exceeded: {} requests per {}s window",
                limit, window_secs
            )));
        }

        entry.push(now);
        Ok(())
    }

    fn check_window_tokens(
        &self,
        counters: &DashMap<String, Vec<(Instant, u64)>>,
        key: &str,
        limit: u64,
        window_secs: u64,
        estimated_tokens: u64,
    ) -> Result<(), RateLimitError> {
        let now = Instant::now();
        let mut entry = counters.entry(key.to_string()).or_default();

        entry.retain(|(t, _)| now.duration_since(*t).as_secs() < window_secs);

        let current_tokens: u64 = entry.iter().map(|(_, t)| t).sum();

        if current_tokens + estimated_tokens > limit {
            return Err(RateLimitError(format!(
                "Token rate limit exceeded: {} tokens per {}s window",
                limit, window_secs
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
