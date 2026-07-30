pub mod backend;
pub mod pg_backend;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::config::types::GatewayRuntimeConfig;
use crate::db::backend::DbBackend;
use crate::domain::channel::{Channel, Endpoint};
use crate::domain::model::{Model, Pricing};
use crate::domain::moderation::ContentFilterRule;
use crate::domain::routing::RoutingRule;
use crate::domain::usage::{UsageFilter, UsageRecord};
use crate::domain::user::{ApiKey, User};

#[derive(Debug)]
pub struct DbError(pub String);

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DbError {}

impl From<sqlx_core::Error> for DbError {
    fn from(e: sqlx_core::Error) -> Self {
        Self(e.to_string())
    }
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct FunnelStats {
    pub total: u64,
    pub success_count: u64,
    pub auth_fail_count: u64,
    pub rate_limit_count: u64,
    pub bad_request_count: u64,
    pub upstream_error_count: u64,
    pub timeout_count: u64,
    pub other_error_count: u64,
    pub p50_latency: f64,
    pub p95_latency: f64,
    pub p99_latency: f64,
    pub avg_latency: f64,
}

#[derive(Debug, Clone)]
pub struct WalletTransactionRow {
    pub id: String,
    #[allow(dead_code)]
    pub user_id: String,
    pub tx_type: String,
    pub amount: Decimal,
    pub balance_before: Decimal,
    pub balance_after: Decimal,
    pub method: String,
    pub status: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RechargeKeyRow {
    pub key: String,
    #[serde(with = "rust_decimal::serde::float")]
    pub amount: Decimal,
    pub used_by: Option<String>,
    pub used_at: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProbeResultRow {
    pub id: String,
    pub channel_id: String,
    pub model_id: String,
    pub success: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
    pub probed_at: String,
    #[serde(default)]
    pub endpoint_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnnouncementRow {
    pub id: String,
    pub title: String,
    pub content: String,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub published: bool,
}

/// Per-time-bucket per-channel per-endpoint aggregate for routing flow history charts.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RoutingHistoryBucket {
    pub bucket: String,
    pub channel_id: String,
    pub endpoint_id: Option<i64>,
    pub requests: u64,
    pub successes: u64,
    pub avg_latency: f64,
}

/// Per-endpoint summary with P95 latency for routing flow history table.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RoutingEndpointStat {
    pub channel_id: String,
    pub endpoint_id: Option<i64>,
    pub requests: u64,
    pub successes: u64,
    pub avg_latency: f64,
    pub p95_latency: f64,
}

pub struct Database {
    pub backend: Box<dyn DbBackend>,
}

#[allow(dead_code)]
impl Database {
    pub async fn new(pg_url: &str) -> Self {
        let backend = pg_backend::PgBackend::new(pg_url)
            .await
            .expect("Failed to create PostgreSQL backend");
        Self {
            backend: Box::new(backend),
        }
    }

    // ── Migration ────────────────────────────────────────────────────────
    pub async fn migrate(&self) -> Result<(), DbError> {
        self.backend.migrate().await
    }

    // ── Users ────────────────────────────────────────────────────────────
    pub async fn list_users(&self, status: Option<&str>) -> Result<Vec<User>, DbError> {
        self.backend.list_users(status).await
    }
    pub async fn get_user(&self, id: &str) -> Result<Option<User>, DbError> {
        self.backend.get_user(id).await
    }
    pub async fn get_user_with_password(&self, id: &str) -> Result<Option<User>, DbError> {
        self.backend.get_user_with_password(id).await
    }
    pub async fn create_user(&self, user: &User) -> Result<(), DbError> {
        self.backend.create_user(user).await
    }
    pub async fn create_initial_admin(&self, user: &User) -> Result<(), DbError> {
        self.backend.create_initial_admin(user).await
    }
    pub async fn update_user(&self, user: &User) -> Result<(), DbError> {
        self.backend.update_user(user).await
    }
    pub async fn bump_user_token_version(&self, id: &str) -> Result<(), DbError> {
        self.backend.bump_user_token_version(id).await
    }
    pub async fn update_user_admin_fields(
        &self,
        id: &str,
        name: Option<String>,
        password_hash: Option<String>,
        rate_limits: Option<crate::domain::user::RateLimit>,
        role: Option<String>,
        concurrency_limit: Option<u32>,
    ) -> Result<User, DbError> {
        self.backend
            .update_user_admin_fields(
                id,
                name,
                password_hash,
                rate_limits,
                role,
                concurrency_limit,
            )
            .await
    }
    pub async fn suspend_user(
        &self,
        id: &str,
        suspended_at: &DateTime<Utc>,
    ) -> Result<User, DbError> {
        self.backend.suspend_user(id, suspended_at).await
    }
    pub async fn restore_user(&self, id: &str) -> Result<User, DbError> {
        self.backend.restore_user(id).await
    }
    pub async fn delete_user(&self, id: &str) -> Result<(), DbError> {
        self.backend.delete_user(id).await
    }
    pub async fn count_admins(&self, status: Option<&str>) -> Result<i64, DbError> {
        self.backend.count_admins(status).await
    }
    pub async fn get_user_timezone(&self, id: &str) -> Result<String, DbError> {
        self.backend.get_user_timezone(id).await
    }
    pub async fn update_user_timezone(&self, id: &str, timezone: &str) -> Result<(), DbError> {
        self.backend.update_user_timezone(id, timezone).await
    }
    pub async fn get_user_currency(&self, id: &str) -> Result<String, DbError> {
        self.backend.get_user_currency(id).await
    }
    pub async fn update_user_currency(&self, id: &str, currency: &str) -> Result<(), DbError> {
        self.backend.update_user_currency(id, currency).await
    }

