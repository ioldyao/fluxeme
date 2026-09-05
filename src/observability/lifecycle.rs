//! Request lifecycle: exactly-once finalization of one authenticated LLM
//! request into a single `GatewayRequestEvent`, with a non-blocking Drop
//! fallback for unfinalized requests.
//!
//! The gateway rule is: every request that successfully authenticates and
//! enters the LLM data plane must produce exactly one gateway request event.
//! `RequestLifecycle` enforces that with an atomic "finalized" flag — all
//! `finalize_*` calls are idempotent and only the first wins. If the value is
//! dropped without an explicit finalize (panic, abort, a missed exit path)
//! `Drop` synthesises `failed / 500 / gateway / unfinalized_request` and hands
//! it to the recorder with a non-blocking `try_send`. Drop is deliberately not
//! crash-safe (no WAL): a hard kill can still lose an in-flight event, which is
//! acceptable for observability telemetry.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use super::gateway_events::{GatewayAttemptEvent, GatewayEventRecorder, GatewayRequestEvent};

/// HTTP method, path and identity captured for the request event.
#[derive(Clone, Debug)]
pub struct RequestMeta {
    pub request_id: String,
    pub method: String,
    pub path: String,
    pub api_format: String,
    pub stream: bool,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
}

/// Identity + request facts captured right after authentication.
#[derive(Clone, Debug)]
pub struct RequestIdentity {
    pub user_id: String,
    pub user_name: String,
    pub team_id: Option<String>,
    pub api_key_id: Option<String>,
    pub api_key_name: String,
    pub requested_model: String,
    pub billing_payment_mode: Option<String>,
}

/// Terminal lifecycle outcome. Mirrors the status vocabulary of the
/// observability design: succeeded / rejected / failed / cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleStatus {
    Succeeded,
    Rejected,
    Failed,
    Cancelled,
}

impl LifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            LifecycleStatus::Succeeded => "succeeded",
            LifecycleStatus::Rejected => "rejected",
            LifecycleStatus::Failed => "failed",
            LifecycleStatus::Cancelled => "cancelled",
        }
    }
}

/// Classified error detail recorded on rejected/failed/cancelled outcomes.
#[derive(Clone, Debug)]
pub struct RequestError {
    pub stage: String,
    pub kind: String,
    pub code: Option<String>,
    pub message: Option<String>,
}

impl RequestError {
    pub fn new(stage: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            stage: stage.into(),
            kind: kind.into(),
            code: None,
            message: None,
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// Mutable draft that accumulates facts as the request progresses and is only
/// serialised once, at finalize time.
#[derive(Clone, Debug)]
struct LifecycleDraft {
    resolved_model: Option<String>,
    model_mapping_rule: Option<String>,
    channel_id: Option<String>,
    endpoint_id: Option<i64>,
    endpoint_url: Option<String>,
    upstream_model: Option<String>,
    provider: Option<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    ttft_ms: Option<u64>,
    client_disconnected: bool,
    attempt_count: u32,
    successful_attempt: Option<u32>,
    wallet_amount: Option<f64>,
}

impl Default for LifecycleDraft {
    fn default() -> Self {
        Self {
            resolved_model: None,
            model_mapping_rule: None,
            channel_id: None,
            endpoint_id: None,
            endpoint_url: None,
            upstream_model: None,
            provider: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            ttft_ms: None,
            client_disconnected: false,
            attempt_count: 0,
            successful_attempt: None,
            wallet_amount: None,
        }
    }
}

/// Tracks one concrete invocation of a provider adapter. Dropping an active
/// attempt finalizes it as a failed invocation, ensuring every adapter call has
/// exactly one terminal attempt event.
pub struct AttemptLifecycle {
    recorder: GatewayEventRecorder,
    started_at: Instant,
    finalized: AtomicBool,
    request_id: String,
    attempt_id: String,
    attempt_no: u32,
    route_id: String,
    channel_id: Option<String>,
    endpoint_id: Option<i64>,
    endpoint_url: Option<String>,
    provider: Option<String>,
}

impl AttemptLifecycle {
    pub fn new(
        recorder: &GatewayEventRecorder,
        request_id: impl Into<String>,
        attempt_no: u32,
        route_id: impl Into<String>,
        channel_id: Option<String>,
        endpoint_id: Option<i64>,
        endpoint_url: Option<String>,
        provider: Option<String>,
    ) -> Self {
        let request_id = request_id.into();
        Self {
            recorder: recorder.clone(),
            started_at: Instant::now(),
            finalized: AtomicBool::new(false),
            attempt_id: format!("{}-{}", request_id, attempt_no),
            request_id,
            attempt_no,
            route_id: route_id.into(),
            channel_id,
            endpoint_id,
            endpoint_url,
            provider,
        }
    }

    pub fn finalize(
        &self,
        success: bool,
        status_code: Option<u16>,
        timeout: bool,
        error: Option<String>,
    ) -> bool {
        if self
            .finalized
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return false;
        }
        self.recorder.record_attempt(GatewayAttemptEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            request_id: self.request_id.clone(),
            attempt_id: self.attempt_id.clone(),
            attempt_no: self.attempt_no,
            route_id: self.route_id.clone(),
            channel_id: self.channel_id.clone(),
            endpoint_id: self.endpoint_id,
            endpoint_url: self.endpoint_url.clone(),
            provider: self.provider.clone(),
            status_code,
            success,
            latency_ms: self.started_at.elapsed().as_millis() as u64,
            timeout,
            error,
        });
        true
    }
}

