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
    half_open_in_flight: bool,
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
                half_open_in_flight: false,
            })),
            threshold,
            cooldown_secs,
        }
    }

    /// Whether this endpoint can receive traffic.
    ///
    /// An Open breaker may grant exactly one trial request after its cooldown.
    /// The claim is made while holding the write lock so concurrent requests
    /// cannot all pass through the HalfOpen state.
    pub fn is_available(&self) -> bool {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if !inner.enabled {
            return false;
        }
        match inner.status {
            BreakerStatus::Closed => true,
            BreakerStatus::HalfOpen => {
                if inner.half_open_in_flight {
                    false
                } else {
                    inner.half_open_in_flight = true;
                    true
                }
            }
            BreakerStatus::Open => {
                if inner
                    .last_failure
                    .is_some_and(|t| t.elapsed().as_secs() >= self.cooldown_secs)
                {
                    inner.status = BreakerStatus::HalfOpen;
                    inner.failure_count = 0;
                    inner.half_open_in_flight = true;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Check availability without claiming a HalfOpen trial.
    pub fn is_available_readonly(&self) -> bool {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        if !inner.enabled {
            return false;
        }
        match inner.status {
            BreakerStatus::Closed => true,
            BreakerStatus::HalfOpen => !inner.half_open_in_flight,
            BreakerStatus::Open => inner
                .last_failure
                .is_some_and(|t| t.elapsed().as_secs() >= self.cooldown_secs),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.read().unwrap_or_else(|e| e.into_inner()).enabled
    }

    pub fn is_healthy(&self) -> bool {
        let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
        inner.enabled && inner.status == BreakerStatus::Closed
    }

    pub fn set_enabled(&self, enabled: bool) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if enabled && !inner.enabled {
            inner.status = BreakerStatus::Closed;
            inner.failure_count = 0;
            inner.last_failure = None;
            inner.half_open_in_flight = false;
        }
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
        inner.half_open_in_flight = false;
    }

    pub fn record_failure(&self) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        inner.failure_count += 1;
        inner.last_failure = Some(Instant::now());
        inner.half_open_in_flight = false;
        if inner.status == BreakerStatus::HalfOpen || inner.failure_count >= self.threshold {
            inner.status = BreakerStatus::Open;
        }
    }

    /// Release a recovery probe without marking the endpoint healthy.
    /// Non-liveness probe errors (for example model/auth errors) must not
    /// erase an existing Open state.
    pub fn release_probe(&self) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if inner.status == BreakerStatus::HalfOpen {
            inner.status = BreakerStatus::Open;
        }
        inner.half_open_in_flight = false;
    }

    /// Claim an explicit recovery probe. Business traffic never calls this;
    /// unlike `is_available`, it only admits an Open endpoint after cooldown.
    pub fn claim_probe(&self) -> bool {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if !inner.enabled || inner.half_open_in_flight {
            return false;
        }
        match inner.status {
            BreakerStatus::Open
                if inner
                    .last_failure
                    .is_some_and(|last| last.elapsed().as_secs() >= self.cooldown_secs) =>
            {
                inner.status = BreakerStatus::HalfOpen;
                inner.half_open_in_flight = true;
                true
            }
            BreakerStatus::HalfOpen => {
                inner.half_open_in_flight = true;
                true
            }
            _ => false,
        }
    }

    fn preserve_runtime_state_from(&self, old: &Self, enabled: bool) {
        let old = old.inner.read().unwrap_or_else(|e| e.into_inner());
        let mut current = self.inner.write().unwrap_or_else(|e| e.into_inner());
        current.enabled = enabled;
        if enabled && old.enabled {
            current.status = old.status;
            current.failure_count = old.failure_count;
            current.last_failure = old.last_failure;
            // Never carry an in-flight claim across a config rebuild. The
            // next probe/request must explicitly claim a fresh lease.
            current.half_open_in_flight = false;
        }
    }
}

// ── Health-aware Balancer ──────────────────────────────────────────

#[derive(Clone)]
enum Strategy {
    RoundRobin,
    WeightedRoundRobin,
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

        let all_equal = endpoints
            .first()
            .map(|first| endpoints.iter().all(|e| e.weight == first.weight))
            .unwrap_or(true);
        let total = endpoints.iter().map(|e| e.weight).sum::<u32>();
        let strategy = if all_equal || total == 0 {
            Strategy::RoundRobin
        } else {
            Strategy::WeightedRoundRobin
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
    /// Returns `None` when no endpoint can currently receive traffic.
    pub fn select(&self) -> Option<(usize, &EndpointConfig)> {
        let available: Vec<usize> = (0..self.endpoints.len())
            .filter(|&i| self.breakers[i].is_available())
            .collect();

        if available.is_empty() {
            return None;
        }

        let idx = self.pick_index(&available);
        Some((available[idx], &self.endpoints[available[idx]]))
    }

    /// Whether this balancer currently has an endpoint available for traffic.
    /// This deliberately does not advance the load-balancing counter.
    pub fn has_available_endpoint(&self) -> bool {
        self.breakers
            .iter()
            .any(|breaker| breaker.is_available_readonly())
    }

    /// Whether a closed, healthy endpoint is available for business traffic.
    /// Open breakers are never promoted by this check; recovery is owned by the
    /// explicit probe lease path.
    pub fn has_healthy_endpoint(&self) -> bool {
        self.breakers.iter().any(|breaker| breaker.is_healthy())
    }

    /// Select only closed endpoints. Unlike `select`, this never claims a
    /// half-open trial for business traffic.
    pub fn select_healthy(&self) -> Option<(usize, &EndpointConfig)> {
        self.select_healthy_excluding(&std::collections::HashSet::new())
    }

    pub fn select_healthy_excluding(
        &self,
        excluded_endpoint_ids: &std::collections::HashSet<i64>,
    ) -> Option<(usize, &EndpointConfig)> {
        self.select_healthy_excluding_indexes(
            excluded_endpoint_ids,
            &std::collections::HashSet::new(),
        )
    }

    pub fn select_healthy_excluding_indexes(
        &self,
        excluded_endpoint_ids: &std::collections::HashSet<i64>,
        excluded_indexes: &std::collections::HashSet<usize>,
    ) -> Option<(usize, &EndpointConfig)> {
        let available: Vec<usize> = (0..self.endpoints.len())
            .filter(|&i| {
                self.breakers[i].is_healthy()
                    && !excluded_indexes.contains(&i)
                    && self.endpoints[i]
                        .id
                        .is_none_or(|id| !excluded_endpoint_ids.contains(&id))
            })
            .collect();
        if available.is_empty() {
            return None;
        }
        let idx = self.pick_index(&available);
        Some((available[idx], &self.endpoints[available[idx]]))
    }

    pub fn claim_probe_endpoint(&self, idx: usize) -> Option<(usize, &EndpointConfig)> {
        self.breakers
            .get(idx)
            .filter(|breaker| breaker.claim_probe())
            .and_then(|_| self.endpoints.get(idx).map(|endpoint| (idx, endpoint)))
    }

    /// Build a new descriptor view while retaining breaker state for endpoints
    /// with the same database id (or stable URL identity when no id exists).
    pub fn rebuild_preserving_state(&self, endpoints: &EndpointGroup) -> Self {
        let rebuilt = Self::new(endpoints);
        for (new_idx, endpoint) in endpoints.iter().enumerate() {
            let old_idx = self
                .endpoints
                .iter()
                .position(|old| match (old.id, endpoint.id) {
                    (Some(old_id), Some(new_id)) => old_id == new_id,
                    _ => old.url == endpoint.url && old.full_url == endpoint.full_url,
                });
            if let Some(old_idx) = old_idx {
                rebuilt.breakers[new_idx]
                    .preserve_runtime_state_from(&self.breakers[old_idx], endpoint.enabled);
            }
        }
        rebuilt
    }

    fn pick_index(&self, candidates: &[usize]) -> usize {
        match &self.strategy {
            Strategy::RoundRobin => self.counter.fetch_add(1, Ordering::Relaxed) % candidates.len(),
            Strategy::WeightedRoundRobin { .. } => {
                let total: u32 = candidates
                    .iter()
                    .map(|&index| self.endpoints[index].weight)
                    .sum();
                if total == 0 {
                    return self.counter.fetch_add(1, Ordering::Relaxed) % candidates.len();
                }

                let counter_val = self.counter.fetch_add(1, Ordering::Relaxed);
                let pos = counter_val % total as usize;
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

    pub fn release_probe(&self, idx: usize) {
        if let Some(b) = self.breakers.get(idx) {
            b.release_probe();
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

    pub fn rebuild_preserving_state(&self, endpoints: &EndpointGroup) -> Self {
        Self {
            inner: self.inner.rebuild_preserving_state(endpoints),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint(id: i64, enabled: bool) -> EndpointConfig {
        EndpointConfig {
            id: Some(id),
            url: format!("https://example-{id}.test/v1"),
            api_key: String::new(),
            weight: 1,
            timeout_secs: None,
            enabled,
            full_url: false,
        }
    }

    #[test]
    fn open_endpoint_is_excluded_when_healthy_peers_exist() {
        let endpoints = vec![endpoint(1, true), endpoint(2, true), endpoint(3, true)];
        let balancer = HealthAwareBalancer::new(&endpoints);
        for _ in 0..3 {
            balancer.record_failure(0);
        }

        for _ in 0..12 {
            let (idx, _) = balancer
                .select()
                .expect("healthy peers should remain selectable");
            assert_ne!(idx, 0);
        }
    }

    #[test]
    fn all_open_endpoints_are_not_selected_as_fallback() {
        let endpoints = vec![endpoint(1, true), endpoint(2, true)];
        let balancer = HealthAwareBalancer::new(&endpoints);
        for idx in 0..endpoints.len() {
            for _ in 0..3 {
                balancer.record_failure(idx);
            }
        }

        assert!(balancer.select().is_none());
        assert!(!balancer.has_available_endpoint());
    }

    #[test]
    fn disabled_endpoint_is_never_available() {
        let endpoints = vec![endpoint(1, false)];
        let balancer = HealthAwareBalancer::new(&endpoints);

        assert!(balancer.select().is_none());
        assert!(!balancer.has_available_endpoint());
    }

    #[test]
    fn readonly_health_check_does_not_consume_half_open_trial() {
        let breaker = CircuitBreaker::new(true, 1, 0);
        breaker.record_failure();

        assert!(breaker.is_available_readonly());
        assert!(breaker.is_available());
        assert!(!breaker.is_available());
    }

    #[test]
    fn healthy_selection_never_promotes_open_endpoint() {
        let endpoints = vec![endpoint(1, true), endpoint(2, true)];
        let balancer = HealthAwareBalancer::new(&endpoints);
        for _ in 0..3 {
            balancer.record_failure(0);
        }

        for _ in 0..8 {
            let (idx, _) = balancer.select_healthy().expect("healthy peer remains");
            assert_eq!(idx, 1);
        }
        assert_eq!(balancer.breakers()[0].status(), BreakerStatus::Open);
    }

    #[test]
    fn probe_claim_is_not_available_before_cooldown() {
        let breaker = CircuitBreaker::new(true, 1, 30);
        breaker.record_failure();
        assert!(!breaker.claim_probe());
        assert_eq!(breaker.status(), BreakerStatus::Open);
    }

    #[test]
    fn re_enabling_endpoint_resets_breaker_state() {
        let breaker = CircuitBreaker::new(true, 1, 30);
        breaker.record_failure();
        breaker.set_enabled(false);
        breaker.set_enabled(true);

        assert_eq!(breaker.status(), BreakerStatus::Closed);
        assert!(breaker.is_available());
    }
}