    // ── API Keys ─────────────────────────────────────────────────────────
    pub async fn list_api_keys(&self, user_id: &str) -> Result<Vec<ApiKey>, DbError> {
        self.backend.list_api_keys(user_id).await
    }
    pub async fn create_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        self.backend.create_api_key(key).await
    }
    pub async fn delete_api_key(&self, key: &str) -> Result<(), DbError> {
        self.backend.delete_api_key(key).await
    }
    pub async fn update_api_key(&self, key: &ApiKey) -> Result<(), DbError> {
        self.backend.update_api_key(key).await
    }
    pub async fn lookup_key(&self, key: &str) -> Result<Option<(User, ApiKey)>, DbError> {
        self.backend.lookup_key(key).await
    }
    pub async fn all_api_keys(&self) -> Result<Vec<(User, ApiKey)>, DbError> {
        self.backend.all_api_keys().await
    }

    // ── Channels & Endpoints ─────────────────────────────────────────────
    pub async fn list_channels(&self) -> Result<Vec<Channel>, DbError> {
        self.backend.list_channels().await
    }
    pub async fn get_channel(&self, id: &str) -> Result<Option<Channel>, DbError> {
        self.backend.get_channel(id).await
    }
    pub async fn create_channel(&self, ch: &Channel) -> Result<(), DbError> {
        self.backend.create_channel(ch).await
    }
    pub async fn update_channel(&self, ch: &Channel) -> Result<(), DbError> {
        self.backend.update_channel(ch).await
    }
    pub async fn delete_channel(&self, id: &str) -> Result<(), DbError> {
        self.backend.delete_channel(id).await
    }
    pub async fn get_endpoint(&self, id: i64) -> Result<Option<Endpoint>, DbError> {
        self.backend.get_endpoint(id).await
    }
    pub async fn update_endpoint_api_key(&self, id: i64, api_key: &str) -> Result<(), DbError> {
        self.backend.update_endpoint_api_key(id, api_key).await
    }
    pub async fn update_endpoint_enabled(&self, id: i64, enabled: bool) -> Result<(), DbError> {
        self.backend.update_endpoint_enabled(id, enabled).await
    }

    // ── Models ───────────────────────────────────────────────────────────
    pub async fn list_models(&self) -> Result<Vec<Model>, DbError> {
        self.backend.list_models().await
    }
    pub async fn get_model(&self, id: &str) -> Result<Option<Model>, DbError> {
        self.backend.get_model(id).await
    }
    pub async fn create_model(&self, m: &Model) -> Result<(), DbError> {
        self.backend.create_model(m).await
    }
    pub async fn update_model(&self, old_id: &str, m: &Model) -> Result<(), DbError> {
        self.backend.update_model(old_id, m).await
    }
    pub async fn delete_model(&self, id: &str) -> Result<(), DbError> {
        self.backend.delete_model(id).await
    }
    pub async fn list_published_models(&self) -> Result<Vec<Model>, DbError> {
        self.backend.list_published_models().await
    }
    pub async fn set_model_published(&self, id: &str, published: bool) -> Result<(), DbError> {
        self.backend.set_model_published(id, published).await
    }
    pub async fn set_model_pricing(&self, id: &str, pricing: &Pricing) -> Result<(), DbError> {
        self.backend.set_model_pricing(id, pricing).await
    }
    pub async fn set_model_context_length(
        &self,
        id: &str,
        context_length: i64,
    ) -> Result<(), DbError> {
        self.backend
            .set_model_context_length(id, context_length)
            .await
    }

