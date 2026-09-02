use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sqlx_core::{
    query::query, query_as::query_as, query_builder::QueryBuilder, query_scalar::query_scalar,
    raw_sql::raw_sql, row::Row,
};
use sqlx_postgres::{PgPool, PgRow, Postgres};

use crate::config::types::GatewayRuntimeConfig;
use crate::db::backend::*;
use crate::db::{AnnouncementRow, DbError, ManagementApiKey, RechargeKeyRow, WalletTransactionRow};
use crate::domain::billing_group::{BillingGroupRow, BillingPaymentMode};
use crate::domain::channel::{Channel, Endpoint};
use crate::domain::gateway::GatewayRoute;
use crate::domain::model::{Model, ModelChannel, Pricing};
use crate::domain::moderation::ContentFilterRule;
use crate::domain::routing::RoutingRule;
use crate::domain::sso::SsoConfigRow;
use crate::domain::team::{Team, TeamMember};
use crate::domain::token_package::{
    settle_usage as calculate_settlement, PriceSnapshot, TokenUsage,
};
use crate::domain::usage::UsageRecord;
use crate::domain::user::{ApiKey, User, USER_STATUS_ACTIVE, USER_STATUS_SUSPENDED};

pub struct PgBackend {
    pool: PgPool,
}

fn map_announcement_row(row: &PgRow) -> AnnouncementRow {
    AnnouncementRow {
        id: row.try_get::<String, _>(0).unwrap_or_default(),
        title: row.try_get::<String, _>(1).unwrap_or_default(),
        content: row.try_get::<String, _>(2).unwrap_or_default(),
        created_by: row.try_get::<String, _>(3).unwrap_or_default(),
        created_at: row.try_get::<String, _>(4).unwrap_or_default(),
        updated_at: row.try_get::<String, _>(5).unwrap_or_default(),
        published: row.try_get::<bool, _>(6).unwrap_or(false),
    }
}

fn map_billing_group_row(row: &PgRow) -> Result<BillingGroupRow, DbError> {
    Ok(BillingGroupRow {
        id: row.try_get(0)?,
        name: row.try_get(1)?,
        payment_mode: row.try_get::<String, _>(2)?.parse().map_err(DbError)?,
        status: row.try_get(3)?,
        is_default: row.try_get(4)?,
        created_by: row.try_get(5)?,
        created_at: row.try_get(6)?,
        updated_at: row.try_get(7)?,
        deleted_at: row.try_get(8).ok(),
        deleted_by: row.try_get(9).ok(),
    })
}

impl PgBackend {
    pub async fn new(pg_url: &str) -> Result<Self, DbError> {
        let pool = PgPool::connect(pg_url)
            .await
            .map_err(|e| DbError(format!("Failed to connect to PostgreSQL: {}", e)))?;
        Ok(Self { pool })
    }

    // ── Private helpers ──────────────────────────────────────────────────────────

    #[allow(dead_code)]
    async fn pricing_lookup(&self, model_name: &str) -> (Decimal, Decimal, Decimal, Decimal) {
        let result = query_as::<_, (f64, f64, f64, f64)>(
            "SELECT prompt_price, completion_price, cache_read_price, cache_write_price FROM models WHERE name = $1",
        )
        .bind(model_name)
        .fetch_optional(&self.pool)
        .await;

        match result {
            Ok(Some((p, c, cr, cw))) => (
                Decimal::try_from(p).unwrap_or(Decimal::ZERO),
                Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                Decimal::try_from(cr).unwrap_or(Decimal::ZERO),
                Decimal::try_from(cw).unwrap_or(Decimal::ZERO),
            ),
            _ => {
                // Fall back to pattern matching
                let rows = query_as::<_, (f64, f64, f64, f64, String)>(
                    "SELECT prompt_price, completion_price, cache_read_price, cache_write_price, model_pattern FROM models",
                )
                .fetch_all(&self.pool)
                .await;

                if let Ok(rows) = rows {
                    for (p, c, cr, cw, pattern) in rows {
                        if pattern.ends_with('*') {
                            let prefix = &pattern[..pattern.len() - 1];
                            if model_name.starts_with(prefix) {
                                return (
                                    Decimal::try_from(p).unwrap_or(Decimal::ZERO),
                                    Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                                    Decimal::try_from(cr).unwrap_or(Decimal::ZERO),
                                    Decimal::try_from(cw).unwrap_or(Decimal::ZERO),
                                );
                            }
                        }
                        if pattern == model_name {
                            return (
                                Decimal::try_from(p).unwrap_or(Decimal::ZERO),
                                Decimal::try_from(c).unwrap_or(Decimal::ZERO),
                                Decimal::try_from(cr).unwrap_or(Decimal::ZERO),
                                Decimal::try_from(cw).unwrap_or(Decimal::ZERO),
                            );
                        }
                    }
                }
                (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO)
            }
        }
    }