impl Drop for AttemptLifecycle {
    fn drop(&mut self) {
        if !self.finalized.load(Ordering::SeqCst) {
            self.finalize(
                false,
                None,
                false,
                Some("attempt_dropped_without_finalize".into()),
            );
        }
    }
}

/// Tracks a single authenticated LLM request and guarantees exactly one
/// `GatewayRequestEvent` is emitted. Clone is deliberately NOT implemented:
/// each request owns exactly one lifecycle.
pub struct RequestLifecycle {
    recorder: GatewayEventRecorder,
    started_at: Instant,
    finalized: AtomicBool,
    meta: RequestMeta,
    identity: RequestIdentity,
    draft: Mutex<LifecycleDraft>,
}

impl RequestLifecycle {
    pub fn new(
        recorder: &GatewayEventRecorder,
        meta: RequestMeta,
        identity: RequestIdentity,
    ) -> Self {
        Self {
            recorder: recorder.clone(),
            started_at: Instant::now(),
            finalized: AtomicBool::new(false),
            meta,
            identity,
            draft: Mutex::new(LifecycleDraft::default()),
        }
    }

    pub fn request_id(&self) -> &str {
        &self.meta.request_id
    }

    pub fn begin_attempt(
        &self,
        attempt_no: u32,
        route_id: impl Into<String>,
        channel_id: Option<String>,
        endpoint_id: Option<i64>,
        endpoint_url: Option<String>,
        provider: Option<String>,
    ) -> AttemptLifecycle {
        AttemptLifecycle::new(
            &self.recorder,
            self.meta.request_id.clone(),
            attempt_no,
            route_id,
            channel_id,
            endpoint_id,
            endpoint_url,
            provider,
        )
    }

    pub fn is_finalized(&self) -> bool {
        self.finalized.load(Ordering::SeqCst)
    }

    pub fn set_model_mapping_rule(&self, rule: Option<String>) {
        self.draft.lock().unwrap().model_mapping_rule = rule;
    }

    /// Record the resolved (post-routing) model, channel and endpoint.
    pub fn set_route(
        &self,
        resolved_model: String,
        channel_id: Option<String>,
        endpoint_id: Option<i64>,
        endpoint_url: Option<String>,
        upstream_model: Option<String>,
        provider: Option<String>,
    ) {
        let mut draft = self.draft.lock().unwrap();
        draft.resolved_model = Some(resolved_model);
        draft.channel_id = channel_id;
        draft.endpoint_id = endpoint_id;
        draft.endpoint_url = endpoint_url;
        draft.upstream_model = upstream_model;
        draft.provider = provider;
    }

