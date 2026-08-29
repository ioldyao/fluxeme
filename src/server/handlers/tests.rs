#[cfg(test)]
mod tests {
    use super::{
        authorize_effective_model, count_tokens_supported_for_channel, estimate_tokens_responses,
        parse_responses_sse_usage, responses_input_tokens_supported_for_channel,
    };
    use crate::domain::channel::Channel;
    use serde_json::json;

    fn auth(allowed_models: Option<Vec<String>>, scopes: Option<Vec<String>>) -> crate::domain::user::AuthResult {
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
            priority: 1,
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

