#[cfg(test)]
mod tests {
    use super::{estimate_tokens_responses, record_auth_failure};
    use crate::observability::gateway_events::{GatewayEvent, GatewayEventRecorder};
    use crate::domain::channel::Channel;
    use crate::service::auth::AuthError;
    use crate::scheduler::dispatch::{
        count_tokens_supported_for_channel, parse_responses_sse_usage,
        responses_input_tokens_supported_for_channel,
    };
    use crate::scheduler::helpers::authorize_effective_model;
    use serde_json::json;

    fn auth(
        allowed_models: Option<Vec<String>>,
        scopes: Option<Vec<String>>,
    ) -> crate::domain::user::AuthResult {
        crate::domain::user::AuthResult {
            user_id: "user".to_string(),
            user_name: "user".to_string(),
            rate_limits: None,
            allowed_models,
            scopes,
            key_kind: "user".to_string(),
            api_key_name: "test".to_string(),
            concurrency_limit: 1,
            team_id: None,
            team_role: None,
            billing_group_id: "default".to_string(),
            billing_payment_mode: crate::domain::billing_group::BillingPaymentMode::Metered,
        }
    }

    #[test]
    fn auth_failure_records_security_event_without_raw_credential() {
        let (recorder, mut rx) = GatewayEventRecorder::test_recorder(2);
        record_auth_failure(
            &recorder,
            "127.0.0.1".parse().unwrap(),
            "request-123",
            "/v1/chat/completions",
            &AuthError("Unknown or disabled API key".into()),
            Some(crate::service::auth::credential_fingerprint(
                "sk-super-secret-value",
            )),
            4,
        );
        match rx.try_recv().expect("access event") {
            GatewayEvent::Access(event) => {
                assert_eq!(event.request_id, "request-123");
                assert_eq!(event.path, "/v1/chat/completions");
                assert_eq!(event.method, "POST");
                assert_eq!(event.client_ip.as_deref(), Some("127.0.0.1"));
                assert_eq!(event.auth_result, "failure");
                assert_eq!(event.error_kind.as_deref(), Some("invalid_key"));
                assert_eq!(event.status_code, 401);
                assert!(!event.success);
                assert_eq!(event.latency_ms, 4);
                assert!(!serde_json::to_string(&event)
                    .unwrap()
                    .contains("sk-super-secret-value"));
            }
            other => panic!("expected access event, got {other:?}"),
        }
    }

    #[test]
    fn auth_failure_event_contains_no_usage_request_or_billing_event() {
        let (recorder, mut rx) = GatewayEventRecorder::test_recorder(2);
        record_auth_failure(
            &recorder,
            "10.0.0.4".parse().unwrap(),
            "request-unauthorized",
            "/v1/messages",
            &AuthError("Missing or invalid API key".into()),
            None,
            1,
        );
        assert!(matches!(rx.try_recv(), Ok(GatewayEvent::Access(_))));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn effective_model_must_be_in_allowlist_after_rewrite() {
        let user = auth(Some(vec!["model-a".to_string()]), None);
        assert!(authorize_effective_model(&user, "model-a").is_ok());
        assert!(authorize_effective_model(&user, "model-b").is_err());
    }

    #[test]
    fn model_scope_is_required_when_scopes_are_present() {
        let user = auth(None, Some(vec!["skill".to_string()]));
        assert!(authorize_effective_model(&user, "model-a").is_err());
    }

    #[test]
    fn legacy_keys_keep_existing_model_access() {
        let user = auth(None, None);
        assert!(authorize_effective_model(&user, "model-a").is_ok());
    }

    fn channel(provider: &str, anthropic_compat: bool) -> Channel {
        Channel {
            id: "ch_1".to_string(),
            name: "test".to_string(),
            provider: provider.to_string(),
            enabled: true,
            anthropic_compat,
            endpoints: Vec::new(),
        }
    }

    #[test]
    fn supports_count_tokens_for_normal_channel() {
        assert!(count_tokens_supported_for_channel(Some(&channel(
            "anthropic",
            false
        ))));
    }

    #[test]
    fn rejects_count_tokens_for_anthropic_compat_channel() {
        assert!(!count_tokens_supported_for_channel(Some(&channel(
            "openai", true
        ))));
    }

    #[test]
    fn supports_count_tokens_when_channel_is_missing() {
        assert!(count_tokens_supported_for_channel(None));
    }

    #[test]
    fn supports_responses_input_tokens_for_openai_channel() {
        assert!(responses_input_tokens_supported_for_channel(Some(
            &channel("openai", false)
        )));
    }

    #[test]
    fn supports_responses_input_tokens_for_anthropic_compat_openai_channel() {
        assert!(responses_input_tokens_supported_for_channel(Some(
            &channel("openai", true)
        )));
    }

    #[test]
    fn rejects_responses_input_tokens_for_non_openai_channel() {
        assert!(!responses_input_tokens_supported_for_channel(Some(
            &channel("anthropic", false)
        )));
    }

    #[test]
    fn parses_responses_sse_cache_write_usage() {
        let data = r#"data: {"type":"response.completed","response":{"usage":{"input_tokens":100,"output_tokens":12,"input_tokens_details":{"cached_tokens":80,"cache_write_tokens":5}}}}"#;
        assert_eq!(parse_responses_sse_usage(data), (100, 12, 80, 5));
    }

    #[test]
    fn estimates_tokens_for_string_responses_input() {
        let body = json!({
            "model": "gpt-5",
            "input": "Tell me a joke."
        });

        assert_eq!(estimate_tokens_responses(&body), 10);
    }

    #[test]
    fn estimates_tokens_for_nested_responses_input() {
        let body = json!({
            "model": "gpt-5",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "hello"},
                        {"type": "input_text", "text": "world"}
                    ]
                }
            ]
        });

        assert_eq!(estimate_tokens_responses(&body), 32);
    }
}
