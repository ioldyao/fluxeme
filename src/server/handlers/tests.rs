#[cfg(test)]
mod tests {
    use super::{
        count_tokens_supported_for_channel, estimate_tokens_responses, parse_responses_sse_usage,
        responses_input_tokens_supported_for_channel,
    };
    use crate::domain::channel::Channel;
    use serde_json::json;

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