    fn map_user_row(row: &PgRow, idx: &mut usize) -> User {
        let id: String = row.get(*idx);
        *idx += 1;
        let name: String = row.get(*idx);
        *idx += 1;
        let rpm: Option<i64> = row.get(*idx);
        *idx += 1;
        let tpm: Option<i64> = row.get(*idx);
        *idx += 1;
        let timezone: Option<String> = row.get(*idx);
        *idx += 1;
        let token_version: i64 = row.get(*idx);
        *idx += 1;
        let role_val: Option<String> = row.get(*idx);
        *idx += 1;
        let concurrency_val: i64 = row.get(*idx);
        *idx += 1;
        let currency: String = row.get(*idx);
        *idx += 1;
        let status: Option<String> = row.get(*idx);
        *idx += 1;
        let suspended_at: Option<String> = row.get(*idx);
        *idx += 1;
        User {
            id,
            name,
            password_hash: None,
            rate_limits: {
                let rpm = rpm.map(|v| v as u64);
                let tpm = tpm.map(|v| v as u64);
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: timezone.unwrap_or_default(),
            token_version,
            role: role_val.unwrap_or_default(),
            concurrency_limit: concurrency_val as u32,
            currency,
            status: status.unwrap_or_else(|| "active".to_string()),
            suspended_at,
        }
    }

    fn map_user_with_pw_row(row: &PgRow, idx: &mut usize) -> User {
        let id: String = row.get(*idx);
        *idx += 1;
        let name: String = row.get(*idx);
        *idx += 1;
        let password_hash: String = row.get(*idx);
        *idx += 1;
        let rpm: Option<i64> = row.get(*idx);
        *idx += 1;
        let tpm: Option<i64> = row.get(*idx);
        *idx += 1;
        let timezone: Option<String> = row.get(*idx);
        *idx += 1;
        let token_version: i64 = row.get(*idx);
        *idx += 1;
        let role_val: Option<String> = row.get(*idx);
        *idx += 1;
        let concurrency_val: i64 = row.get(*idx);
        *idx += 1;
        let currency: String = row.get(*idx);
        *idx += 1;
        let status: Option<String> = row.get(*idx);
        *idx += 1;
        let suspended_at: Option<String> = row.get(*idx);
        *idx += 1;
        User {
            id,
            name,
            password_hash: Some(password_hash),
            rate_limits: {
                let rpm = rpm.map(|v| v as u64);
                let tpm = tpm.map(|v| v as u64);
                if rpm.is_some() || tpm.is_some() {
                    Some(crate::domain::user::RateLimit { rpm, tpm })
                } else {
                    None
                }
            },
            timezone: timezone.unwrap_or_default(),
            token_version,
            role: role_val.unwrap_or_default(),
            concurrency_limit: concurrency_val as u32,
            currency,
            status: status.unwrap_or_else(|| "active".to_string()),
            suspended_at,
        }
    }

    #[allow(dead_code)]
    fn map_usage_record(row: &PgRow, idx: &mut usize) -> UsageRecord {
        let timestamp: String = row.get(*idx);
        *idx += 1;
        let request_id: String = row.get(*idx);
        *idx += 1;
        let user_id: String = row.get(*idx);
        *idx += 1;
        let user_name: String = row.get(*idx);
        *idx += 1;
        let channel_id: String = row.get(*idx);
        *idx += 1;
        let model: String = row.get(*idx);
        *idx += 1;
        let prompt_tokens: i64 = row.get(*idx);
        *idx += 1;
        let completion_tokens: i64 = row.get(*idx);
        *idx += 1;
        let total_tokens: i64 = row.get(*idx);
        *idx += 1;
        let latency_ms: i64 = row.get(*idx);
        *idx += 1;
        let status_code: i32 = row.get(*idx);
        *idx += 1;
        let success: bool = row.get(*idx);
        *idx += 1;
        let api_key_name: Option<String> = row.get(*idx);
        *idx += 1;
        let api_format: String = row.get(*idx);
        *idx += 1;
        let stream: bool = row.get(*idx);
        *idx += 1;
        let cache_hit_input_tokens: i64 = row.get(*idx);
        *idx += 1;
        let cache_write_tokens: i64 = row.get(*idx);
        *idx += 1;
        let prompt_price: f64 = row.get(*idx);
        *idx += 1;
        let completion_price: f64 = row.get(*idx);
        *idx += 1;
        let cache_read_price: f64 = row.get(*idx);
        *idx += 1;
        let client_ip: Option<String> = row.get(*idx);
        *idx += 1;
        let original_model: String = if *idx < row.len() {
            row.get(*idx)
        } else {
            String::new()
        };
        *idx += 1;
        UsageRecord {
            timestamp,
            request_id,
            user_id,
            user_name,
            channel_id,
            model,
            prompt_tokens: prompt_tokens as u64,
            completion_tokens: completion_tokens as u64,
            total_tokens: total_tokens as u64,
            latency_ms: latency_ms as u64,
            status_code: status_code as u16,
            success,
            request_body: None,
            response_body: None,
            reasoning_body: None,
            api_key_name,
            api_format,
            stream,
            cache_hit_input_tokens: cache_hit_input_tokens as u64,
            cache_write_tokens: cache_write_tokens as u64,
            prompt_price: Decimal::try_from(prompt_price).unwrap_or(Decimal::ZERO),
            completion_price: Decimal::try_from(completion_price).unwrap_or(Decimal::ZERO),
            cache_read_price: Decimal::try_from(cache_read_price).unwrap_or(Decimal::ZERO),
            cache_write_price: Decimal::ZERO,
            client_ip,
            endpoint_id: None,
            endpoint_url: None,
            original_model,
            team_id: None,
            ttft_ms: None,
            account_type: None,
            billing_group_id: None,
            billing_group_name: None,
            billing_payment_mode: None,
        }
    }

    #[allow(dead_code)]
    fn map_usage_with_bodies(row: &PgRow, idx: &mut usize) -> UsageRecord {
        let timestamp: String = row.get(*idx);
        *idx += 1;
        let request_id: String = row.get(*idx);
        *idx += 1;
        let user_id: String = row.get(*idx);
        *idx += 1;
        let user_name: String = row.get(*idx);
        *idx += 1;
        let channel_id: String = row.get(*idx);
        *idx += 1;
        let model: String = row.get(*idx);
        *idx += 1;
        let prompt_tokens: i64 = row.get(*idx);
        *idx += 1;
        let completion_tokens: i64 = row.get(*idx);
        *idx += 1;
        let total_tokens: i64 = row.get(*idx);
        *idx += 1;
        let latency_ms: i64 = row.get(*idx);
        *idx += 1;
        let status_code: i32 = row.get(*idx);
        *idx += 1;
        let success: bool = row.get(*idx);
        *idx += 1;
        let request_body: Option<String> = row.get(*idx);
        *idx += 1;
        let response_body: Option<String> = row.get(*idx);
        *idx += 1;
        let reasoning_body: Option<String> = row.get(*idx);
        *idx += 1;
        let api_key_name: Option<String> = row.get(*idx);
        *idx += 1;
        let api_format: String = row.get(*idx);
        *idx += 1;
        let stream: bool = row.get(*idx);
        *idx += 1;
        let cache_hit_input_tokens: i64 = row.get(*idx);
        *idx += 1;
        let cache_write_tokens: i64 = row.get(*idx);
        *idx += 1;
        let prompt_price: f64 = row.get(*idx);
        *idx += 1;
        let completion_price: f64 = row.get(*idx);
        *idx += 1;
        let cache_read_price: f64 = row.get(*idx);
        *idx += 1;
        let client_ip: Option<String> = row.get(*idx);
        *idx += 1;
        let original_model: String = if *idx < row.len() {
            row.get(*idx)
        } else {
            String::new()
        };
        *idx += 1;
        UsageRecord {
            timestamp,
            request_id,
            user_id,
            user_name,
            channel_id,
            model,
            prompt_tokens: prompt_tokens as u64,
            completion_tokens: completion_tokens as u64,
            total_tokens: total_tokens as u64,
            latency_ms: latency_ms as u64,
            status_code: status_code as u16,
            success,
            request_body,
            response_body,
            reasoning_body,
            api_key_name,
            api_format,
            stream,
            cache_hit_input_tokens: cache_hit_input_tokens as u64,
            cache_write_tokens: cache_write_tokens as u64,
            prompt_price: Decimal::try_from(prompt_price).unwrap_or(Decimal::ZERO),
            completion_price: Decimal::try_from(completion_price).unwrap_or(Decimal::ZERO),
            cache_read_price: Decimal::try_from(cache_read_price).unwrap_or(Decimal::ZERO),
            cache_write_price: Decimal::ZERO,
            client_ip,
            endpoint_id: None,
            endpoint_url: None,
            original_model,
            team_id: None,
            ttft_ms: None,
            account_type: None,
            billing_group_id: None,
            billing_group_name: None,
            billing_payment_mode: None,
        }
    }
}

mod access;
mod billing_queries;
mod catalog;
mod migrations;
mod system;
mod teams_sso;
mod token_billing;
mod users;
mod wallet;

impl PgBackend {
    fn apply_recharge_key_filters<'a>(
        builder: &mut QueryBuilder<'a, Postgres>,
        search: Option<&str>,
        status: Option<&str>,
        user_search: Option<&str>,
        now: &'a str,
    ) {
        if let Some(s) = search.filter(|s| !s.is_empty()) {
            builder.push(" AND key LIKE ");
            builder.push_bind(format!("%{}%", s));
        }
        if let Some(u) = user_search.filter(|u| !u.is_empty()) {
            builder.push(" AND (used_by LIKE ");
            builder.push_bind(format!("%{}%", u));
            builder.push(" OR created_by LIKE ");
            builder.push_bind(format!("%{}%", u));
            builder.push(")");
        }
        match status {
            Some("active") => {
                builder.push(" AND used_by IS NULL");
                builder.push(" AND (revoked IS NULL OR revoked = false)");
                builder.push(" AND (expires_at IS NULL OR expires_at > ");
                builder.push_bind(now);
                builder.push(")");
            }
            Some("used") => {
                builder.push(" AND used_by IS NOT NULL");
            }
            Some("expired") => {
                builder.push(" AND used_by IS NULL");
                builder.push(" AND (revoked IS NULL OR revoked = false)");
                builder.push(" AND expires_at IS NOT NULL");
                builder.push(" AND expires_at < ");
                builder.push_bind(now);
            }
            Some("revoked") => {
                builder.push(" AND revoked = true");
            }
            _ => {}
        }
    }
}

