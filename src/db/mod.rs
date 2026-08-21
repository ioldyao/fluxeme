pub mod backend;
pub mod pg_backend;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::config::types::GatewayRuntimeConfig;
use crate::db::backend::DbBackend;
use crate::domain::billing_group::{BillingGroupRow, BillingPaymentMode};
use crate::domain::channel::{Channel, Endpoint};
use crate::domain::model::{Model, Pricing};
use crate::domain::moderation::ContentFilterRule;
use crate::domain::routing::RoutingRule;
use crate::domain::sso::SsoConfigRow;
use crate::domain::team::{Team, TeamMember};
use crate::domain::token_package::{
    TokenPackageGrantRow, TokenPackagePlanRow, TokenReservationHandle, TokenReservationRequest,
    TokenSettlementRequest,
};
use crate::domain::usage::UsageRecord;
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

#[derive(Debug, Clone)]
pub struct BillingActivityRow {
    pub timestamp: String,
    pub request_id: String,
    pub user_id: String,
    pub user_name: String,
    pub model: String,
    pub channel_id: String,
    pub activity_status: String,
    pub status_reason: String,
    pub status_code: u16,
    pub success: bool,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_hit_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    pub package_units: u64,
    pub package_grant_id: Option<String>,
    pub wallet_amount: Decimal,
    pub priced_cost_amount: Decimal,
    pub charge_source: String,
    pub account_type: String,
    pub team_id: Option<String>,
    pub api_key_name: Option<String>,
    pub latency_ms: u64,
    pub reservation_id: Option<String>,
    pub billing_group_id: Option<String>,
    pub billing_group_name: Option<String>,
    pub billing_payment_mode: String,
}

