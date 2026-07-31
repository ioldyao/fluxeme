use async_trait::async_trait;
use rust_decimal::Decimal;

use crate::domain::channel::{Channel, Endpoint};
use crate::domain::model::{Model, Pricing};
use crate::domain::moderation::ContentFilterRule;
use crate::domain::routing::RoutingRule;
use crate::domain::usage::UsageFilter;
use crate::domain::usage::UsageRecord;
use crate::domain::user::{ApiKey, User};
use chrono::{DateTime, Utc};

use super::{
    AnnouncementRow, DbError, FunnelStats, ProbeResultRow, RechargeKeyRow, WalletTransactionRow,
};

/// PostgreSQL persistence contract used by application services.
///
/// The contract remains separate from `PgBackend` so services can use test
/// doubles without adding another production database implementation.
#[allow(dead_code, clippy::too_many_arguments)]
#[async_trait]
pub trait DbBackend: Send + Sync {
    // ── Migration ────────────────────────────────────────────────────────
    async fn migrate(&self) -> Result<(), DbError>;
    /// Connectivity check (SELECT 1). Used by readiness probes.
    async fn ping(&self) -> Result<(), DbError>;

    // ── Users ────────────────────────────────────────────────────────────
    async fn list_users(&self, status: Option<&str>) -> Result<Vec<User>, DbError>;
    async fn get_user(&self, id: &str) -> Result<Option<User>, DbError>;
    async fn get_user_with_password(&self, id: &str) -> Result<Option<User>, DbError>;
    async fn create_user(&self, user: &User) -> Result<(), DbError>;
    async fn create_initial_admin(&self, user: &User) -> Result<(), DbError>;
    async fn update_user(&self, user: &User) -> Result<(), DbError>;
    async fn bump_user_token_version(&self, id: &str) -> Result<(), DbError>;
    async fn update_user_admin_fields(
        &self,
        id: &str,
        name: Option<String>,
        password_hash: Option<String>,
        rate_limits: Option<crate::domain::user::RateLimit>,
        role: Option<String>,
        concurrency_limit: Option<u32>,
    ) -> Result<User, DbError>;
    async fn suspend_user(&self, id: &str, suspended_at: &DateTime<Utc>) -> Result<User, DbError>;
    async fn restore_user(&self, id: &str) -> Result<User, DbError>;
    async fn delete_user(&self, id: &str) -> Result<(), DbError>;
    async fn count_admins(&self, status: Option<&str>) -> Result<i64, DbError>;
    async fn get_user_timezone(&self, id: &str) -> Result<String, DbError>;
    async fn update_user_timezone(&self, id: &str, timezone: &str) -> Result<(), DbError>;
    async fn get_user_currency(&self, id: &str) -> Result<String, DbError>;
    async fn update_user_currency(&self, id: &str, currency: &str) -> Result<(), DbError>;

    // ── API Keys ─────────────────────────────────────────────────────────
    async fn list_api_keys(&self, user_id: &str) -> Result<Vec<ApiKey>, DbError>;
    async fn create_api_key(&self, key: &ApiKey) -> Result<(), DbError>;
    async fn delete_api_key(&self, key: &str) -> Result<(), DbError>;
    async fn update_api_key(&self, key: &ApiKey) -> Result<(), DbError>;
    async fn lookup_key(&self, key: &str) -> Result<Option<(User, ApiKey)>, DbError>;
    async fn all_api_keys(&self) -> Result<Vec<(User, ApiKey)>, DbError>;

    // ── Channels & Endpoints ─────────────────────────────────────────────
    async fn list_channels(&self) -> Result<Vec<Channel>, DbError>;
    async fn get_channel(&self, id: &str) -> Result<Option<Channel>, DbError>;
    async fn create_channel(&self, ch: &Channel) -> Result<(), DbError>;
    async fn update_channel(&self, ch: &Channel) -> Result<(), DbError>;
    async fn delete_channel(&self, id: &str) -> Result<(), DbError>;
    async fn get_endpoint(&self, id: i64) -> Result<Option<Endpoint>, DbError>;
    async fn update_endpoint_api_key(&self, id: i64, api_key: &str) -> Result<(), DbError>;
    async fn update_endpoint_enabled(&self, id: i64, enabled: bool) -> Result<(), DbError>;

    // ── Models ───────────────────────────────────────────────────────────
    async fn list_models(&self) -> Result<Vec<Model>, DbError>;
    async fn get_model(&self, id: &str) -> Result<Option<Model>, DbError>;
    async fn create_model(&self, m: &Model) -> Result<(), DbError>;
    async fn update_model(&self, old_id: &str, m: &Model) -> Result<(), DbError>;
    async fn delete_model(&self, id: &str) -> Result<(), DbError>;
    async fn list_published_models(&self) -> Result<Vec<Model>, DbError>;
    async fn set_model_published(&self, id: &str, published: bool) -> Result<(), DbError>;
    async fn set_model_pricing(&self, id: &str, pricing: &Pricing) -> Result<(), DbError>;
    async fn set_model_context_length(&self, id: &str, context_length: i64) -> Result<(), DbError>;

