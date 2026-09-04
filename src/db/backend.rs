use async_trait::async_trait;
use rust_decimal::Decimal;

use crate::db::ManagementApiKey;
use crate::domain::billing_group::{BillingGroupRow, BillingPaymentMode};
use crate::domain::channel::{Channel, Endpoint};
use crate::domain::gateway::GatewayRoute;
use crate::domain::model::{Model, Pricing};
use crate::domain::moderation::ContentFilterRule;
use crate::domain::routing::RoutingRule;
use crate::domain::scheduler::SchedulerEndpointPolicy;
use crate::domain::sso::SsoConfigRow;
use crate::domain::team::{Team, TeamMember};
use crate::domain::token_package::{
    TokenPackageGrantRow, TokenPackagePlanRow, TokenReservationHandle, TokenReservationRequest,
    TokenSettlementRequest,
};
use crate::domain::usage::UsageRecord;
use crate::domain::user::{ApiKey, User};
use chrono::{DateTime, Utc};

use super::{AnnouncementRow, DbError, RechargeKeyRow, WalletTransactionRow};

/// PostgreSQL persistence contract used by application services.
///
/// The contract remains separate from `PgBackend` so services can use test
/// doubles without adding another production database implementation.
#[allow(dead_code, clippy::too_many_arguments)]
#[async_trait]
pub trait CoreBackend: Send + Sync {
    // ── Migration ────────────────────────────────────────────────────────
    async fn migrate(&self) -> Result<(), DbError>;
    /// Connectivity check (SELECT 1). Used by readiness probes.
    async fn ping(&self) -> Result<(), DbError>;
    /// PostgreSQL 连接池（供自洽子系统复用同一连接池）。
    fn pg_pool(&self) -> &sqlx_postgres::PgPool;
}

#[async_trait]
pub trait UsersBackend: Send + Sync {
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
}

#[async_trait]
pub trait AccessBackend: Send + Sync {
    // ── API Keys ─────────────────────────────────────────────────────────
    async fn list_api_keys(&self, user_id: &str) -> Result<Vec<ApiKey>, DbError>;
    async fn create_api_key(&self, key: &ApiKey) -> Result<(), DbError>;
    async fn create_api_key_with_scopes(
        &self,
        key: &ApiKey,
        scopes: &[String],
    ) -> Result<(), DbError> {
        self.create_api_key(key).await?;
        for scope in scopes {
            self.add_api_key_scope(&key.key, scope, "*", "invoke")
                .await?;
        }
        Ok(())
    }
    async fn delete_api_key(&self, key: &str) -> Result<(), DbError>;
    async fn update_api_key(&self, key: &ApiKey) -> Result<(), DbError>;
    async fn lookup_key(&self, key: &str) -> Result<Option<(User, ApiKey)>, DbError>;
    async fn all_api_keys(&self) -> Result<Vec<(User, ApiKey)>, DbError>;

    // ── Dedicated backend management keys ───────────────────────────────
    async fn list_management_api_keys(&self) -> Result<Vec<ManagementApiKey>, DbError>;
    async fn create_management_api_key(&self, key: &ManagementApiKey) -> Result<(), DbError>;
    async fn set_management_api_key_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<bool, DbError>;
    async fn delete_management_api_key(&self, id: &str) -> Result<bool, DbError>;
    async fn lookup_management_api_key(
        &self,
        key_hash: &str,
    ) -> Result<Option<ManagementApiKey>, DbError>;
    async fn touch_management_api_key(&self, id: &str, used_at: &str) -> Result<(), DbError>;

    // ── Billing groups ────────────────────────────────────────────────
    async fn list_billing_groups(&self, active_only: bool)
        -> Result<Vec<BillingGroupRow>, DbError>;
    async fn get_billing_group(&self, id: &str) -> Result<Option<BillingGroupRow>, DbError>;
    async fn create_billing_group(
        &self,
        id: &str,
        name: &str,
        payment_mode: BillingPaymentMode,
        created_by: &str,
    ) -> Result<BillingGroupRow, DbError>;
    async fn set_billing_group_status(&self, id: &str, status: &str) -> Result<(), DbError>;
    async fn delete_billing_group(
        &self,
        id: &str,
        actor_id: &str,
        reason: &str,
    ) -> Result<(), DbError>;

    // ── API Key Scopes（Platform API Key） ─────────────────────────────
    async fn add_api_key_scope(
        &self,
        api_key_id: &str,
        resource_type: &str,
        resource_id: &str,
        action: &str,
    ) -> Result<(), DbError>;
    /// key 是否有该资源的访问范围；`*` 表示该资源类型的全局范围。
    async fn api_key_has_resource_scope(
        &self,
        api_key_id: &str,
        resource_type: &str,
        resource_id: &str,
        action: &str,
    ) -> Result<bool, DbError>;
}

