use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Utc;

#[derive(Clone, Debug)]
pub struct ActiveRequest {
    pub request_id: String,
    pub model: String,
    pub channel_id: String,
    pub endpoint_id: Option<i64>,
    pub accepted_at: String,
    pub upstream_started_at: Option<String>,
    pub first_byte_at: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct FlowSnapshot {
    pub as_of: String,
    pub in_flight: u64,
    pub upstream_generating: u64,
    pub upstream_outputting: u64,
}

#[derive(Clone, Default)]
pub struct FlowTracker {
    inner: Arc<Mutex<HashMap<String, ActiveRequest>>>,
}

impl FlowTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_accepted(
        &self,
        request_id: String,
        model: String,
        channel_id: String,
        endpoint_id: Option<i64>,
        accepted_at: String,
    ) {
        self.inner.lock().unwrap().insert(
            request_id.clone(),
            ActiveRequest {
                request_id,
                model,
                channel_id,
                endpoint_id,
                accepted_at,
                upstream_started_at: None,
                first_byte_at: None,
            },
        );
    }

    pub fn mark_upstream_started(&self, request_id: &str, at: String) {
        if let Some(req) = self.inner.lock().unwrap().get_mut(request_id) {
            req.upstream_started_at = Some(at);
        }
    }

    pub fn mark_first_byte(&self, request_id: &str, at: String) {
        if let Some(req) = self.inner.lock().unwrap().get_mut(request_id) {
            req.first_byte_at = Some(at);
        }
    }

    pub fn mark_completed(&self, request_id: &str) {
        self.inner.lock().unwrap().remove(request_id);
    }

    pub fn snapshot(&self) -> FlowSnapshot {
        let guard = self.inner.lock().unwrap();
        let mut in_flight = 0u64;
        let mut upstream_generating = 0u64;
        let mut upstream_outputting = 0u64;
        for req in guard.values() {
            in_flight += 1;
            if req.upstream_started_at.is_some() {
                if req.first_byte_at.is_some() {
                    upstream_outputting += 1;
                } else {
                    upstream_generating += 1;
                }
            }
        }
        FlowSnapshot {
            as_of: Utc::now().to_rfc3339(),
            in_flight,
            upstream_generating,
            upstream_outputting,
        }
    }
}