    /// Accumulate token usage (safe to call more than once; finalize uses the
    /// running total).
    pub fn add_tokens(
        &self,
        prompt_tokens: u64,
        completion_tokens: u64,
        cache_read_tokens: u64,
        cache_write_tokens: u64,
    ) {
        let mut draft = self.draft.lock().unwrap();
        draft.prompt_tokens = draft.prompt_tokens.saturating_add(prompt_tokens);
        draft.completion_tokens = draft.completion_tokens.saturating_add(completion_tokens);
        draft.cache_read_tokens = draft.cache_read_tokens.saturating_add(cache_read_tokens);
        draft.cache_write_tokens = draft.cache_write_tokens.saturating_add(cache_write_tokens);
    }

    pub fn set_ttft(&self, ttft_ms: u64) {
        self.draft.lock().unwrap().ttft_ms = Some(ttft_ms);
    }

    /// Record that the client disconnected before the request completed.
    pub fn mark_client_disconnected(&self) {
        self.draft.lock().unwrap().client_disconnected = true;
    }

    /// Record attempt bookkeeping. Attempts are counted by the caller
    /// (attempt lifecycle lands in a later phase); the request event simply
    /// reflects the final totals.
    pub fn set_attempts(&self, attempt_count: u32, successful_attempt: Option<u32>) {
        let mut draft = self.draft.lock().unwrap();
        draft.attempt_count = attempt_count;
        draft.successful_attempt = successful_attempt;
    }

    pub fn set_wallet_amount(&self, wallet_amount: f64) {
        self.draft.lock().unwrap().wallet_amount = Some(wallet_amount);
    }

    /// Finalize a pre-classified handler error. The caller owns returning the
    /// original `GatewayError`; this method only emits telemetry.
    pub fn finalize_classified(&self, error: &crate::scheduler::helpers::ClassifiedError) -> bool {
        let detail = RequestError {
            stage: error.stage.to_string(),
            kind: error.kind.to_string(),
            code: None,
            message: error.message.clone().or_else(|| {
                let message = error.err.message();
                (!message.is_empty()).then(|| message.to_string())
            }),
        };
        match error.status() {
            LifecycleStatus::Cancelled => self.finalize_cancelled(error.status_code, detail),
            LifecycleStatus::Failed => self.finalize_failed(error.status_code, detail),
            LifecycleStatus::Rejected | LifecycleStatus::Succeeded => {
                self.finalize_rejected(error.status_code, detail)
            }
        }
    }

    /// Finalize as succeeded. Returns true only when this is the first
    /// finalize (exactly-once).
    pub fn finalize_success(&self) -> bool {
        self.finalize(LifecycleStatus::Succeeded, 200, None)
    }

    /// Finalize as a client-visible rejection (4xx): validation, rate limit,
    /// authorization, routing or guardrail failures that never reached an
    /// upstream call.
    pub fn finalize_rejected(&self, status_code: u16, error: RequestError) -> bool {
        self.finalize(LifecycleStatus::Rejected, status_code, Some(error))
    }

    /// Finalize as a gateway/upstream failure (5xx).
    pub fn finalize_failed(&self, status_code: u16, error: RequestError) -> bool {
        self.finalize(LifecycleStatus::Failed, status_code, Some(error))
    }

    /// Finalize as cancelled (client disconnect / stream abort).
    pub fn finalize_cancelled(&self, status_code: u16, error: RequestError) -> bool {
        self.finalize(LifecycleStatus::Cancelled, status_code, Some(error))
    }