/// Compute usage cost from token counts and per-1M prices.
/// Pure helper extracted from the billing path so it is unit-testable.
fn compute_cost_amount(
    prompt_tokens: u64,
    completion_tokens: u64,
    cache_hit_input_tokens: u64,
    cache_write_tokens: u64,
    prompt_price: f64,
    completion_price: f64,
    cache_read_price: f64,
    cache_write_price: f64,
) -> f64 {
    prompt_tokens as f64 / 1000000.0 * prompt_price
        + completion_tokens as f64 / 1000000.0 * completion_price
        + cache_hit_input_tokens as f64 / 1000000.0 * cache_read_price
        + cache_write_tokens as f64 / 1000000.0 * cache_write_price
}

/// Resolve which account a usage record charges.
/// Returns (account_id, account_type) where account_type is "user" or "team".
/// Personal records charge the user's wallet; team records charge the team wallet.
fn usage_account(record: &UsageRecord) -> (String, &'static str) {
    match &record.team_id {
        Some(team_id) => (team_id.clone(), "team"),
        None => (record.user_id.clone(), "user"),
    }
}

#[cfg(test)]
mod billing_tests {
    use super::{compute_cost_amount, usage_account, PgBackend};
    use crate::db::backend::*;
    use crate::domain::usage::UsageRecord;
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal::Decimal;
    use std::sync::Arc;
    use tokio::sync::Barrier;