/// API Gateway 路由持久化（纯 API 网关业务配置，PG 侧）。
#[async_trait]
pub trait GatewayBackend: Send + Sync {
    async fn list_gateway_routes(&self) -> Result<Vec<GatewayRoute>, DbError>;
    async fn get_gateway_route(&self, id: &str) -> Result<Option<GatewayRoute>, DbError>;
    async fn create_gateway_route(&self, route: &GatewayRoute) -> Result<(), DbError>;
    async fn update_gateway_route(&self, route: &GatewayRoute) -> Result<(), DbError>;
    async fn delete_gateway_route(&self, id: &str) -> Result<(), DbError>;
}

#[async_trait]
pub trait CatalogBackend: Send + Sync {
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

    // ── Scheduler policies (Scheduler Control plane) ────────────────────
    async fn list_scheduler_endpoint_policies(
        &self,
    ) -> Result<Vec<SchedulerEndpointPolicy>, DbError>;
    /// Atomic replace of a model's endpoint policies.
    async fn replace_endpoint_policies(
        &self,
        model_id: &str,
        endpoints: &[SchedulerEndpointPolicy],
    ) -> Result<(), DbError>;

    // ── Routing Rules ────────────────────────────────────────────────────
    async fn list_rules(&self) -> Result<Vec<RoutingRule>, DbError>;
    async fn create_rule(&self, r: &RoutingRule) -> Result<(), DbError>;
    async fn update_rule(&self, r: &RoutingRule) -> Result<(), DbError>;
    async fn delete_rule(&self, id: &str) -> Result<(), DbError>;
    async fn delete_team_rule(&self, team_id: &str, rule_id: &str) -> Result<bool, DbError>;
    /// List user-level routing rules for a specific user.
    async fn list_user_rules(&self, user_id: &str) -> Result<Vec<RoutingRule>, DbError>;
}