    /// Single finalize path guarded by the atomic flag. Returns false (and
    /// emits nothing) if the lifecycle was already finalized.
    fn finalize(
        &self,
        status: LifecycleStatus,
        status_code: u16,
        error: Option<RequestError>,
    ) -> bool {
        if self
            .finalized
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return false;
        }
        let draft = self.draft.lock().unwrap();
        let total_tokens = draft
            .prompt_tokens
            .saturating_add(draft.completion_tokens)
            .saturating_add(draft.cache_read_tokens)
            .saturating_add(draft.cache_write_tokens);
        let event = GatewayRequestEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            request_id: self.meta.request_id.clone(),
            user_id: Some(self.identity.user_id.clone()),
            user_name: Some(self.identity.user_name.clone()),
            team_id: self.identity.team_id.clone(),
            api_key_id: self.identity.api_key_id.clone(),
            api_key_name: Some(self.identity.api_key_name.clone()),
            route_id: draft
                .channel_id
                .clone()
                .unwrap_or_else(|| self.identity.requested_model.clone()),
            method: self.meta.method.clone(),
            path: self.meta.path.clone(),
            api_format: self.meta.api_format.clone(),
            stream: self.meta.stream,
            client_ip: self.meta.client_ip.clone(),
            user_agent: self.meta.user_agent.clone(),
            requested_model: self.identity.requested_model.clone(),
            resolved_model: draft.resolved_model.clone(),
            model_mapping_rule: draft.model_mapping_rule.clone(),
            channel_id: draft.channel_id.clone(),
            endpoint_id: draft.endpoint_id,
            endpoint_url: draft.endpoint_url.clone(),
            upstream_model: draft.upstream_model.clone(),
            provider: draft.provider.clone(),
            status: status.as_str().to_string(),
            status_code,
            error_stage: error.as_ref().map(|e| e.stage.clone()),
            error_kind: error.as_ref().map(|e| e.kind.clone()),
            error_code: error.as_ref().and_then(|e| e.code.clone()),
            error_message: error.as_ref().and_then(|e| e.message.clone()),
            attempt_count: draft.attempt_count,
            successful_attempt: draft.successful_attempt,
            prompt_tokens: draft.prompt_tokens,
            completion_tokens: draft.completion_tokens,
            cache_read_tokens: draft.cache_read_tokens,
            cache_write_tokens: draft.cache_write_tokens,
            total_tokens,
            total_latency_ms: self.started_at.elapsed().as_millis() as u64,
            ttft_ms: draft.ttft_ms,
            client_disconnected: draft.client_disconnected,
            termination_reason: error.as_ref().map(|e| e.kind.clone()).or_else(|| {
                (status == LifecycleStatus::Succeeded).then(|| "completed".to_string())
            }),
            billing_payment_mode: self.identity.billing_payment_mode.clone(),
            wallet_amount: draft.wallet_amount,
            bytes_in: 0,
        };
        self.recorder.record_request(event);
        true
    }

    /// Non-blocking Drop fallback: if the lifecycle was never finalized,
    /// record `failed / 500 / gateway / unfinalized_request`. Uses only the
    /// recorder's `try_send` path — never blocks, never allocates async work.
    fn finalize_on_drop(&mut self) {
        if self.finalized.load(Ordering::SeqCst) {
            return;
        }
        self.finalize(
            LifecycleStatus::Failed,
            500,
            Some(
                RequestError::new("gateway", "unfinalized_request")
                    .with_message("Request lifecycle dropped without an explicit finalize"),
            ),
        );
    }
}