    // ── Routing Rules ────────────────────────────────────────────────────
    pub async fn list_rules(&self) -> Result<Vec<RoutingRule>, DbError> {
        self.backend.list_rules().await
    }
    pub async fn create_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        self.backend.create_rule(r).await
    }
    pub async fn update_rule(&self, r: &RoutingRule) -> Result<(), DbError> {
        self.backend.update_rule(r).await
    }
    pub async fn delete_rule(&self, id: &str) -> Result<(), DbError> {
        self.backend.delete_rule(id).await
    }
    pub async fn list_user_rules(&self, user_id: &str) -> Result<Vec<RoutingRule>, DbError> {
        self.backend.list_user_rules(user_id).await
    }

    // ── Usage Logs ───────────────────────────────────────────────────────
    pub async fn insert_usage(&self, record: &UsageRecord) -> Result<(), DbError> {
        self.backend.insert_usage(record).await
    }
    pub async fn count_usage(&self) -> Result<usize, DbError> {
        self.backend.count_usage().await
    }
    pub async fn count_usage_by_user(&self, user_id: &str) -> Result<usize, DbError> {
        self.backend.count_usage_by_user(user_id).await
    }
    pub async fn count_usage_filtered(&self, filter: &UsageFilter) -> Result<usize, DbError> {
        self.backend.count_usage_filtered(filter).await
    }
    pub async fn query_usage(
        &self,
        limit: usize,
        offset: usize,
        filter: &UsageFilter,
    ) -> Result<Vec<UsageRecord>, DbError> {
        self.backend.query_usage(limit, offset, filter).await
    }
    pub async fn get_usage_detail(&self, request_id: &str) -> Result<Option<UsageRecord>, DbError> {
        self.backend.get_usage_detail(request_id).await
    }
    pub async fn purge_usage_logs(&self, cutoff: &str) -> Result<usize, DbError> {
        self.backend.purge_usage_logs(cutoff).await
    }
    pub async fn usage_stats_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<(u64, u64, u64, u64), DbError> {
        self.backend.usage_stats_since(since, user_id).await
    }
    pub async fn usage_cost_rows_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, DbError> {
        self.backend.usage_cost_rows_since(since, user_id).await
    }
    pub async fn query_usage_since(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<UsageRecord>, DbError> {
        self.backend.query_usage_since(since, user_id).await
    }
    pub async fn daily_usage_counts(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, i64)>, DbError> {
        self.backend
            .daily_usage_counts(since, user_id, tz_offset_seconds)
            .await
    }
    pub async fn daily_usage_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
        tz_offset_seconds: i64,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64, u64)>, DbError> {
        self.backend
            .daily_usage_stats(since, user_id, tz_offset_seconds)
            .await
    }
    pub async fn model_activity(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64, u64)>, DbError> {
        self.backend.model_activity(since, user_id).await
    }
    pub async fn funnel_stats(
        &self,
        since: &str,
        user_id: Option<&str>,
    ) -> Result<FunnelStats, DbError> {
        self.backend.funnel_stats(since, user_id).await
    }

