use std::sync::{Arc, RwLock};

use regex::Regex;

use crate::db::Database;
use crate::domain::moderation::ContentFilterRule;

/// Pre-compiled rule: the original config row plus an already-parsed Regex
/// (when `pattern_type == "regex"`).
struct CompiledRule {
    rule: ContentFilterRule,
    regex: Option<Regex>,
}

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
/// Pre-compiles regex patterns so the hot path never calls `Regex::new()`.
/// Caches the `content_moderation_enabled` setting so handlers don't
/// query the DB on every request.
pub struct ContentFilterService {
    db: Arc<Database>,
    rules: RwLock<Vec<CompiledRule>>,
    /// Cached copy of the `content_moderation_enabled` DB setting.
    /// Refreshed on `reload()`. Defaults to `false`.
    enabled: RwLock<bool>,
}

impl ContentFilterService {
    pub async fn new(db: Arc<Database>) -> Self {
        let svc = Self {
            db,
            rules: RwLock::new(Vec::new()),
            enabled: RwLock::new(false),
        };
        svc.reload().await;
        svc
    }

    /// Whether content filtering is enabled (cached from DB).
    /// Handlers should call this instead of querying the database.
    pub fn is_enabled(&self) -> bool {
        *self.enabled.read().unwrap()
    }

    /// Reload all enabled rules and the enabled flag from the database.
    pub async fn reload(&self) {
        // Refresh the enabled flag
        let filter_enabled = self
            .db
            .get_setting("content_moderation_enabled")
            .await
            .ok()
            .flatten()
            .map(|v| v != "false")
            .unwrap_or(false);
        *self.enabled.write().unwrap() = filter_enabled;

        // Reload rules with pre-compiled regexes
        match self.db.list_filter_rules().await {
            Ok(rules) => {
                let mut compiled: Vec<CompiledRule> = rules
                    .into_iter()
                    .filter(|r| r.enabled)
                    .map(|r| {
                        let regex = if r.pattern_type == "regex" {
                            match Regex::new(&r.pattern) {
                                Ok(re) => Some(re),
                                Err(e) => {
                                    tracing::warn!(
                                        "Invalid regex pattern for rule '{}': {}",
                                        r.name,
                                        e
                                    );
                                    None
                                }
                            }
                        } else {
                            None
                        };
                        CompiledRule { rule: r, regex }
                    })
                    .collect();
                compiled.sort_by_key(|c| c.rule.priority);
                *self.rules.write().unwrap() = compiled;
                tracing::info!(
                    "Content filter loaded {} enabled rules (enabled={})",
                    self.rules.read().unwrap().len(),
                    filter_enabled,
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

        for compiled in rules.iter() {
            let rule = &compiled.rule;

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

            if !match_compiled(&masked_body, compiled) {
                continue;
            }

            match rule.action.as_str() {
                "block" => {
                    return FilterOutcome::Blocked(rule.name.clone());
                }
                "mask" => {
                    masked_body = apply_mask_compiled(&masked_body, compiled);
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

        for compiled in rules.iter() {
            let rule = &compiled.rule;

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

            if match_compiled(&result, compiled) {
                result = apply_mask_compiled(&result, compiled);
            }
        }

        result
    }
}

// ── Matching logic ──────────────────────────────────────────────────────

/// Check whether `text` matches the given compiled rule.
fn match_compiled(text: &str, compiled: &CompiledRule) -> bool {
    match compiled.rule.pattern_type.as_str() {
        "regex" => {
            if let Some(ref re) = compiled.regex {
                re.is_match(text)
            } else {
                false // regex failed to compile at reload time
            }
        }
        _ => {
            // keyword mode: split by comma, check each keyword
            let keywords: Vec<&str> = compiled.rule.pattern.split(',').map(|s| s.trim()).collect();
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
fn apply_mask_compiled(text: &str, compiled: &CompiledRule) -> String {
    let replacement = compiled
        .rule
        .replacement
        .as_deref()
        .unwrap_or("[REDACTED]");

    match compiled.rule.pattern_type.as_str() {
        "regex" => {
            if let Some(ref re) = compiled.regex {
                re.replace_all(text, replacement).to_string()
            } else {
                text.to_string()
            }
        }
        _ => {
            let mut result = text.to_string();
            let keywords: Vec<&str> = compiled
                .rule
                .pattern
                .split(',')
                .map(|s| s.trim())
                .collect();
            for kw in keywords {
                if !kw.is_empty() {
                    result = result.replace(kw, replacement);
                }
            }
            result
        }
    }
}