    #[tokio::test]
    #[ignore = "requires a configured PostgreSQL integration database"]
    async fn payment_identity_replay_is_idempotent_when_enabled() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let db = PgBackend::new(&url).await.expect("connect");
        db.migrate().await.expect("migrate");
        let suffix = uuid::Uuid::new_v4().to_string();
        let user_id = format!("replay-{suffix}");
        let base = format!("replay-{suffix}");
        sqlx_core::query::query("INSERT INTO users (id,name,password_hash,balance,frozen,token_wallet_reserved,role,status) VALUES ($1,$2,'',1.0,0,0,'user','active')")
            .bind(&user_id).bind(&user_id).execute(db.pg_pool()).await.expect("user");
        for (idx, (amount, sequence, payment_type)) in [
            (Decimal::new(1, 2), 0_i64, "initial_settlement"),
            (Decimal::ZERO, 0_i64, "initial_settlement"),
            (Decimal::new(1, 2), 1_i64, "recovery"),
        ]
        .into_iter()
        .enumerate()
        {
            let reservation_id = format!("{base}-res-{idx}");
            let request_id = format!("{base}-req-{idx}");
            let receivable_id = format!("{base}-recv-{idx}");
            let now = chrono::Utc::now().to_rfc3339();
            sqlx_core::query::query("INSERT INTO token_request_reservations (id,request_id,user_id,account_type,model,state,settlement_state,actual_wallet_amount,wallet_shortfall_amount,created_at,expires_at) VALUES ($1,$2,$3,'user','replay','settlement_pending','settlement_pending',0,$4,$5,$5)")
                .bind(&reservation_id).bind(&request_id).bind(&user_id).bind(if amount.is_zero() { 0.1 } else { 0.0 }).bind(&now)
                .execute(db.pg_pool()).await.expect("reservation");
            sqlx_core::query::query("INSERT INTO token_settlement_receivables (id,reservation_id,request_id,user_id,account_type,wallet_due_amount,settled_wallet_amount,outstanding_amount,state,created_at,updated_at) VALUES ($1,$2,$3,$4,'user',$5,0,$5,'partially_settled',$6,$6)")
                .bind(&receivable_id).bind(&reservation_id).bind(&request_id).bind(&user_id).bind(if amount.is_zero() { 0.1 } else { 0.1 }).bind(&now)
                .execute(db.pg_pool()).await.expect("receivable");
            let key = format!("replay:{base}:{idx}");
            let before = db.get_wallet_balance(&user_id).await.expect("balance").0;
            let mut applied = Vec::new();
            for _ in 0..10 {
                applied.push(
                    db.apply_token_settlement_payment(
                        &receivable_id,
                        sequence,
                        payment_type,
                        &key,
                        amount,
                    )
                    .await
                    .expect("apply"),
                );
            }
            let after = db.get_wallet_balance(&user_id).await.expect("balance").0;
            let payments: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments WHERE receivable_id=$1 AND idempotency_key=$2").bind(&receivable_id).bind(&key).fetch_one(db.pg_pool()).await.expect("payments");
            let txns: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments p JOIN wallet_transactions w ON w.id=p.wallet_transaction_id WHERE p.receivable_id=$1 AND p.amount > 0").bind(&receivable_id).fetch_one(db.pg_pool()).await.expect("txns");
            assert_eq!(applied.iter().filter(|v| **v).count(), 1);
            assert_eq!(payments, 1);
            assert_eq!(after - before, -amount);
            if amount > Decimal::ZERO {
                assert_eq!(txns, 1);
            } else {
                assert_eq!(txns, 0);
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires TEST3_PAYMENT_REPLAY=1 and configured PostgreSQL"]
    async fn payment_identity_replay_test3_when_enabled() {
        assert_eq!(std::env::var("TEST3_PAYMENT_REPLAY").as_deref(), Ok("1"));
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let db = PgBackend::new(&url).await.expect("connect");
        db.migrate().await.expect("migrate");
        let user_id = "test3".to_string();
        let baseline: (f64, f64, f64) = sqlx_core::query_as::query_as(
            "SELECT balance, frozen, token_wallet_reserved FROM users WHERE id=$1",
        )
        .bind(&user_id)
        .fetch_one(db.pg_pool())
        .await
        .expect("test3 baseline");
        assert!(baseline.0 >= 0.02, "test3 must have clean replay funds");
        let base = format!("test3-payment-replay-{}", uuid::Uuid::new_v4());
        let now = chrono::Utc::now().to_rfc3339();
        let mut cases = Vec::new();
        for (idx, (amount, sequence, payment_type, due)) in [
            (
                Decimal::new(1, 2),
                0_i64,
                "initial_settlement",
                Decimal::new(1, 2),
            ),
            (
                Decimal::ZERO,
                0_i64,
                "initial_settlement",
                Decimal::new(1, 2),
            ),
            (
                Decimal::ZERO,
                0_i64,
                "initial_settlement",
                Decimal::new(1, 2),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let reservation_id = format!("{base}-res-{idx}");
            let request_id = format!("{base}-req-{idx}");
            let receivable_id = format!("{base}-recv-{idx}");
            sqlx_core::query::query("INSERT INTO token_request_reservations (id,request_id,user_id,account_type,model,state,settlement_state,actual_wallet_amount,wallet_shortfall_amount,created_at,expires_at) VALUES ($1,$2,$3,'user','gpt-5.6-luna','settlement_pending','settlement_pending',0,$4,$5,$5)")
                .bind(&reservation_id).bind(&request_id).bind(&user_id).bind(due.to_f64().unwrap()).bind(&now)
                .execute(db.pg_pool()).await.expect("reservation fixture");
            sqlx_core::query::query("INSERT INTO token_settlement_receivables (id,reservation_id,request_id,user_id,account_type,wallet_due_amount,settled_wallet_amount,outstanding_amount,state,created_at,updated_at) VALUES ($1,$2,$3,$4,'user',$5,0,$5,'partially_settled',$6,$6)")
                .bind(&receivable_id).bind(&reservation_id).bind(&request_id).bind(&user_id).bind(due.to_f64().unwrap()).bind(&now)
                .execute(db.pg_pool()).await.expect("receivable fixture");
            let key = format!("test3:{base}:{idx}:payment:{sequence}");
            let wallet_before = db
                .get_wallet_balance(&user_id)
                .await
                .expect("wallet before")
                .0;
            let settled_before: f64 = sqlx_core::query_scalar::query_scalar(
                "SELECT settled_wallet_amount FROM token_settlement_receivables WHERE id=$1",
            )
            .bind(&receivable_id)
            .fetch_one(db.pg_pool())
            .await
            .expect("settled before");
            let outstanding_before: f64 = sqlx_core::query_scalar::query_scalar(
                "SELECT outstanding_amount FROM token_settlement_receivables WHERE id=$1",
            )
            .bind(&receivable_id)
            .fetch_one(db.pg_pool())
            .await
            .expect("outstanding before");
            let mut results = Vec::new();
            for _ in 0..10 {
                results.push(
                    db.apply_token_settlement_payment(
                        &receivable_id,
                        sequence,
                        payment_type,
                        &key,
                        amount,
                    )
                    .await
                    .expect("payment application"),
                );
            }
            let wallet_after = db
                .get_wallet_balance(&user_id)
                .await
                .expect("wallet after")
                .0;
            let settled_after: f64 = sqlx_core::query_scalar::query_scalar(
                "SELECT settled_wallet_amount FROM token_settlement_receivables WHERE id=$1",
            )
            .bind(&receivable_id)
            .fetch_one(db.pg_pool())
            .await
            .expect("settled after");
            let outstanding_after: f64 = sqlx_core::query_scalar::query_scalar(
                "SELECT outstanding_amount FROM token_settlement_receivables WHERE id=$1",
            )
            .bind(&receivable_id)
            .fetch_one(db.pg_pool())
            .await
            .expect("outstanding after");
            let payment_rows: i64 = sqlx_core::query_scalar::query_scalar(
                "SELECT COUNT(*) FROM token_settlement_payments WHERE receivable_id=$1",
            )
            .bind(&receivable_id)
            .fetch_one(db.pg_pool())
            .await
            .expect("payment rows");
            let identity_rows: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments WHERE receivable_id=$1 AND idempotency_key=$2").bind(&receivable_id).bind(&key).fetch_one(db.pg_pool()).await.expect("identity rows");
            let sequence_rows: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments WHERE receivable_id=$1 AND payment_sequence=$2").bind(&receivable_id).bind(sequence).fetch_one(db.pg_pool()).await.expect("sequence rows");
            let wallet_tx_rows: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments p JOIN wallet_transactions w ON w.id=p.wallet_transaction_id WHERE p.receivable_id=$1 AND p.amount > 0").bind(&receivable_id).fetch_one(db.pg_pool()).await.expect("wallet tx rows");
            assert_eq!(results.iter().filter(|applied| **applied).count(), 1);
            assert_eq!(payment_rows, 1);
            assert_eq!(identity_rows, 1);
            assert_eq!(sequence_rows, 1);
            assert_eq!(wallet_tx_rows, i64::from(amount > Decimal::ZERO));
            assert!(
                (wallet_after.to_f64().unwrap()
                    - (wallet_before.to_f64().unwrap() - amount.to_f64().unwrap()))
                .abs()
                    < 1e-12
            );
            cases.push(serde_json::json!({
                "case": if idx == 0 { "A_initial_positive" } else if idx == 1 { "B_initial_zero" } else { "C_initial_zero_before_recovery" },
                "request_id": request_id, "reservation_id": reservation_id, "receivable_id": receivable_id,
                "payment_sequence": sequence, "payment_type": payment_type, "idempotency_key": key,
                "amount": amount.to_string(), "attempt_results": results,
                "payment_row_count": payment_rows, "idempotency_key_row_count": identity_rows,
                "sequence_row_count": sequence_rows, "wallet_transaction_row_count": wallet_tx_rows,
                "wallet_before": wallet_before.to_string(), "wallet_after_attempt_1": (wallet_before - amount).to_string(), "wallet_after_attempt_10": wallet_after.to_string(),
                "settled_before": settled_before.to_string(), "settled_after_attempt_1": (settled_before + amount.to_f64().unwrap()).to_string(), "settled_after_attempt_10": settled_after.to_string(),
                "outstanding_before": outstanding_before.to_string(), "outstanding_after_attempt_1": (outstanding_before - amount.to_f64().unwrap()).max(0.0).to_string(), "outstanding_after_attempt_10": outstanding_after.to_string()
            }));
            if idx == 2 {
                let recovery_key = format!("test3:{base}:{idx}:payment:1");
                let recovery_amount = Decimal::new(1, 2);
                let recovery_wallet_before = db
                    .get_wallet_balance(&user_id)
                    .await
                    .expect("recovery wallet before")
                    .0;
                let recovery_settled_before: f64 = sqlx_core::query_scalar::query_scalar(
                    "SELECT settled_wallet_amount FROM token_settlement_receivables WHERE id=$1",
                )
                .bind(&receivable_id)
                .fetch_one(db.pg_pool())
                .await
                .expect("recovery settled before");
                let recovery_outstanding_before: f64 = sqlx_core::query_scalar::query_scalar(
                    "SELECT outstanding_amount FROM token_settlement_receivables WHERE id=$1",
                )
                .bind(&receivable_id)
                .fetch_one(db.pg_pool())
                .await
                .expect("recovery outstanding before");
                let mut recovery_results = Vec::new();
                for _ in 0..10 {
                    recovery_results.push(
                        db.apply_token_settlement_payment(
                            &receivable_id,
                            1,
                            "recovery",
                            &recovery_key,
                            recovery_amount,
                        )
                        .await
                        .expect("recovery application"),
                    );
                }
                let recovery_wallet_after = db
                    .get_wallet_balance(&user_id)
                    .await
                    .expect("recovery wallet after")
                    .0;
                let recovery_settled_after: f64 = sqlx_core::query_scalar::query_scalar(
                    "SELECT settled_wallet_amount FROM token_settlement_receivables WHERE id=$1",
                )
                .bind(&receivable_id)
                .fetch_one(db.pg_pool())
                .await
                .expect("recovery settled after");
                let recovery_outstanding_after: f64 = sqlx_core::query_scalar::query_scalar(
                    "SELECT outstanding_amount FROM token_settlement_receivables WHERE id=$1",
                )
                .bind(&receivable_id)
                .fetch_one(db.pg_pool())
                .await
                .expect("recovery outstanding after");
                let recovery_payments: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments WHERE receivable_id=$1 AND payment_sequence=1").bind(&receivable_id).fetch_one(db.pg_pool()).await.expect("recovery payments");
                let recovery_txs: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments p JOIN wallet_transactions w ON w.id=p.wallet_transaction_id WHERE p.receivable_id=$1 AND p.payment_sequence=1").bind(&receivable_id).fetch_one(db.pg_pool()).await.expect("recovery txs");
                assert_eq!(
                    recovery_results.iter().filter(|applied| **applied).count(),
                    1
                );
                assert_eq!(recovery_payments, 1);
                assert_eq!(recovery_txs, 1);
                cases.push(serde_json::json!({
                    "case": "C_recovery_positive", "payment_sequence": 1, "payment_type": "recovery", "idempotency_key": recovery_key, "amount": recovery_amount.to_string(), "attempt_results": recovery_results,
                    "payment_row_count": recovery_payments, "wallet_transaction_row_count": recovery_txs,
                    "wallet_before": recovery_wallet_before.to_string(), "wallet_after_attempt_1": (recovery_wallet_before - recovery_amount).to_string(), "wallet_after_attempt_10": recovery_wallet_after.to_string(),
                    "settled_before": recovery_settled_before.to_string(), "settled_after_attempt_1": (recovery_settled_before + recovery_amount.to_f64().unwrap()).to_string(), "settled_after_attempt_10": recovery_settled_after.to_string(),
                    "outstanding_before": recovery_outstanding_before.to_string(), "outstanding_after_attempt_1": (recovery_outstanding_before - recovery_amount.to_f64().unwrap()).max(0.0).to_string(), "outstanding_after_attempt_10": recovery_outstanding_after.to_string()
                }));
            }
        }
        println!(
            "TEST3_PAYMENT_REPLAY={}",
            serde_json::to_string_pretty(&cases).expect("json")
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST1_PAYMENT_REPLAY_CONCURRENT=1 and configured PostgreSQL"]
    async fn payment_identity_replay_test1_concurrent_when_enabled() {
        assert_eq!(
            std::env::var("TEST1_PAYMENT_REPLAY_CONCURRENT").as_deref(),
            Ok("1")
        );
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let db = Arc::new(PgBackend::new(&url).await.expect("connect"));
        db.migrate().await.expect("migrate");
        let user_id = "test1".to_string();
        let baseline: (f64, f64, f64) = sqlx_core::query_as::query_as(
            "SELECT balance, frozen, token_wallet_reserved FROM users WHERE id=$1",
        )
        .bind(&user_id)
        .fetch_one(db.pg_pool())
        .await
        .expect("test1 baseline");
        assert!(baseline.0 > 0.1, "test1 must have integration funds");
        let base = format!("test1-concurrent-{}", uuid::Uuid::new_v4());
        let now = chrono::Utc::now().to_rfc3339();
        let cases = [
            (
                "A_initial_positive",
                Decimal::new(1, 2),
                0_i64,
                "initial_settlement",
                Decimal::new(1, 2),
            ),
            (
                "B_initial_zero",
                Decimal::ZERO,
                0_i64,
                "initial_settlement",
                Decimal::new(1, 2),
            ),
            (
                "C_recovery_positive",
                Decimal::new(1, 2),
                1_i64,
                "recovery",
                Decimal::new(1, 2),
            ),
        ];
        for (case_name, amount, sequence, payment_type, due) in cases {
            let reservation_id = format!("{base}-{case_name}-res");
            let request_id = format!("{base}-{case_name}-req");
            let receivable_id = format!("{base}-{case_name}-recv");
            sqlx_core::query::query("INSERT INTO token_request_reservations (id,request_id,user_id,account_type,model,state,settlement_state,actual_wallet_amount,wallet_shortfall_amount,created_at,expires_at) VALUES ($1,$2,$3,'user','gpt-5.6-luna','settlement_pending','settlement_pending',0,$4,$5,$5)")
                .bind(&reservation_id).bind(&request_id).bind(&user_id).bind(due.to_f64().unwrap()).bind(&now).execute(db.pg_pool()).await.expect("reservation");
            sqlx_core::query::query("INSERT INTO token_settlement_receivables (id,reservation_id,request_id,user_id,account_type,wallet_due_amount,settled_wallet_amount,outstanding_amount,state,created_at,updated_at) VALUES ($1,$2,$3,$4,'user',$5,0,$5,'partially_settled',$6,$6)")
                .bind(&receivable_id).bind(&reservation_id).bind(&request_id).bind(&user_id).bind(due.to_f64().unwrap()).bind(&now).execute(db.pg_pool()).await.expect("receivable");
            let wallet_before = db
                .get_wallet_balance(&user_id)
                .await
                .expect("wallet before")
                .0;
            let key = format!("test1-concurrent:{base}:{case_name}:payment:{sequence}");
            let barrier = Arc::new(Barrier::new(10));
            let mut tasks = Vec::new();
            for _ in 0..10 {
                let db = db.clone();
                let barrier = barrier.clone();
                let receivable_id = receivable_id.clone();
                let key = key.clone();
                let payment_type = payment_type.to_string();
                tasks.push(tokio::spawn(async move {
                    barrier.wait().await;
                    db.apply_token_settlement_payment(
                        &receivable_id,
                        sequence,
                        &payment_type,
                        &key,
                        amount,
                    )
                    .await
                }));
            }
            let mut results = Vec::new();
            for task in tasks {
                results.push(task.await.expect("join").expect("payment application"));
            }
            let wallet_after = db
                .get_wallet_balance(&user_id)
                .await
                .expect("wallet after")
                .0;
            let payments: i64 = sqlx_core::query_scalar::query_scalar(
                "SELECT COUNT(*) FROM token_settlement_payments WHERE receivable_id=$1",
            )
            .bind(&receivable_id)
            .fetch_one(db.pg_pool())
            .await
            .expect("payments");
            let identity_rows: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments WHERE receivable_id=$1 AND idempotency_key=$2").bind(&receivable_id).bind(&key).fetch_one(db.pg_pool()).await.expect("identity");
            let sequence_rows: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments WHERE receivable_id=$1 AND payment_sequence=$2").bind(&receivable_id).bind(sequence).fetch_one(db.pg_pool()).await.expect("sequence");
            let wallet_txs: i64 = sqlx_core::query_scalar::query_scalar("SELECT COUNT(*) FROM token_settlement_payments p JOIN wallet_transactions w ON w.id=p.wallet_transaction_id WHERE p.receivable_id=$1 AND p.amount > 0").bind(&receivable_id).fetch_one(db.pg_pool()).await.expect("wallet tx");
            let settled: f64 = sqlx_core::query_scalar::query_scalar(
                "SELECT settled_wallet_amount FROM token_settlement_receivables WHERE id=$1",
            )
            .bind(&receivable_id)
            .fetch_one(db.pg_pool())
            .await
            .expect("settled");
            let outstanding: f64 = sqlx_core::query_scalar::query_scalar(
                "SELECT outstanding_amount FROM token_settlement_receivables WHERE id=$1",
            )
            .bind(&receivable_id)
            .fetch_one(db.pg_pool())
            .await
            .expect("outstanding");
            assert_eq!(results.iter().filter(|result| **result).count(), 1);
            assert_eq!(payments, 1);
            assert_eq!(identity_rows, 1);
            assert_eq!(sequence_rows, 1);
            assert_eq!(wallet_txs, i64::from(amount > Decimal::ZERO));
            assert!(
                (wallet_after.to_f64().unwrap()
                    - (wallet_before.to_f64().unwrap() - amount.to_f64().unwrap()))
                .abs()
                    < 1e-12
            );
            println!("CONCURRENT_CASE={} request_id={} reservation_id={} receivable_id={} payment_sequence={} payment_type={} idempotency_key={} amount={} results={:?} payment_rows={} identity_rows={} sequence_rows={} wallet_tx_rows={} wallet_before={} wallet_after={} settled={} outstanding={}", case_name, request_id, reservation_id, receivable_id, sequence, payment_type, key, amount, results, payments, identity_rows, sequence_rows, wallet_txs, wallet_before, wallet_after, settled, outstanding);
        }
        let ledger = sqlx_core::query_as::query_as::<_, (i64, f64, i64, f64, f64, f64)>("SELECT COUNT(*), COALESCE(SUM(amount) FILTER (WHERE type='recharge' AND status='completed'),0), COUNT(*) FILTER (WHERE type='deduction' AND status='completed'), COALESCE(SUM(ABS(amount)) FILTER (WHERE type='deduction' AND status='completed'),0), COALESCE(SUM(amount) FILTER (WHERE type NOT IN ('recharge','deduction') AND status='completed'),0), (SELECT balance FROM users WHERE id=$1) FROM wallet_transactions WHERE user_id=$1").bind(&user_id).fetch_one(db.pg_pool()).await.expect("ledger");
        println!("CONCURRENT_LEDGER user_id={} tx_count={} recharge={} deductions={} adjustments={} balance={}", user_id, ledger.0, ledger.1, ledger.2, ledger.3, ledger.4);
    }

    fn record(user_id: &str, team_id: Option<&str>) -> UsageRecord {
        let mut r = UsageRecord {
            timestamp: String::new(),
            request_id: String::new(),
            user_id: user_id.to_string(),
            user_name: String::new(),
            channel_id: String::new(),
            model: String::new(),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            latency_ms: 0,
            status_code: 0,
            success: true,
            request_body: None,
            response_body: None,
            reasoning_body: None,
            api_key_name: None,
            api_format: String::new(),
            stream: false,
            cache_hit_input_tokens: 0,
            cache_write_tokens: 0,
            prompt_price: rust_decimal::Decimal::ZERO,
            completion_price: rust_decimal::Decimal::ZERO,
            cache_read_price: rust_decimal::Decimal::ZERO,
            cache_write_price: rust_decimal::Decimal::ZERO,
            client_ip: None,
            endpoint_id: None,
            endpoint_url: None,
            original_model: String::new(),
            team_id: team_id.map(|s| s.to_string()),
            ttft_ms: None,
            account_type: None,
            billing_group_id: None,
            billing_group_name: None,
            billing_payment_mode: None,
        };
        r.prompt_tokens = 1_000_000; // $1 at $1/1M
        r.prompt_price = rust_decimal::Decimal::ONE;
        r.completion_tokens = 2_000_000;
        r.completion_price = rust_decimal::Decimal::from(2);
        r.cache_hit_input_tokens = 500_000;
        r.cache_read_price = rust_decimal::Decimal::from(1);
        r.total_tokens = r.prompt_tokens + r.cache_hit_input_tokens + r.completion_tokens;
        r
    }

    #[test]
    fn cost_amount_matches_tokens_times_prices() {
        let r = record("user-1", None);
        // 1M*$1 + 2M*$2 + 0.5M*$1 = 1 + 4 + 0.5
        let cost = compute_cost_amount(
            r.prompt_tokens,
            r.completion_tokens,
            r.cache_hit_input_tokens,
            r.cache_write_tokens,
            1.0,
            2.0,
            1.0,
            0.0,
        );
        assert!((cost - 5.5).abs() < 1e-9, "expected 5.5, got {}", cost);
    }

    #[test]
    fn cache_write_price_is_included() {
        let cost = compute_cost_amount(0, 0, 0, 1_000_000, 0.0, 0.0, 0.0, 3.5);
        assert!((cost - 3.5).abs() < 1e-9, "expected 3.5, got {}", cost);
    }

    #[test]
    fn zero_cost_when_no_tokens() {
        let cost = compute_cost_amount(0, 0, 0, 0, 1.0, 1.0, 1.0, 0.0);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn personal_record_charges_user_wallet() {
        let r = record("user-1", None);
        let (account, ty) = usage_account(&r);
        assert_eq!(account, "user-1");
        assert_eq!(ty, "user");
    }

    #[test]
    fn team_record_charges_team_wallet() {
        let r = record("user-1", Some("team-9"));
        let (account, ty) = usage_account(&r);
        assert_eq!(account, "team-9");
        assert_eq!(ty, "team");
    }
}
