use std::sync::{Arc, RwLock};

use regex::Regex;

use crate::db::Database;
use crate::domain::moderation::ContentFilterRule;

/// Outcome of a content filter check on a request body.
#[derive(Debug)]
pub enum FilterOutcome {
    /// The request passes all rules — forward as-is.
    Pass,
    /// The request was blocked by a rule.
    Blocked(String),
    /// The request body was masked (sensitive content replaced).
    Masked(String),
}

/// Error returned when a request is blocked by a content filter rule.
#[derive(Debug)]
pub struct FilterBlocked(pub String);

impl std::fmt::Display for FilterBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Content filter blocked: {}", self.0)
    }
}

impl std::error::Error for FilterBlocked {}

/// In-memory content filter service.
///
/// Loads rules from the database on startup and after admin changes.
/// Provides methods to check and mask request/response bodies.
pub struct ContentFilterService {
    db: Arc<Database>,
    rules: RwLock<Vec<ContentFilterRule>>,
}

impl ContentFilterService {
    pub async fn new(db: Arc<Database>) -> Self {
        let svc = Self {
            db,
            rules: RwLock::new(Vec::new()),
        };
        svc.reload().await;
        svc
    }

    /// Reload all enabled rules from the database.
    pub async fn reload(&self) {
        match self.db.list_filter_rules().await {
            Ok(rules) => {
                let mut enabled: Vec<ContentFilterRule> =
                    rules.into_iter().filter(|r| r.enabled).collect();
                enabled.sort_by_key(|r| r.priority);
                *self.rules.write().unwrap() = enabled;
                tracing::info!(
                    "Content filter loaded {} enabled rules",
                    self.rules.read().unwrap().len()
                );
            }
            Err(e) => tracing::error!("Failed to load content filter rules: {}", e),
        }
    }

    /// Check a request body against the filter rules.
    ///
    /// Returns:
    /// - `FilterOutcome::Pass` if no rules match.
    /// - `FilterOutcome::Blocked` if a block-level rule matches.
    /// - `FilterOutcome::Masked` if only mask-level rules match (with masked content).
    pub fn check_request(&self, body_str: &str, channel_id: Option<&str>) -> FilterOutcome {
        let rules = self.rules.read().unwrap();
        let mut masked_body = body_str.to_string();
        let mut was_masked = false;

        for rule in rules.iter() {
            // Filter by scope
            if rule.scope != "request" && rule.scope != "both" {
                continue;
            }

            // Filter by channel: if channel_id is set on the rule, it must match
            if let Some(ref rule_ch) = rule.channel_id {
                match channel_id {
                    Some(req_ch) if req_ch == rule_ch => {} // match
                    _ => continue,                          // not matching this channel
                }
            }

            if !match_pattern(&masked_body, rule) {
                continue;
            }

            match rule.action.as_str() {
                "block" => {
                    return FilterOutcome::Blocked(rule.name.clone());
                }
                "mask" => {
                    masked_body = apply_mask(&masked_body, rule);
                    was_masked = true;
                }
                _ => {}
            }
        }

        if was_masked {
            FilterOutcome::Masked(masked_body)
        } else {
            FilterOutcome::Pass
        }
    }

    /// Apply mask rules to a response body.
    ///
    /// Returns the masked body (or the original if no rules match).
    pub fn apply_response(&self, body_str: &str, channel_id: Option<&str>) -> String {
        let rules = self.rules.read().unwrap();
        let mut result = body_str.to_string();

        for rule in rules.iter() {
            if rule.scope != "response" && rule.scope != "both" {
                continue;
            }

            if let Some(ref rule_ch) = rule.channel_id {
                match channel_id {
                    Some(req_ch) if req_ch == rule_ch => {}
                    _ => continue,
                }
            }

            if rule.action != "mask" {
                continue;
            }

            if match_pattern(&result, rule) {
                result = apply_mask(&result, rule);
            }
        }

        result
    }
}

// ── Matching logic ──────────────────────────────────────────────────────

/// Check whether `text` matches the given rule's pattern.
fn match_pattern(text: &str, rule: &ContentFilterRule) -> bool {
    match rule.pattern_type.as_str() {
        "regex" => match Regex::new(&rule.pattern) {
            Ok(re) => re.is_match(text),
            Err(e) => {
                tracing::warn!(
                    "Invalid regex pattern for rule '{}': {}",
                    rule.name,
                    e
                );
                false
            }
        },
        _ => {
            // keyword mode: split by comma, check each keyword
            let keywords: Vec<&str> = rule.pattern.split(',').map(|s| s.trim()).collect();
            keywords.iter().any(|kw| {
                if kw.is_empty() {
                    return false;
                }
                text.contains(kw)
            })
        }
    }
}

/// Replace all matches of the rule's pattern in `text` with the replacement.
fn apply_mask(text: &str, rule: &ContentFilterRule) -> String {
    let replacement = rule
        .replacement
        .as_deref()
        .unwrap_or("[REDACTED]");

    match rule.pattern_type.as_str() {
        "regex" => match Regex::new(&rule.pattern) {
            Ok(re) => re.replace_all(text, replacement).to_string(),
            Err(_) => text.to_string(),
        },
        _ => {
            let mut result = text.to_string();
            let keywords: Vec<&str> = rule.pattern.split(',').map(|s| s.trim()).collect();
            for kw in keywords {
                if !kw.is_empty() {
                    result = result.replace(kw, replacement);
                }
            }
            result
        }
    }
}