    pub async fn period_summary(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64), DbError> {
        self.backend.period_summary(year, month, user_id).await
    }
    pub async fn period_model_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal)>, DbError> {
        self.backend
            .period_model_breakdown(year, month, user_id)
            .await
    }
    pub async fn period_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, Decimal)>, DbError> {
        self.backend
            .period_channel_breakdown(year, month, user_id)
            .await
    }
    pub async fn daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError> {
        self.backend.daily_deductions(year, month, user_id).await
    }
    pub async fn count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<usize, DbError> {
        self.backend
            .count_daily_deductions(year, month, user_id)
            .await
    }
    pub async fn daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError> {
        self.backend
            .daily_deductions_paginated(year, month, user_id, limit, offset)
            .await
    }
    pub async fn billing_months(&self) -> Result<Vec<String>, DbError> {
        self.backend.billing_months().await
    }
    pub async fn billing_months_for_user(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        self.backend.billing_months_for_user(user_id).await
    }
    pub async fn period_summary_all(&self) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        self.backend.period_summary_all().await
    }
    pub async fn period_summary_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        self.backend.period_summary_for_user(user_id).await
    }
    pub async fn lookup_model_pricing(
        &self,
        model_name: &str,
    ) -> Result<(Decimal, Decimal, Decimal), DbError> {
        self.backend.lookup_model_pricing(model_name).await
    }

    // ── Wallet ───────────────────────────────────────────────────────────
    pub async fn get_wallet_balance(&self, user_id: &str) -> Result<(Decimal, Decimal), DbError> {
        self.backend.get_wallet_balance(user_id).await
    }
    pub async fn update_wallet_balance(
        &self,
        user_id: &str,
        balance: Decimal,
    ) -> Result<(), DbError> {
        self.backend.update_wallet_balance(user_id, balance).await
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn add_wallet_transaction(
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
    ) -> Result<(), DbError> {
        self.backend
            .add_wallet_transaction(
                id,
                user_id,
                tx_type,
                amount,
                balance_before,
                balance_after,
                method,
                status,
                note,
            )
            .await
    }
    pub async fn get_wallet_transactions(
        &self,
        user_id: &str,
        page: usize,
        size: usize,
    ) -> Result<Vec<WalletTransactionRow>, DbError> {
        self.backend
            .get_wallet_transactions(user_id, page, size)
            .await
    }
    pub async fn count_wallet_transactions(&self, user_id: &str) -> Result<usize, DbError> {
        self.backend.count_wallet_transactions(user_id).await
    }
    pub async fn list_wallet_tx_by_dates(
        &self,
        user_id: Option<&str>,
        page: usize,
        size: usize,
        since: Option<&str>,
        until: Option<&str>,
        tx_type: Option<&str>,
    ) -> Result<(Vec<WalletTransactionRow>, usize), DbError> {
        self.backend
            .list_wallet_tx_by_dates(user_id, page, size, since, until, tx_type)
            .await
    }
    pub async fn get_total_consumed(&self, user_id: &str) -> Result<Decimal, DbError> {
        self.backend.get_total_consumed(user_id).await
    }
    pub async fn get_total_recharged(&self, user_id: &str) -> Result<Decimal, DbError> {
        self.backend.get_total_recharged(user_id).await
    }
    pub async fn get_wallet_estimated_days(
        &self,
        user_id: &str,
    ) -> Result<Option<Decimal>, DbError> {
        self.backend.get_wallet_estimated_days(user_id).await
    }

    // ── Recharge Keys ────────────────────────────────────────────────────
    pub async fn create_recharge_key(
        &self,
        key: &str,
        amount: Decimal,
        created_by: &str,
        expires_at: Option<&str>,
    ) -> Result<(), DbError> {
        self.backend
            .create_recharge_key(key, amount, created_by, expires_at)
            .await
    }
    pub async fn redeem_recharge_key(&self, key: &str, user_id: &str) -> Result<Decimal, DbError> {
        self.backend.redeem_recharge_key(key, user_id).await
    }
    pub async fn revoke_recharge_key(&self, key: &str) -> Result<(), DbError> {
        self.backend.revoke_recharge_key(key).await
    }
    pub async fn list_recharge_keys(&self) -> Result<Vec<RechargeKeyRow>, DbError> {
        self.backend.list_recharge_keys().await
    }
    pub async fn list_recharge_keys_paginated(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<RechargeKeyRow>, DbError> {
        self.backend
            .list_recharge_keys_paginated(limit, offset)
            .await
    }
    pub async fn count_recharge_keys_filtered(
        &self,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<usize, DbError> {
        self.backend
            .count_recharge_keys_filtered(search, status, user_search)
            .await
    }
    pub async fn list_recharge_keys_filtered(
        &self,
        limit: usize,
        offset: usize,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
    ) -> Result<Vec<RechargeKeyRow>, DbError> {
        self.backend
            .list_recharge_keys_filtered(limit, offset, search, status, user_search)
            .await
    }

    // ── Settings ─────────────────────────────────────────────────────────
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        self.backend.get_setting(key).await
    }
    pub async fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        self.backend.set_setting(key, value).await
    }
    pub async fn get_gateway_config(&self) -> Result<GatewayRuntimeConfig, DbError> {
        self.backend.get_gateway_config().await
    }
    pub async fn set_gateway_config(&self, config: &GatewayRuntimeConfig) -> Result<(), DbError> {
        self.backend.set_gateway_config(config).await
    }
    // ── Casbin Policies ─────────────────────────────────────────────────
    pub async fn casbin_list_policies(
        &self,
    ) -> Result<Vec<(String, String, String, String, String, String, String)>, DbError> {
        self.backend.casbin_list_policies().await
    }

    pub async fn casbin_add_policy(
        &self,
        ptype: &str,
        v0: &str,
        v1: &str,
        v2: &str,
        v3: &str,
        v4: &str,
        v5: &str,
    ) -> Result<(), DbError> {
        self.backend
            .casbin_add_policy(ptype, v0, v1, v2, v3, v4, v5)
            .await
    }

    pub async fn casbin_remove_policy(
        &self,
        ptype: &str,
        v0: &str,
        v1: &str,
    ) -> Result<(), DbError> {
        self.backend.casbin_remove_policy(ptype, v0, v1).await
    }

    // ── Announcements ─────────────────────────────────────────────────
    pub async fn list_announcements(&self) -> Result<Vec<AnnouncementRow>, DbError> {
        self.backend.list_announcements().await
    }
    pub async fn list_published_announcements(&self) -> Result<Vec<AnnouncementRow>, DbError> {
        self.backend.list_published_announcements().await
    }
    pub async fn get_announcement(&self, id: &str) -> Result<Option<AnnouncementRow>, DbError> {
        self.backend.get_announcement(id).await
    }
    pub async fn create_announcement(&self, a: &AnnouncementRow) -> Result<(), DbError> {
        self.backend.create_announcement(a).await
    }
    pub async fn update_announcement(&self, a: &AnnouncementRow) -> Result<(), DbError> {
        self.backend.update_announcement(a).await
    }
    pub async fn delete_announcement(&self, id: &str) -> Result<(), DbError> {
        self.backend.delete_announcement(id).await
    }

    pub async fn get_balances_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, Decimal)>, DbError> {
        self.backend.get_balances_page(limit, offset).await
    }

    // ── Content Filter Rules ─────────────────────────────────────────────
    pub async fn list_filter_rules(&self) -> Result<Vec<ContentFilterRule>, DbError> {
        self.backend.list_filter_rules().await
    }
    pub async fn create_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError> {
        self.backend.create_filter_rule(rule).await
    }
    pub async fn update_filter_rule(&self, rule: &ContentFilterRule) -> Result<(), DbError> {
        self.backend.update_filter_rule(rule).await
    }
    pub async fn delete_filter_rule(&self, id: &str) -> Result<(), DbError> {
        self.backend.delete_filter_rule(id).await
    }

    // ── Health Probe Results ─────────────────────────────────────────────
    pub async fn insert_probe_result(&self, row: &ProbeResultRow) -> Result<(), DbError> {
        self.backend.insert_probe_result(row).await
    }
    pub async fn all_latest_probe_results(&self) -> Result<Vec<ProbeResultRow>, DbError> {
        self.backend.all_latest_probe_results().await
    }

    pub async fn channel_usage_24h(
        &self,
    ) -> Result<Vec<(String, String, u64, u64, f64, f64)>, DbError> {
        self.backend.channel_usage_24h().await
    }

    pub async fn recent_request_paths(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, String, Option<i64>, u64, bool)>, DbError> {
        self.backend.recent_request_paths(limit).await
    }

    pub async fn routing_flow_snapshot(
        &self,
        hours: u32,
    ) -> Result<Vec<(String, String, Option<i64>, u64)>, DbError> {
        self.backend.routing_flow_snapshot(hours).await
    }

    pub async fn routing_history_buckets(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<RoutingHistoryBucket>, DbError> {
        self.backend
            .routing_history_buckets(start, end, model)
            .await
    }

    pub async fn routing_history_endpoint_stats(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<RoutingEndpointStat>, DbError> {
        self.backend
            .routing_history_endpoint_stats(start, end, model)
            .await
    }

    pub async fn routing_history_endpoint_details(
        &self,
        start: &str,
        end: &str,
        model: Option<&str>,
    ) -> Result<Vec<(String, Option<i64>, Option<String>, u64, u64, f64, f64)>, DbError> {
        self.backend
            .routing_history_endpoint_details(start, end, model)
            .await
    }

    // ── Batch Operations ────────────────────────────────────────────────
    pub async fn batch_insert_usage_with_billing(
        &self,
        batch: &[UsageRecord],
        billing_enabled: bool,
    ) -> Result<Vec<(String, Decimal, Decimal)>, DbError> {
        self.backend
            .batch_insert_usage_with_billing(batch, billing_enabled)
            .await
    }
}
