use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Per-provider concurrency pool.  Each LLM provider (openai, deepseek, vllm,
/// etc.) gets its own semaphore — a saturated provider never starves others.
///
/// Replaces the global `tower::ConcurrencyLimitLayer` with fine-grained,
/// provider-aware isolation.
pub struct ProviderPool {
    name: String,
    semaphore: Arc<Semaphore>,
}

impl ProviderPool {
    pub fn new(name: &str, max_concurrent: usize) -> Self {
        Self {
            name: name.to_string(),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Acquire a slot, waiting up to `max_wait` if the pool is full.
    /// LLM requests take 30–60s — queuing a few seconds for a slot to
    /// free up beats getting a 503 and retrying from scratch.
    pub async fn acquire(self: Arc<Self>, max_wait: Duration) -> Result<ProviderPermit, ()> {
        let permit = tokio::time::timeout(max_wait, self.semaphore.clone().acquire_owned())
            .await
            .map_err(|_| ())?  // timeout
            .map_err(|_| ())?; // semaphore closed (should not happen)
        Ok(ProviderPermit {
            _permit: permit,
            _pool: self,
        })
    }
}

/// RAII guard — the provider slot is released when this drops.
pub struct ProviderPermit {
    _permit: OwnedSemaphorePermit,
    #[allow(dead_code)]
    _pool: Arc<ProviderPool>,
}

/// All provider pools, keyed by provider name ("openai", "deepseek", …).
pub type ProviderPools = Arc<HashMap<String, Arc<ProviderPool>>>;

/// Create pools for each registered provider.  Providers not listed use
/// `default_max_concurrent`.
pub fn init_provider_pools(
    names: &[&str],
    default_max_concurrent: usize,
) -> ProviderPools {
    let map: HashMap<_, _> = names
        .iter()
        .map(|name| {
            (
                name.to_string(),
                Arc::new(ProviderPool::new(name, default_max_concurrent)),
            )
        })
        .collect();
    Arc::new(map)
}
