use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::config::types::EndpointConfig;

pub type EndpointGroup = Vec<EndpointConfig>;

#[derive(Clone)]
pub struct LoadBalancer {
    strategy: Strategy,
    counter: Arc<AtomicUsize>,
}

#[derive(Clone)]
enum Strategy {
    RoundRobin,
    WeightedRoundRobin { weights: Vec<u32>, total: u32 },
}

impl LoadBalancer {
    pub fn new(endpoints: &EndpointGroup) -> Self {
        let all_equal = endpoints.iter().all(|e| e.weight == endpoints[0].weight);

        let strategy = if all_equal {
            Strategy::RoundRobin
        } else {
            let total: u32 = endpoints.iter().map(|e| e.weight).sum();
            Strategy::WeightedRoundRobin {
                weights: endpoints.iter().map(|e| e.weight).collect(),
                total,
            }
        };

        Self {
            strategy,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn select<'a>(&self, endpoints: &'a EndpointGroup) -> Option<&'a EndpointConfig> {
        if endpoints.is_empty() {
            return None;
        }

        match &self.strategy {
            Strategy::RoundRobin => {
                let idx = self.counter.fetch_add(1, Ordering::Relaxed) % endpoints.len();
                Some(&endpoints[idx])
            }
            Strategy::WeightedRoundRobin { weights, total } => {
                let counter_val = self.counter.fetch_add(1, Ordering::Relaxed);
                let pos = counter_val % *total as usize;

                let mut cumulative = 0u32;
                let mut selected = 0;
                for (i, w) in weights.iter().enumerate() {
                    cumulative += w;
                    if pos < cumulative as usize {
                        selected = i;
                        break;
                    }
                }

                Some(&endpoints[selected])
            }
        }
    }
}

#[allow(dead_code)]
pub fn select_next_fallback(current: usize, endpoints: &EndpointGroup) -> Option<&EndpointConfig> {
    let next = (current + 1) % endpoints.len();
    if next != current {
        Some(&endpoints[next])
    } else {
        None
    }
}
