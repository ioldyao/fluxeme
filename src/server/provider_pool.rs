use std::collections::HashMap;
use std::sync::Arc;

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

    /// Try to acquire a slot immediately.  If full, return `Err` so the
    /// caller responds with 503 and the client can retry.
    pub fn try_acquire(self: Arc<Self>) -> Result<ProviderPermit, ()> {
        let permit = self.semaphore.clone().try_acquire_owned().map_err(|_| ())?;
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
