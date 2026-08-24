// ── Token estimators ──────────────────────────────────────────────

fn estimate_tokens(body: &Value) -> u64 {
    let total_chars: usize = body["messages"]
        .as_array()
        .map(|msgs| {
            msgs.iter()
                .filter_map(|m| m["content"].as_str())
                .map(|s| s.len())
                .sum()
        })
        .unwrap_or(0);
    (total_chars / 4) as u64
}

fn estimate_tokens_anthropic(body: &Value) -> u64 {
    let total_chars: usize = body["messages"]
        .as_array()
        .map(|msgs| {
            msgs.iter()
                .map(|m| match &m["content"] {
                    Value::String(s) => s.len(),
                    Value::Array(arr) => arr
                        .iter()
                        .filter_map(|c| c["text"].as_str())
                        .map(|s| s.len())
                        .sum(),
                    _ => 0,
                })
                .sum()
        })
        .unwrap_or(0);
    (total_chars / 4) as u64
}

fn estimate_tokens_responses(body: &Value) -> u64 {
    let total_chars = serde_json::to_string(body).map(|s| s.len()).unwrap_or(0);
    (total_chars / 4) as u64
}