#[derive(Debug, Clone, Default)]
pub struct BillingActivityFilter {
    pub search: Option<String>,
    pub api_key_name: Option<String>,
    pub model: Option<String>,
    pub charge_source: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BillingActivitySummary {
    pub activity_count: u64,
    pub success_count: u64,
    pub failed_count: u64,
    pub interrupted_count: u64,
    pub zero_cost_count: u64,
    pub total_tokens: u64,
    pub package_units: u64,
    #[serde(with = "rust_decimal::serde::float")]
    pub wallet_amount: Decimal,
    #[serde(with = "rust_decimal::serde::float")]
    pub priced_cost_amount: Decimal,
    pub api_key_count: u64,
    pub model_count: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BillingActivityDimensionRow {
    pub name: String,
    pub activity_count: u64,
    pub key_count: u64,
    pub model_count: u64,
    pub related_names: Vec<String>,
    pub source_names: Vec<String>,
    pub total_tokens: u64,
    pub package_units: u64,
    #[serde(with = "rust_decimal::serde::float")]
    pub wallet_amount: Decimal,
    #[serde(with = "rust_decimal::serde::float")]
    pub priced_cost_amount: Decimal,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BillingActivityDimensions {
    pub api_keys: Vec<BillingActivityDimensionRow>,
    pub models: Vec<BillingActivityDimensionRow>,
    pub sources: Vec<BillingActivityDimensionRow>,
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
    /// Team scope. None = personal recharge, Some = team recharge.
    #[serde(default)]
    pub team_id: Option<String>,
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

    /// 底层 PostgreSQL 连接池（供自洽子系统如 SkillHub 复用同一连接池，
    /// 避免重复建池。业务数据归属 PG）。
    pub fn pg_pool(&self) -> &sqlx_postgres::PgPool {
        self.backend.pg_pool()
    }
    pub async fn ping(&self) -> Result<(), DbError> {
        self.backend.ping().await
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

    // ── Billing groups
    pub async fn list_billing_groups(&self, active_only: bool) -> Result<Vec<BillingGroupRow>, DbError> {
        self.backend.list_billing_groups(active_only).await
    }
    pub async fn get_billing_group(&self, id: &str) -> Result<Option<BillingGroupRow>, DbError> {
        self.backend.get_billing_group(id).await
    }
    pub async fn create_billing_group(
        &self,
        id: &str,
        name: &str,
        payment_mode: BillingPaymentMode,
        created_by: &str,
    ) -> Result<BillingGroupRow, DbError> {
        self.backend.create_billing_group(id, name, payment_mode, created_by).await
    }
    pub async fn set_billing_group_status(&self, id: &str, status: &str) -> Result<(), DbError> {
        self.backend.set_billing_group_status(id, status).await
    }

    // ── API Key Scopes（Platform API Key：访问范围 = 资源类型） ──────────
    // 语义：key 创建时勾选可访问的资源类型（model / skill / mcp），
    // 存为 api_key_scopes(resource_id='*')。Skill Runtime 鉴权按资源类型
    // 放行（取消按单个技能授权）。
    pub async fn add_api_key_scope(
        &self,
        api_key_id: &str,
        resource_type: &str,
        resource_id: &str,
        action: &str,
    ) -> Result<(), DbError> {
        self.backend
            .add_api_key_scope(api_key_id, resource_type, resource_id, action)
            .await
    }
    pub async fn api_key_has_resource_scope(
        &self,
        api_key_id: &str,
        resource_type: &str,
        action: &str,
    ) -> Result<bool, DbError> {
        self.backend
            .api_key_has_resource_scope(api_key_id, resource_type, action)
            .await
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
    pub async fn period_summary(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64), DbError> {
        self.backend.period_summary(year, month, user_id).await
    }
    pub async fn period_summary_since(
        &self,
        start: &str,
        user_id: Option<&str>,
    ) -> Result<Decimal, DbError> {
        self.backend.period_summary_since(start, user_id).await
    }
    pub async fn billing_event_modes(
        &self,
        request_ids: &[String],
    ) -> Result<std::collections::HashMap<String, (String, Option<String>)>, DbError> {
        self.backend.billing_event_modes(request_ids).await
    }
    pub async fn list_billing_activities(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
        filter: &BillingActivityFilter,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<BillingActivityRow>, DbError> {
        self.backend
            .list_billing_activities(start, end, user_id, filter, limit, offset)
            .await
    }
    pub async fn count_billing_activities(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
        filter: &BillingActivityFilter,
    ) -> Result<usize, DbError> {
        self.backend
            .count_billing_activities(start, end, user_id, filter)
            .await
    }
    pub async fn billing_activity_summary(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
    ) -> Result<BillingActivitySummary, DbError> {
        self.backend
            .billing_activity_summary(start, end, user_id)
            .await
    }
    pub async fn billing_activity_dimensions(
        &self,
        start: &str,
        end: &str,
        user_id: Option<&str>,
    ) -> Result<BillingActivityDimensions, DbError> {
        self.backend
            .billing_activity_dimensions(start, end, user_id)
            .await
    }
    pub async fn period_token_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, u64, Decimal)>, DbError> {
        self.backend
            .period_token_breakdown(year, month, user_id)
            .await
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
    pub async fn admin_billing_active_counts(
        &self,
        year: i32,
        month: u32,
    ) -> Result<(u64, u64), DbError> {
        self.backend.admin_billing_active_counts(year, month).await
    }
    pub async fn admin_billing_team_spend_ranking(
        &self,
        year: i32,
        month: u32,
        limit: usize,
    ) -> Result<Vec<(String, String, Decimal, u64, u64, u64)>, DbError> {
        self.backend
            .admin_billing_team_spend_ranking(year, month, limit)
            .await
    }
    pub async fn admin_billing_teams_page(
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
    > {
        self.backend
            .admin_billing_teams_page(year, month, search, sort_by, sort_order, limit, offset)
            .await
    }
    pub async fn admin_billing_team_users_page(
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
    > {
        self.backend
            .admin_billing_team_users_page(team_id, year, month, limit, offset)
            .await
    }
    pub async fn admin_billing_scoped_period_summary(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<(Decimal, u64, u64, Vec<(String, u64, Decimal)>), DbError> {
        self.backend
            .admin_billing_scoped_period_summary(year, month, team_id, user_id)
            .await
    }
    pub async fn admin_billing_scoped_model_breakdown(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal)>, DbError> {
        self.backend
            .admin_billing_scoped_model_breakdown(year, month, team_id, user_id)
            .await
    }
    pub async fn admin_billing_scoped_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, String, Decimal)>, DbError> {
        self.backend
            .admin_billing_scoped_channel_breakdown(year, month, team_id, user_id)
            .await
    }
    pub async fn admin_billing_daily_trend(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        self.backend
            .admin_billing_daily_trend(year, month, team_id, user_id)
            .await
    }
    pub async fn admin_billing_scoped_count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<usize, DbError> {
        self.backend
            .admin_billing_scoped_count_daily_deductions(year, month, team_id, user_id)
            .await
    }
    pub async fn admin_billing_scoped_daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        team_id: Option<&str>,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, Decimal, u64)>, DbError> {
        self.backend
            .admin_billing_scoped_daily_deductions_paginated(
                year, month, team_id, user_id, limit, offset,
            )
            .await
    }
    pub async fn admin_billing_scoped_period_summary_all(
        &self,
        team_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, Decimal, u64, u64)>, DbError> {
        self.backend
            .admin_billing_scoped_period_summary_all(team_id, user_id)
            .await
    }
    pub async fn admin_billing_user_spend_ranking(
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
    > {
        self.backend
            .admin_billing_user_spend_ranking(year, month, limit)
            .await
    }
    pub async fn admin_billing_user_api_keys_page(
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
    > {
        self.backend
            .admin_billing_user_api_keys_page(team_id, user_id, year, month, limit, offset)
            .await
    }
    pub async fn lookup_model_pricing(
        &self,
        model_name: &str,
    ) -> Result<(Decimal, Decimal, Decimal, Decimal), DbError> {
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
        team_id: Option<&str>,
    ) -> Result<(), DbError> {
        self.backend
            .create_recharge_key(key, amount, created_by, expires_at, team_id)
            .await
    }
    pub async fn redeem_recharge_key(
        &self,
        key: &str,
        user_id: &str,
    ) -> Result<(Decimal, Option<String>), DbError> {
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
    /// Bump the cross-instance config version so other instances reload
    /// their in-memory caches (routing / auth / content_filter / authz).
    pub async fn bump_config_version(&self) -> Result<(), DbError> {
        let now = chrono::Utc::now().timestamp_millis().to_string();
        self.backend.set_setting("config_version", &now).await
    }

    // ── SSO Configs ─────────────────────────────────────────────────────────
    pub async fn list_sso_configs(&self) -> Result<Vec<SsoConfigRow>, DbError> {
        self.backend.list_sso_configs().await
    }
    pub async fn get_sso_config(&self, id: &str) -> Result<Option<SsoConfigRow>, DbError> {
        self.backend.get_sso_config(id).await
    }
    pub async fn get_sso_config_by_team(
        &self,
        team_id: &str,
    ) -> Result<Option<SsoConfigRow>, DbError> {
        self.backend.get_sso_config_by_team(team_id).await
    }
    pub async fn create_sso_config(&self, config: &SsoConfigRow) -> Result<(), DbError> {
        self.backend.create_sso_config(config).await
    }
    pub async fn update_sso_config(&self, config: &SsoConfigRow) -> Result<(), DbError> {
        self.backend.update_sso_config(config).await
    }
    pub async fn delete_sso_config(&self, id: &str) -> Result<(), DbError> {
        self.backend.delete_sso_config(id).await
    }
    pub async fn list_sso_user_orgs(&self) -> Result<Vec<(String, String)>, DbError> {
        self.backend.list_sso_user_orgs().await
    }
    pub async fn upsert_sso_user_orgs(
        &self,
        user_id: &str,
        orgs_json: &str,
    ) -> Result<(), DbError> {
        self.backend.upsert_sso_user_orgs(user_id, orgs_json).await
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

    // ── Token resource packages ──────────────────────────────────────────
    pub async fn list_token_package_plans(&self) -> Result<Vec<TokenPackagePlanRow>, DbError> {
        self.backend.list_token_package_plans().await
    }
    pub async fn delete_token_package_plan(&self, plan_id: &str) -> Result<(), DbError> {
        self.backend.delete_token_package_plan(plan_id).await
    }
    pub async fn revoke_token_package_grant(&self, grant_id: &str) -> Result<(), DbError> {
        self.backend.revoke_token_package_grant(grant_id).await
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn create_token_package_plan(
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
    ) -> Result<TokenPackagePlanRow, DbError> {
        self.backend
            .create_token_package_plan(
                id,
                code,
                name,
                accounting_mode,
                display_token_amount,
                total_units,
                input_credit_factor,
                output_credit_factor,
                cache_credit_factor,
                exhaustion_policy,
                priority,
                validity_days,
                created_by,
            )
            .await
    }
    pub async fn list_token_package_grants(
        &self,
        user_id: Option<&str>,
        team_id: Option<&str>,
    ) -> Result<Vec<TokenPackageGrantRow>, DbError> {
        self.backend
            .list_token_package_grants(user_id, team_id)
            .await
    }
    pub async fn create_token_package_grant(
        &self,
        grant_id: &str,
        plan_id: &str,
        user_id: Option<&str>,
        team_id: Option<&str>,
        source: &str,
        note: &str,
        expires_at: Option<&str>,
    ) -> Result<TokenPackageGrantRow, DbError> {
        self.backend
            .create_token_package_grant(
                grant_id, plan_id, user_id, team_id, source, note, expires_at,
            )
            .await
    }
    pub async fn reserve_token_request(
        &self,
        request: &TokenReservationRequest,
    ) -> Result<TokenReservationHandle, DbError> {
        self.backend.reserve_token_request(request).await
    }
    pub async fn settle_token_request(
        &self,
        settlement: &TokenSettlementRequest,
    ) -> Result<(), DbError> {
        self.backend.settle_token_request(settlement).await
    }
    pub async fn release_token_request(
        &self,
        reservation_id: &str,
        reason: &str,
    ) -> Result<(), DbError> {
        self.backend
            .release_token_request(reservation_id, reason)
            .await
    }
    pub async fn token_request_billing_amount(
        &self,
        request_id: &str,
    ) -> Result<Option<(bool, Decimal)>, DbError> {
        self.backend.token_request_billing_amount(request_id).await
    }
    pub async fn settle_released_token_request(
        &self,
        request_id: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        cache_hit_input_tokens: u64,
    ) -> Result<(), DbError> {
        self.backend
            .settle_released_token_request(
                request_id,
                prompt_tokens,
                completion_tokens,
                cache_hit_input_tokens,
            )
            .await
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

    // ── Teams ────────────────────────────────────────────────────────────
    pub async fn create_team(&self, team: &Team, owner_id: &str) -> Result<(), DbError> {
        self.backend.create_team(team, owner_id).await
    }
    pub async fn get_team(&self, team_id: &str) -> Result<Option<Team>, DbError> {
        self.backend.get_team(team_id).await
    }
    pub async fn list_teams_for_user(&self, user_id: &str) -> Result<Vec<Team>, DbError> {
        self.backend.list_teams_for_user(user_id).await
    }
    pub async fn list_all_teams(&self) -> Result<Vec<Team>, DbError> {
        self.backend.list_all_teams().await
    }
    pub async fn update_team(&self, team_id: &str, name: &str) -> Result<(), DbError> {
        self.backend.update_team(team_id, name).await
    }
    pub async fn delete_team(&self, team_id: &str) -> Result<(), DbError> {
        self.backend.delete_team(team_id).await
    }
    pub async fn add_team_member(
        &self,
        team_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<(), DbError> {
        self.backend.add_team_member(team_id, user_id, role).await
    }
    pub async fn remove_team_member(&self, team_id: &str, user_id: &str) -> Result<(), DbError> {
        self.backend.remove_team_member(team_id, user_id).await
    }
    pub async fn set_team_member_role(
        &self,
        team_id: &str,
        user_id: &str,
        role: &str,
    ) -> Result<(), DbError> {
        self.backend
            .set_team_member_role(team_id, user_id, role)
            .await
    }
    pub async fn list_team_members(&self, team_id: &str) -> Result<Vec<TeamMember>, DbError> {
        self.backend.list_team_members(team_id).await
    }
    pub async fn get_team_member(
        &self,
        team_id: &str,
        user_id: &str,
    ) -> Result<Option<TeamMember>, DbError> {
        self.backend.get_team_member(team_id, user_id).await
    }
    pub async fn get_team_wallet(&self, team_id: &str) -> Result<Option<(f64, f64)>, DbError> {
        self.backend.get_team_wallet(team_id).await
    }
    pub async fn all_team_members(&self) -> Result<Vec<TeamMember>, DbError> {
        self.backend.all_team_members().await
    }
    pub async fn add_team_wallet_balance(&self, team_id: &str, amount: f64) -> Result<(), DbError> {
        self.backend.add_team_wallet_balance(team_id, amount).await
    }
    pub async fn list_team_wallet_transactions(
        &self,
        team_id: &str,
        page: usize,
        size: usize,
    ) -> Result<(Vec<WalletTransactionRow>, usize), DbError> {
        self.backend
            .list_team_wallet_transactions(team_id, page, size)
            .await
    }
    pub async fn list_team_api_keys(&self, team_id: &str) -> Result<Vec<ApiKey>, DbError> {
        self.backend.list_team_api_keys(team_id).await
    }
    pub async fn list_team_rules(&self, team_id: &str) -> Result<Vec<RoutingRule>, DbError> {
        self.backend.list_team_rules(team_id).await
    }
}