    // ── Routing Rules ────────────────────────────────────────────────────
    async fn list_rules(&self) -> Result<Vec<RoutingRule>, DbError>;
    async fn create_rule(&self, r: &RoutingRule) -> Result<(), DbError>;
    async fn update_rule(&self, r: &RoutingRule) -> Result<(), DbError>;
    async fn delete_rule(&self, id: &str) -> Result<(), DbError>;
    /// List user-level routing rules for a specific user.
    async fn list_user_rules(&self, user_id: &str) -> Result<Vec<RoutingRule>, DbError>;

    // ── Usage Logs ───────────────────────────────────────────────────────
    async fn insert_usage(&self, record: &UsageRecord) -> Result<(), DbError>;
    async fn count_usage(&self) -> Result<usize, DbError>;
    async fn count_usage_by_user(&self, user_id: &str) -> Result<usize, DbError>;
    async fn count_usage_filtered(&self, filter: &UsageFilter) -> Result<usize, DbError>;
    async fn query_usage(
        &self,
        limit: usize,
        offset: usize,
        filter: &UsageFilter,
    ) -> Result<Vec<UsageRecord>, DbError>;
    async fn get_usage_detail(&self, request_id: &str) -> Result<Option<UsageRecord>, DbError>;
    async fn purge_usage_logs(&self, cutoff: &str) -> Result<usize, DbError>;
    async fn usage_stats_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<(u64, u64, u64, u64), DbError>;
    async fn usage_cost_rows_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, DbError>;
    async fn query_usage_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, DbError>;
    async fn daily_usage_counts(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, i64)>, DbError>;
    async fn daily_usage_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64, u64)>, DbError>;
    async fn model_activity(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64)>, DbError>;
    async fn funnel_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<FunnelStats, DbError>;

    // ── Billing / Period ─────────────────────────────────────────────────
    async fn period_summary(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64), DbError>;
    async fn period_model_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal)>, DbError>;
    async fn period_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, Decimal)>, DbError>;
    async fn daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError>;
    async fn count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<usize, DbError>;
    async fn daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError>;
    async fn billing_months(&self) -> Result<Vec<String>, DbError>;
    async fn billing_months_for_user(&self, user_id: &str) -> Result<Vec<String>, DbError>;
    async fn period_summary_all(&self) -> Result<Vec<(String, Decimal, u64, u64)>, DbError>;
    async fn period_summary_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError>;
    async fn lookup_model_pricing(
        &self,
        model_name: &str,
    ) -> Result<(Decimal, Decimal, Decimal), DbError>;

    // ── Wallet ───────────────────────────────────────────────────────────
    async fn get_wallet_balance(&self, user_id: &str) -> Result<(Decimal, Decimal), DbError>;
    async fn update_wallet_balance(&self, user_id: &str, balance: Decimal) -> Result<(), DbError>;
    async fn add_wallet_transaction(
        &self,
        id: &str,
        user_id: &str,
        tx_type: &str,
        amount: Decimal,
        balance_before: Decimal,
        balance_after: Decimal,
        method: &str,
        status: &str,
        note: &str,
    ) -> Result<(), DbError>;
    async fn get_wallet_transactions(
        &self,
        user_id: &str,
        page: usize,
        size: usize,
    ) -> Result<Vec<WalletTransactionRow>, DbError>;
    async fn count_wallet_transactions(&self, user_id: &str) -> Result<usize, DbError>;
    async fn list_wallet_tx_by_dates(
        &self,
        user_id: Option<&str>,
        page: usize,
        size: usize,
        since: Option<&str>,
        until: Option<&str>,
        tx_type: Option<&str>,
    ) -> Result<(Vec<WalletTransactionRow>, usize), DbError>;
    async fn get_total_consumed(&self, user_id: &str) -> Result<Decimal, DbError>;
    async fn get_total_recharged(&self, user_id: &str) -> Result<Decimal, DbError>;
    async fn get_wallet_estimated_days(&self, user_id: &str) -> Result<Option<Decimal>, DbError>;

    // ── Recharge Keys ────────────────────────────────────────────────────
    async fn create_recharge_key(
        &self,
        key: &str,
        amount: Decimal,
        created_by: &str,
        expires_at: Option<&str>,
    ) -> Result<(), DbError>;
    async fn redeem_recharge_key(&self, key: &str, user_id: &str) -> Result<Decimal, DbError>;
    async fn revoke_recharge_key(&self, key: &str) -> Result<(), DbError>;
    async fn list_recharge_keys(&self) -> Result<Vec<RechargeKeyRow>, DbError>;
    async fn list_recharge_keys_paginated(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<RechargeKeyRow>, DbError>;
    async fn count_recharge_keys_filtered(
        &self,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<usize, DbError>;
    async fn list_recharge_keys_filtered(
        &self,
        limit: usize,
        offset: usize,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<Vec<RechargeKeyRow>, DbError>;

    // ── Settings ─────────────────────────────────────────────────────────
    async fn get_setting(&self, key: &str) -> Result<Option<String>, DbError>;
    async fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError>;
    async fn get_gateway_config(
        &self,
    ) -> Result<crate::config::types::GatewayRuntimeConfig, DbError>;
    async fn set_gateway_config(
        &self,
        config: &crate::config::types::GatewayRuntimeConfig,
    ) -> Result<(), DbError>;
    async fn get_balances_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, Decimal)>, DbError>;

    // ── Announcements ─────────────────────────────────────────────────────
    async fn list_announcements(&self) -> Result<Vec<AnnouncementRow>, DbError>;
    async fn list_published_announcements(&self) -> Result<Vec<AnnouncementRow>, DbError>;
    async fn get_announcement(&self, id: &str) -> Result<Option<AnnouncementRow>, DbError>;
    async fn create_announcement(&self, a: &AnnouncementRow) -> Result<(), DbError>;
    async fn update_announcement(&self, a: &AnnouncementRow) -> Result<(), DbError>;
    async fn delete_announcement(&self, id: &str) -> Result<(), DbError>;

    // ── Content Filter Rules ──────────────────────────────────────────────
    async fn list_filter_rules(&self) -> Result<Vec<ContentFilterRule>, DbError>;
    async fn create_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError>;
    async fn update_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError>;
    async fn delete_filter_rule(&self, id: &str) -> Result<(), DbError>;

    // ── Health Probe Results ──────────────────────────────────────────────
    async fn insert_probe_result(&self, row: &ProbeResultRow) -> Result<(), DbError>;
    /// Returns the most recent probe result for each (channel, endpoint_url)
    /// target. Channel-scoped failures with no endpoint URL are preserved as a
    /// separate latest record.
    async fn all_latest_probe_results(&self) -> Result<Vec<ProbeResultRow>, DbError>;

    /// Raw probe results from the last `minutes` minutes, newest first.
    /// Used by the flow-control endpoint state timeline (probe-driven grid).
    async fn recent_probe_results(&self, minutes: i64) -> Result<Vec<ProbeResultRow>, DbError>;

    /// Per-model per-channel usage stats for the health/routing dashboard.
    /// Returns Vec<(channel_id, model, requests_count, success_count, avg_latency, p95_latency)>.
    async fn channel_usage_24h(&self)
        -> Result<Vec<(String, String, u64, u64, f64, f64)>, DbError>;

    /// Aggregated (model, channel_id, endpoint_id, count) for the last N hours.
    /// Used by the routing flow panel to restore history on page load.
    async fn routing_flow_snapshot(
        &self,
        hours: u32,
    ) -> Result<Vec<(String, String, Option<i64>, u64)>, DbError>;
    /// Recent request paths with endpoint_id for the routing flow panel.
    /// Returns Vec<(timestamp, model, channel_id, Option<endpoint_id>, latency_ms, success)>.
    async fn recent_request_paths(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, String, Option<i64>, u64, bool)>, DbError>;

    /// Time-bucketed aggregates for routing flow history charts.
    /// Bucket size: hourly when span < 2 days, daily otherwise.
    async fn routing_history_buckets(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<super::RoutingHistoryBucket>, DbError>;

    // ── Casbin Policies ─────────────────────────────────────────────────
    async fn casbin_list_policies(
        &self,
    ) -> Result<Vec<(String, String, String, String, String, String, String)>, DbError>;
    async fn casbin_add_policy(
        &self,
        ptype: &str,
        v0: &str,
        v1: &str,
        v2: &str,
        v3: &str,
        v4: &str,
        v5: &str,
    ) -> Result<(), DbError>;
    async fn casbin_remove_policy(&self, ptype: &str, v0: &str, v1: &str) -> Result<(), DbError>;

    /// Per-endpoint aggregate stats with P95 for routing flow history summary table.
    async fn routing_history_endpoint_stats(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<super::RoutingEndpointStat>, DbError>;

    /// Per-(channel, endpoint_id) aggregate stats with P95 for the detail rows
    /// under each channel in the history summary table.
    /// Returns Vec<(channel_id, endpoint_id, endpoint_url, requests, successes, avg_latency, p95_latency)>.
    async fn routing_history_endpoint_details(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<(String, Option<i64>, Option<String>, u64, u64, f64, f64)>, DbError>;

    // ── Batch Operations (used by background writer) ─────────────────────
    /// Insert a batch of usage records with wallet deduction in a single transaction.
    /// Returns Vec<(user_id, new_balance, frozen)> for each deduction that occurred.
    async fn batch_insert_usage_with_billing(
        &self,
        batch: &[UsageRecord],
        billing_enabled: bool,
    ) -> Result<Vec<(String, Decimal, Decimal)>, DbError>;
}
