use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::config::types::EndpointConfig;

pub type EndpointGroup = Vec<EndpointConfig>;

// ── Circuit Breaker ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BreakerStatus {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug)]
struct BreakerInner {
    enabled: bool,
    status: BreakerStatus,
    failure_count: u32,
    last_failure: Option<Instant>,
}

#[derive(Debug)]
pub struct CircuitBreaker {
    inner: Arc<RwLock<BreakerInner>>,
    threshold: u32,
    cooldown_secs: u64,
}

impl CircuitBreaker {
    pub fn new(enabled: bool, threshold: u32, cooldown_secs: u64) -> Self {
        Self {
            inner: Arc::new(RwLock::new(BreakerInner {
                enabled,
                status: BreakerStatus::Closed,
                failure_count: 0,
                last_failure: None,
            })),
            threshold,
            cooldown_secs,
        }
    }

    /// Whether this endpoint can receive traffic.
    pub fn is_available(&self) -> bool {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        if !inner.enabled {
            return false;
        }
        match inner.status {
            BreakerStatus::Closed => true,
            BreakerStatus::HalfOpen => true,
            BreakerStatus::Open => {
                if let Some(t) = inner.last_failure {
                    if t.elapsed().as_secs() >= self.cooldown_secs {
                        drop(inner);
                        self.try_half_open();
                        return true;
                    }
                }
                false
            }
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).enabled
    }

    pub fn set_enabled(&self, enabled: bool) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.enabled = enabled;
    }

    #[allow(dead_code)]
    pub fn status(&self) -> BreakerStatus {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).status
    }

    pub fn record_success(&self) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.failure_count = 0;
        inner.status = BreakerStatus::Closed;
    }

    pub fn record_failure(&self) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.failure_count += 1;
        inner.last_failure = Some(Instant::now());
        if inner.failure_count >= self.threshold {
            inner.status = BreakerStatus::Open;
        }
    }

    fn try_half_open(&self) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if inner.status == BreakerStatus::Open {
            inner.status = BreakerStatus::HalfOpen;
            inner.failure_count = 0;
        }
    }
}

// ── Health-aware Balancer ──────────────────────────────────────────

#[derive(Clone)]
enum Strategy {
    RoundRobin,
    WeightedRoundRobin { total: u32 },
}

#[derive(Clone)]
pub struct HealthAwareBalancer {
    endpoints: EndpointGroup,
    breakers: Vec<Arc<CircuitBreaker>>,
    strategy: Strategy,
    counter: Arc<AtomicUsize>,
}

impl HealthAwareBalancer {
    pub fn new(endpoints: &EndpointGroup) -> Self {
        let breakers: Vec<_> = endpoints
            .iter()
            .map(|ep| Arc::new(CircuitBreaker::new(ep.enabled, 3, 30)))
            .collect();

        let all_equal = endpoints.iter().all(|e| e.weight == endpoints[0].weight);
        let strategy = if all_equal {
            Strategy::RoundRobin
        } else {
            let total: u32 = endpoints.iter().map(|e| e.weight).sum();
            Strategy::WeightedRoundRobin { total }
        };

        Self {
            endpoints: endpoints.clone(),
            breakers,
            strategy,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[allow(dead_code)]
    pub fn endpoint_count(&self) -> usize {
        self.endpoints.len()
    }

    pub fn breakers(&self) -> &[Arc<CircuitBreaker>] {
        &self.breakers
    }

    /// Pick an available endpoint index + the endpoint config.
    /// Returns `None` only if the group is empty.
    pub fn select(&self) -> Option<(usize, &EndpointConfig)> {
        let available: Vec<usize> = (0..self.endpoints.len())
            .filter(|&i| self.breakers[i].is_available())
            .collect();

        let candidates = if available.is_empty() {
            (0..self.endpoints.len())
                .filter(|&i| self.breakers[i].is_enabled())
                .collect::<Vec<_>>()
        } else {
            available
        };

        if candidates.is_empty() {
            return None;
        }

        let idx = self.pick_index(&candidates);
        Some((candidates[idx], &self.endpoints[candidates[idx]]))
    }

    fn pick_index(&self, candidates: &[usize]) -> usize {
        match &self.strategy {
            Strategy::RoundRobin => self.counter.fetch_add(1, Ordering::Relaxed) % candidates.len(),
            Strategy::WeightedRoundRobin { total } => {
                let counter_val = self.counter.fetch_add(1, Ordering::Relaxed);
                let pos = counter_val % *total as usize;

                let mut cumulative = 0u32;
                for (i, &ci) in candidates.iter().enumerate() {
                    cumulative += self.endpoints[ci].weight;
                    if pos < cumulative as usize {
                        return i;
                    }
                }
                candidates.len() - 1
            }
        }
    }

    pub fn record_success(&self, idx: usize) {
        if let Some(b) = self.breakers.get(idx) {
            b.record_success();
        }
    }

    pub fn record_failure(&self, idx: usize) {
        if let Some(b) = self.breakers.get(idx) {
            b.record_failure();
        }
    }

    #[allow(dead_code)]
    pub fn endpoint(&self, idx: usize) -> Option<&EndpointConfig> {
        self.endpoints.get(idx)
    }

    /// Return all configured endpoints for administrative health checks.
    /// This intentionally bypasses load-balancing selection.
    pub fn endpoints(&self) -> &[EndpointConfig] {
        &self.endpoints
    }
}

// ── Legacy LoadBalancer (delegate, kept for compat) ────────────────

#[derive(Clone)]
pub struct LoadBalancer {
    inner: HealthAwareBalancer,
}

impl LoadBalancer {
    pub fn new(endpoints: &EndpointGroup) -> Self {
        Self {
            inner: HealthAwareBalancer::new(endpoints),
        }
    }

    #[allow(dead_code)]
    pub fn select<'a>(&'a self, _endpoints: &'a EndpointGroup) -> Option<&'a EndpointConfig> {
        self.inner.select().map(|(_, ep)| ep)
    }

    /// Expose inner balancer for health/status queries.
    pub fn as_health_aware(&self) -> &HealthAwareBalancer {
        &self.inner
    }
}