impl Drop for RequestLifecycle {
    fn drop(&mut self) {
        self.finalize_on_drop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observability::gateway_events::{GatewayEvent, GatewayEventRecorder};
    use tokio::sync::mpsc;

    fn test_recorder(cap: usize) -> (GatewayEventRecorder, mpsc::Receiver<GatewayEvent>) {
        GatewayEventRecorder::test_recorder(cap)
    }

    fn meta() -> RequestMeta {
        RequestMeta {
            request_id: "req-1".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            api_format: "openai".to_string(),
            stream: false,
            client_ip: Some("1.2.3.4".to_string()),
            user_agent: Some("test-agent".to_string()),
        }
    }

    fn identity() -> RequestIdentity {
        RequestIdentity {
            user_id: "user-1".to_string(),
            user_name: "Alice".to_string(),
            team_id: None,
            api_key_id: Some("key-1".to_string()),
            api_key_name: "my-key".to_string(),
            requested_model: "gpt-4o".to_string(),
            billing_payment_mode: Some("metered".to_string()),
        }
    }

    fn recv(rx: &mut mpsc::Receiver<GatewayEvent>) -> GatewayRequestEvent {
        match rx.try_recv() {
            Ok(GatewayEvent::Request(event)) => event,
            Ok(other) => panic!("expected request event, got {other:?}"),
            Err(e) => panic!("expected queued event: {e}"),
        }
    }

    #[test]
    fn success_finalize_produces_exactly_one_request_event() {
        let (recorder, mut rx) = test_recorder(4);
        let lc = RequestLifecycle::new(&recorder, meta(), identity());
        lc.set_route(
            "gpt-4o".to_string(),
            Some("ch-1".to_string()),
            Some(7),
            Some("https://up.example".to_string()),
            None,
            Some("openai".to_string()),
        );
        lc.add_tokens(10, 5, 2, 1);
        assert!(lc.finalize_success());
        // Second finalize must be a no-op (exactly-once).
        assert!(!lc.finalize_success());
        assert!(!lc.finalize_failed(502, RequestError::new("upstream", "upstream_error")));
        assert!(lc.is_finalized());

        let event = recv(&mut rx);
        assert_eq!(event.request_id, "req-1");
        assert_eq!(event.status, "succeeded");
        assert_eq!(event.status_code, 200);
        assert_eq!(event.resolved_model.as_deref(), Some("gpt-4o"));
        assert_eq!(event.channel_id.as_deref(), Some("ch-1"));
        assert_eq!(event.endpoint_id, Some(7));
        assert_eq!(event.prompt_tokens, 10);
        assert_eq!(event.completion_tokens, 5);
        assert_eq!(event.cache_read_tokens, 2);
        assert_eq!(event.cache_write_tokens, 1);
        assert_eq!(event.total_tokens, 18);
        assert_eq!(event.user_name.as_deref(), Some("Alice"));
        assert_eq!(event.requested_model, "gpt-4o");
        assert!(event.error_stage.is_none());
        assert!(rx.try_recv().is_err(), "only one event may be emitted");
    }

    #[test]
    fn rejected_finalize_records_classified_error() {
        let (recorder, mut rx) = test_recorder(4);
        let lc = RequestLifecycle::new(&recorder, meta(), identity());
        assert!(lc.finalize_rejected(503, RequestError::new("routing", "no_available_endpoint"),));
        let event = recv(&mut rx);
        assert_eq!(event.status, "rejected");
        assert_eq!(event.status_code, 503);
        assert_eq!(event.error_stage.as_deref(), Some("routing"));
        assert_eq!(event.error_kind.as_deref(), Some("no_available_endpoint"));
        assert_eq!(event.attempt_count, 0);
    }

    #[test]
    fn drop_without_finalize_records_unfinalized_request() {
        let (recorder, mut rx) = test_recorder(4);
        {
            let lc = RequestLifecycle::new(&recorder, meta(), identity());
            // Never finalized — Drop must emit the fallback event.
            let _ = &lc;
        }
        let event = recv(&mut rx);
        assert_eq!(event.status, "failed");
        assert_eq!(event.status_code, 500);
        assert_eq!(event.error_stage.as_deref(), Some("gateway"));
        assert_eq!(event.error_kind.as_deref(), Some("unfinalized_request"));
        assert_eq!(event.requested_model, "gpt-4o");
    }

    #[test]
    fn drop_after_explicit_finalize_emits_nothing() {
        let (recorder, mut rx) = test_recorder(4);
        {
            let lc = RequestLifecycle::new(&recorder, meta(), identity());
            assert!(lc.finalize_cancelled(
                499,
                RequestError::new("response_stream", "client_disconnect"),
            ));
        }
        let event = recv(&mut rx);
        assert_eq!(event.status, "cancelled");
        assert_eq!(event.status_code, 499);
        assert_eq!(event.error_kind.as_deref(), Some("client_disconnect"));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn cancelled_is_distinct_from_failed() {
        let (recorder, mut rx) = test_recorder(4);
        let lc = RequestLifecycle::new(&recorder, meta(), identity());
        assert!(lc.finalize_cancelled(
            499,
            RequestError::new("response_stream", "client_disconnect"),
        ));
        let event = recv(&mut rx);
        assert_eq!(event.status, "cancelled");
        assert_eq!(event.status_code, 499);
    }

    #[test]
    fn lifecycle_is_not_send_clone() {
        // Compile-time guard: a lifecycle must not be clonable so each
        // request has exactly one finalizer.
        fn assert_not_clone<T: ?Sized>() {}
        assert_not_clone::<RequestLifecycle>();
    }

    #[test]
    fn concurrent_finalize_still_emits_one_request_event() {
        use std::sync::Arc;
        use std::thread;

        let (recorder, mut rx) = test_recorder(8);
        let lifecycle = Arc::new(RequestLifecycle::new(&recorder, meta(), identity()));
        let mut workers = Vec::new();
        for _ in 0..8 {
            let lifecycle = Arc::clone(&lifecycle);
            workers.push(thread::spawn(move || lifecycle.finalize_success()));
        }
        let wins = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|won| *won)
            .count();
        assert_eq!(wins, 1, "exactly one finalizer may win");
        let event = recv(&mut rx);
        assert_eq!(event.status, "succeeded");
        assert!(
            rx.try_recv().is_err(),
            "concurrent finalizers emit one event"
        );
    }

    #[test]
    fn retry_and_fallback_have_one_attempt_event_per_invocation() {
        let (recorder, mut rx) = test_recorder(8);
        let lifecycle = RequestLifecycle::new(&recorder, meta(), identity());

        let first = lifecycle.begin_attempt(
            1,
            "route-a",
            Some("ch-a".into()),
            Some(1),
            None,
            Some("mock".into()),
        );
        assert!(first.finalize(false, Some(502), false, Some("upstream failed".into())));
        assert!(!first.finalize(false, Some(502), false, Some("duplicate".into())));
        drop(first);

        let second = lifecycle.begin_attempt(
            2,
            "route-b",
            Some("ch-b".into()),
            Some(2),
            None,
            Some("mock".into()),
        );
        assert!(second.finalize(true, Some(200), false, None));
        lifecycle.set_attempts(2, Some(2));
        assert!(lifecycle.finalize_success());

        match rx.try_recv().unwrap() {
            GatewayEvent::Attempt(event) => {
                assert_eq!(event.attempt_no, 1);
                assert!(!event.success);
            }
            other => panic!("expected first attempt, got {other:?}"),
        }
        match rx.try_recv().unwrap() {
            GatewayEvent::Attempt(event) => {
                assert_eq!(event.attempt_no, 2);
                assert!(event.success);
            }
            other => panic!("expected second attempt, got {other:?}"),
        }
        let request = recv(&mut rx);
        assert_eq!(request.attempt_count, 2);
        assert_eq!(request.successful_attempt, Some(2));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn dropping_active_attempt_records_one_failed_attempt() {
        let (recorder, mut rx) = test_recorder(4);
        let lifecycle = RequestLifecycle::new(&recorder, meta(), identity());
        let attempt = lifecycle.begin_attempt(1, "route-a", None, None, None, None);
        drop(attempt);
        match rx.try_recv().unwrap() {
            GatewayEvent::Attempt(event) => {
                assert_eq!(event.attempt_no, 1);
                assert!(!event.success);
                assert_eq!(
                    event.error.as_deref(),
                    Some("attempt_dropped_without_finalize")
                );
            }
            other => panic!("expected attempt event, got {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }
}