#[async_trait]
pub trait BillingQueryBackend: Send + Sync {
    // ── Usage Logs ───────────────────────────────────────────────────────
    // ── Billing / Period ─────────────────────────────────────────────────
    async fn period_summary(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64), DbError>;
    async fn period_wallet_amount(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Decimal, DbError>;
    async fn period_summary_since(
        &self,
        start: &str,
        user_id: Option<&str>,
    ) -> Result<Decimal, DbError>;
    async fn billing_event_modes(
        &self,
        request_ids: &[String],
    ) -> Result<std::collections::HashMap<String, (String, Option<String>)>, DbError>;
    async fn usage_billing(
        &self,
        user_id: &str,
        request_ids: &[String],
    ) -> Result<Vec<crate::db::UsageBillingRow>, DbError>;
    async fn usage_billing_for_requests(
        &self,
        request_ids: &[String],
    ) -> Result<Vec<crate::db::UsageBillingRow>, DbError>;
    async fn list_billing_activities(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
        filter: &crate::db::BillingActivityFilter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<crate::db::BillingActivityRow>, DbError>;
    async fn count_billing_activities(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
        filter: &crate::db::BillingActivityFilter,
    ) -> Result<usize, DbError>;
    async fn billing_activity_summary(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
    ) -> Result<crate::db::BillingActivitySummary, DbError>;
    async fn billing_activity_dimensions(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
    ) -> Result<crate::db::BillingActivityDimensions, DbError>;
    async fn period_token_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, u64, Decimal)>, DbError>;
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
    async fn admin_billing_active_counts(
        &self,
        year: i32,
        month: u32,
    ) -> Result<(u64, u64), DbError>;
    async fn admin_billing_team_spend_ranking(
        &self,
        year: i32,
        month: u32,
        limit: usize,
    ) -> Result<Vec<(String, String, Decimal, u64, u64, u64)>, DbError>;
    async fn admin_billing_teams_page(
        &self,
        year: i32,
        month: u32,
        search: Option<&str>,
        sort_by: Option<&str>,
        sort_order: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<(
                String,
                String,
                String,
                Decimal,
                u64,
                u64,
                u64,
                Option<String>,
            )>,
            usize,
        ),
        DbError,
    >;
    async fn admin_billing_team_users_page(
        &self,
        team_id: &str,
        year: i32,
        month: u32,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<(String, String, Decimal, u64, u64, Option<String>)>,
            usize,
        ),
        DbError,
    >;
    async fn admin_billing_scoped_period_summary(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64, Vec<(String, u64, Decimal)>), DbError>;
    async fn admin_billing_scoped_model_breakdown(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal)>, DbError>;
    async fn admin_billing_scoped_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, Decimal)>, DbError>;
    async fn admin_billing_daily_trend(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError>;
    async fn admin_billing_scoped_count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<usize, DbError>;
    async fn admin_billing_scoped_daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError>;
    async fn admin_billing_scoped_period_summary_all(
        &self,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError>;
    async fn admin_billing_user_spend_ranking(
        &self,
        year: i32,
        month: u32,
        limit: usize,
    ) -> Result<
        Vec<(
            Option<String>,
            Option<String>,
            u64,
            bool,
            String,
            String,
            Decimal,
            u64,
            u64,
            u64,
            Option<String>,
        )>,
        DbError,
    >;
    async fn admin_billing_user_api_keys_page(
        &self,
        team_id: Option<&str>,
        user_id: &str,
        year: i32,
        month: u32,
        limit: usize,
        offset: usize,
    ) -> Result<
        (
            Vec<(
                Option<String>,
                Decimal,
                u64,
                u64,
                u64,
                u64,
                u64,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<bool>,
                Option<String>,
            )>,
            usize,
        ),
        DbError,
    >;
    async fn lookup_model_pricing(
        &self,
        model_name: &str,
    ) -> Result<(Decimal, Decimal, Decimal, Decimal), DbError>;
}

#[async_trait]
pub trait WalletBackend: Send + Sync {
    // ── Wallet ───────────────────────────────────────────────────────────
    async fn get_wallet_balance(&self, user_id: &str) -> Result<(Decimal, Decimal), DbError>;
    async fn get_wallet_request_reserved(&self, user_id: &str) -> Result<Decimal, DbError>;
    async fn get_total_wallet_consumed(&self, user_id: &str) -> Result<Decimal, DbError>;
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
    async fn get_total_recharged(&self, user_id: &str) -> Result<Decimal, DbError>;
    async fn get_wallet_estimated_days(&self, user_id: &str) -> Result<Option<Decimal>, DbError>;

    // ── Recharge Keys ────────────────────────────────────────────────────
    async fn create_recharge_key(
        &self,
        key: &str,
        amount: Decimal,
        created_by: &str,
        expires_at: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<(), DbError>;
    async fn redeem_recharge_key(
        &self,
        key: &str,
        user_id: &str,
    ) -> Result<(Decimal, Option<String>), DbError>;
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
}

#[async_trait]
pub trait SystemBackend: Send + Sync {
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
    /// Returns the most recent probe result for each (channel, endpoint_url)
    /// target. Channel-scoped failures with no endpoint URL are preserved as a
    /// separate latest record.
    /// Raw probe results from the last `minutes` minutes, newest first.
    /// Used by the flow-control endpoint state timeline (probe-driven grid).
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
}

#[async_trait]
pub trait TokenBillingBackend: Send + Sync {
    // ── Batch Operations (used by background writer) ─────────────────────
    /// Insert a batch of usage records with wallet deduction in a single transaction.
    /// Returns Vec<(user_id, new_balance, frozen)> for each deduction that occurred.
    async fn batch_insert_usage_with_billing(
        &self,
        batch: &[UsageRecord],
        billing_enabled: bool,
    ) -> Result<Vec<(String, Decimal, Decimal)>, DbError>;

    // ── Token resource packages ──────────────────────────────────────────
    async fn list_token_package_plans(&self) -> Result<Vec<TokenPackagePlanRow>, DbError>;
    async fn delete_token_package_plan(&self, plan_id: &str) -> Result<(), DbError>;
    async fn revoke_token_package_grant(&self, grant_id: &str) -> Result<(), DbError>;
    async fn create_token_package_plan(
        &self,
        id: &str,
        code: &str,
        name: &str,
        accounting_mode: &str,
        display_token_amount: i64,
        total_units: i64,
        input_credit_factor: Decimal,
        output_credit_factor: Decimal,
        cache_credit_factor: Decimal,
        exhaustion_policy: &str,
        priority: i32,
        validity_days: Option<i32>,
        created_by: &str,
    ) -> Result<TokenPackagePlanRow, DbError>;
    async fn list_token_package_grants(
        &self,
        user_id: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<Vec<TokenPackageGrantRow>, DbError>;
    async fn create_token_package_grant(
        &self,
        grant_id: &str,
        plan_id: &str,
        user_id: Option<&str>,
        team_id: Option<&str>,
        source: &str,
        note: &str,
        expires_at: Option<&str>,
    ) -> Result<TokenPackageGrantRow, DbError>;
    async fn reserve_token_request(
        &self,
        request: &TokenReservationRequest,
    ) -> Result<TokenReservationHandle, DbError>;
    async fn settle_token_request(
        &self,
        settlement: &TokenSettlementRequest,
    ) -> Result<(), DbError>;
    async fn release_token_request(
        &self,
        reservation_id: &str,
        reason: &str,
    ) -> Result<(), DbError>;
    /// Reclaim expired reservations. Only rows still in `reserved` state and
    /// whose expiry has passed are eligible; the operation is idempotent.
    async fn reclaim_expired_token_reservations(&self, limit: usize) -> Result<usize, DbError>;
    async fn recover_token_settlement_receivables(
        &self,
        limit: usize,
        worker_id: &str,
    ) -> Result<usize, DbError>;
    async fn apply_token_settlement_payment(
        &self,
        receivable_id: &str,
        payment_sequence: i64,
        payment_type: &str,
        idempotency_key: &str,
        amount: Decimal,
    ) -> Result<bool, DbError>;
    async fn token_request_billing_amount(
        &self,
        request_id: &str,
    ) -> Result<Option<(bool, Decimal, String, Option<String>, Option<String>)>, DbError>;
    async fn settle_released_token_request(
        &self,
        request_id: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        cache_hit_input_tokens: u64,
    ) -> Result<(), DbError>;
}

#[async_trait]
pub trait TeamsSsoBackend: Send + Sync {
    // ── Teams ─────────────────────────────────────────────────────────────
    async fn create_team(&self, team: &Team, owner_id: &str) -> Result<(), DbError>;
    async fn get_team(&self, team_id: &str) -> Result<Option<Team>, DbError>;
    async fn list_teams_for_user(&self, user_id: &str) -> Result<Vec<Team>, DbError>;
    async fn list_all_teams(&self) -> Result<Vec<Team>, DbError>;
    async fn update_team(&self, team_id: &str, name: &str) -> Result<(), DbError>;
    async fn delete_team(&self, team_id: &str) -> Result<(), DbError>;

    async fn add_team_member(
        &self,
        team_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<(), DbError>;
    async fn remove_team_member(&self, team_id: &str, user_id: &str) -> Result<(), DbError>;
    async fn set_team_member_role(
        &self,
        team_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<(), DbError>;
    async fn list_team_members(&self, team_id: &str) -> Result<Vec<TeamMember>, DbError>;
    async fn get_team_member(
        &self,
        team_id: &str,
        user_id: &str,
    ) -> Result<Option<TeamMember>, DbError>;
    /// All memberships across all teams, for cache loading. Returns
    /// (team_id, user_id, role) triples.
    async fn all_team_members(&self) -> Result<Vec<TeamMember>, DbError>;

    /// Team wallet balance as (balance, frozen).
    async fn get_team_wallet(&self, team_id: &str) -> Result<Option<(f64, f64)>, DbError>;
    /// Add `amount` to the team wallet balance (admin credit / recharge).
    async fn add_team_wallet_balance(&self, team_id: &str, amount: f64) -> Result<(), DbError>;
    /// Team wallet transactions, newest first. Returns (rows, total_count).
    async fn list_team_wallet_transactions(
        &self,
        team_id: &str,
        page: usize,
        size: usize,
    ) -> Result<(Vec<WalletTransactionRow>, usize), DbError>;
    /// Team-scoped API keys (api_keys where team_id = $1).
    async fn list_team_api_keys(&self, team_id: &str) -> Result<Vec<ApiKey>, DbError>;
    /// Team-scoped routing rules (scope='user' AND team_id = $1).
    async fn list_team_rules(&self, team_id: &str) -> Result<Vec<RoutingRule>, DbError>;

    // ── SSO Configs ─────────────────────────────────────────────────────────
    async fn list_sso_configs(&self) -> Result<Vec<SsoConfigRow>, DbError>;
    async fn get_sso_config(&self, id: &str) -> Result<Option<SsoConfigRow>, DbError>;
    async fn get_sso_config_by_team(&self, team_id: &str) -> Result<Option<SsoConfigRow>, DbError>;
    async fn create_sso_config(&self, config: &SsoConfigRow) -> Result<(), DbError>;
    async fn update_sso_config(&self, config: &SsoConfigRow) -> Result<(), DbError>;
    async fn delete_sso_config(&self, id: &str) -> Result<(), DbError>;

    /// SSO user → IdP organizations mapping. Returns (user_id, orgs_json).
    async fn list_sso_user_orgs(&self) -> Result<Vec<(String, String)>, DbError>;
    /// Upsert a user's IdP organizations (orgs_json = JSON array of SsoOrg).
    async fn upsert_sso_user_orgs(&self, user_id: &str, orgs_json: &str) -> Result<(), DbError>;
}

pub trait DbBackend:
    CoreBackend
    + UsersBackend
    + AccessBackend
    + CatalogBackend
    + BillingQueryBackend
    + WalletBackend
    + SystemBackend
    + TokenBillingBackend
    + TeamsSsoBackend
    + GatewayBackend
{
}

impl<T> DbBackend for T where
    T: CoreBackend
        + UsersBackend
        + AccessBackend
        + CatalogBackend
        + BillingQueryBackend
        + WalletBackend
        + SystemBackend
        + TokenBillingBackend
        + TeamsSsoBackend
        + GatewayBackend
{
}
