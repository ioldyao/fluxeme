pub mod backend;
pub mod pg_backend;
pub mod sqlite_backend;

use crate::config::types::GatewayRuntimeConfig;
use crate::db::backend::DbBackend;
use crate::domain::channel::{Channel, Endpoint};
use crate::domain::model::{Model, Pricing};
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

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        Self(e.to_string())
    }
}

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        Self(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct WalletTransactionRow {
    pub id: String,
    pub user_id: String,
    pub tx_type: String,
    pub amount: f64,
    pub balance_before: f64,
    pub balance_after: f64,
    pub method: String,
    pub status: String,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RechargeKeyRow {
    pub key: String,
    pub amount: f64,
    pub used_by: Option<String>,
    pub used_at: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked: bool,
}

pub struct Database {
    pub backend: Box<dyn DbBackend>,
}

impl Database {
    pub async fn new(db_type: &str, path: &str, pg_url: &str) -> Self {
        match db_type {
            "sqlite" => Self {
                backend: Box::new(
                    sqlite_backend::SqliteBackend::new(path)
                        .expect("Failed to create SQLite backend"),
                ),
            },
            _ => {
                let backend = pg_backend::PgBackend::new(pg_url)
                    .await
                    .expect("Failed to create PostgreSQL backend");
                Self {
                    backend: Box::new(backend),
                }
            }
        }
    }

    // ── Migration ────────────────────────────────────────────────────────
    pub async fn migrate(&self) -> Result<(), DbError> {
        self.backend.migrate().await
    }

    // ── Users ────────────────────────────────────────────────────────────
    pub async fn list_users(&self) -> Result<Vec<User>, DbError> {
        self.backend.list_users().await
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
    pub async fn update_user(&self, user: &User) -> Result<(), DbError> {
        self.backend.update_user(user).await
    }
    pub async fn delete_user(&self, id: &str) -> Result<(), DbError> {
        self.backend.delete_user(id).await
    }
    pub async fn count_admins(&self) -> Result<i64, DbError> {
        self.backend.count_admins().await
    }
    pub async fn get_user_timezone(&self, id: &str) -> Result<String, DbError> {
        self.backend.get_user_timezone(id).await
    }
    pub async fn update_user_timezone(&self, id: &str, timezone: &str) -> Result<(), DbError> {
        self.backend.update_user_timezone(id, timezone).await
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
        self.backend.set_model_context_length(id, context_length).await
    }

    // ── Subscriptions ────────────────────────────────────────────────────
    pub async fn subscribe_user(&self, user_id: &str, model_id: &str) -> Result<(), DbError> {
        self.backend.subscribe_user(user_id, model_id).await
    }
    pub async fn unsubscribe_user(&self, user_id: &str, model_id: &str) -> Result<(), DbError> {
        self.backend.unsubscribe_user(user_id, model_id).await
    }
    pub async fn delete_subscriptions_by_model(&self, model_id: &str) -> Result<(), DbError> {
        self.backend.delete_subscriptions_by_model(model_id).await
    }
    pub async fn list_subscribed_model_ids(&self, user_id: &str) -> Result<Vec<String>, DbError> {
        self.backend.list_subscribed_model_ids(user_id).await
    }
    pub async fn list_subscriptions(&self, user_id: &str) -> Result<Vec<Model>, DbError> {
        self.backend.list_subscriptions(user_id).await
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
    pub async fn delete_rule(&self, name: &str) -> Result<(), DbError> {
        self.backend.delete_rule(name).await
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
    pub async fn get_usage_detail(
        &self,
        request_id: &str,
    ) -> Result<Option<UsageRecord>, DbError> {
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

    // ── Billing / Period ─────────────────────────────────────────────────
    pub async fn period_summary(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<(f64, u64, u64), DbError> {
        self.backend.period_summary(year, month, user_id).await
    }
    pub async fn period_model_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, f64)>, DbError> {
        self.backend.period_model_breakdown(year, month, user_id).await
    }
    pub async fn period_channel_breakdown(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, f64)>, DbError> {
        self.backend.period_channel_breakdown(year, month, user_id).await
    }
    pub async fn daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<Vec<(String, f64, u64)>, DbError> {
        self.backend.daily_deductions(year, month, user_id).await
    }
    pub async fn count_daily_deductions(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
    ) -> Result<usize, DbError> {
        self.backend.count_daily_deductions(year, month, user_id).await
    }
    pub async fn daily_deductions_paginated(
        &self,
        year: i32,
        month: u32,
        user_id: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, f64, u64)>, DbError> {
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
    pub async fn period_summary_all(&self) -> Result<Vec<(String, f64, u64, u64)>, DbError> {
        self.backend.period_summary_all().await
    }
    pub async fn period_summary_for_user(&self, user_id: &str) -> Result<Vec<(String, f64, u64, u64)>, DbError> {
        self.backend.period_summary_for_user(user_id).await
    }
    pub async fn lookup_model_pricing(&self, model_name: &str) -> Result<(f64, f64), DbError> {
        self.backend.lookup_model_pricing(model_name).await
    }

    // ── Wallet ───────────────────────────────────────────────────────────
    pub async fn get_wallet_balance(&self, user_id: &str) -> Result<(f64, f64), DbError> {
        self.backend.get_wallet_balance(user_id).await
    }
    pub async fn update_wallet_balance(&self, user_id: &str, balance: f64) -> Result<(), DbError> {
        self.backend.update_wallet_balance(user_id, balance).await
    }
    pub async fn add_wallet_transaction(
        &self,
        id: &str,
        user_id: &str,
        tx_type: &str,
        amount: f64,
        balance_before: f64,
        balance_after: f64,
        method: &str,
        status: &str,
        note: &str,
    ) -> Result<(), DbError> {
        self.backend
            .add_wallet_transaction(
                id, user_id, tx_type, amount, balance_before, balance_after, method, status, note,
            )
            .await
    }
    pub async fn get_wallet_transactions(
        &self,
        user_id: &str,
        page: usize,
        size: usize,
    ) -> Result<Vec<WalletTransactionRow>, DbError> {
        self.backend.get_wallet_transactions(user_id, page, size).await
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
    pub async fn get_total_consumed(&self, user_id: &str) -> Result<f64, DbError> {
        self.backend.get_total_consumed(user_id).await
    }
    pub async fn get_total_recharged(&self, user_id: &str) -> Result<f64, DbError> {
        self.backend.get_total_recharged(user_id).await
    }
    pub async fn get_wallet_estimated_days(&self, user_id: &str) -> Result<Option<f64>, DbError> {
        self.backend.get_wallet_estimated_days(user_id).await
    }

    // ── Recharge Keys ────────────────────────────────────────────────────
    pub async fn create_recharge_key(
        &self,
        key: &str,
        amount: f64,
        created_by: &str,
        expires_at: Option<&str>,
    ) -> Result<(), DbError> {
        self.backend
            .create_recharge_key(key, amount, created_by, expires_at)
            .await
    }
    pub async fn redeem_recharge_key(&self, key: &str, user_id: &str) -> Result<f64, DbError> {
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
        self.backend.list_recharge_keys_paginated(limit, offset).await
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
    pub async fn set_gateway_config(
        &self,
        config: &GatewayRuntimeConfig,
    ) -> Result<(), DbError> {
        self.backend.set_gateway_config(config).await
    }
    pub async fn get_balances_page(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, f64, f64)>, DbError> {
        self.backend.get_balances_page(limit, offset).await
    }

    // ── Batch Operations ────────────────────────────────────────────────
    pub async fn batch_insert_usage_with_billing(
        &self,
        batch: &[UsageRecord],
        billing_enabled: bool,
    ) -> Result<Vec<(String, f64, f64)>, DbError> {
        self.backend
            .batch_insert_usage_with_billing(batch, billing_enabled)
            .await
    }
}
