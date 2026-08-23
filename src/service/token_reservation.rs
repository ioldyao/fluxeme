use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use rust_decimal::Decimal;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::task;

use crate::db::Database;
use crate::domain::token_package::{
    settle_usage as calculate_settlement, PriceSnapshot, TokenReservationHandle,
    TokenReservationRequest, TokenSettlementRequest, TokenUsage,
};

#[derive(Clone)]
pub struct ReservationFinalizer {
    db: Arc<Database>,
    handle: TokenReservationHandle,
    finalized: Arc<AtomicBool>,
}

impl ReservationFinalizer {
    pub fn new(db: Arc<Database>, handle: TokenReservationHandle) -> Self {
        Self {
            db,
            handle,
            finalized: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn settle_usage(
        &self,
        prompt_tokens: u64,
        completion_tokens: u64,
        cache_hit_input_tokens: u64,
        cache_write_tokens: u64,
        success: bool,
        reason: &str,
    ) {
        let breakdown = calculate_settlement(
            TokenUsage {
                prompt_tokens,
                completion_tokens,
                cache_hit_input_tokens,
                cache_write_tokens,
            },
            cache_write_tokens,
            PriceSnapshot {
                prompt: self.handle.prompt_price,
                completion: self.handle.completion_price,
                cache_read: self.handle.cache_read_price,
                cache_write: self.handle.cache_write_price,
            },
            self.handle.accounting_mode,
            self.handle.input_factor,
            self.handle.output_factor,
            self.handle.cache_factor,
            self.handle.reserved_package_units,
            self.handle.billing_payment_mode,
        );
        self.settle(
            breakdown.actual_units,
            breakdown.wallet_amount,
            prompt_tokens,
            completion_tokens,
            cache_hit_input_tokens,
            cache_write_tokens,
            if success { 200 } else { 502 },
            success,
            reason,
        );
    }

    fn settle(
        &self,
        actual_units: u64,
        wallet_amount: Decimal,
        prompt_tokens: u64,
        completion_tokens: u64,
        cache_hit_input_tokens: u64,
        cache_write_tokens: u64,
        status_code: u16,
        success: bool,
        reason: &str,
    ) {
        if self.finalized.swap(true, Ordering::AcqRel) {
            return;
        }
        let db = self.db.clone();
        let reservation_id = self.handle.reservation_id.clone();
        let finalized = self.finalized.clone();
        let reason = reason.to_string();
        task::spawn(async move {
            let result = db
                .settle_token_request(&TokenSettlementRequest {
                    reservation_id,
                    actual_prompt_tokens: prompt_tokens,
                    actual_completion_tokens: completion_tokens,
                    actual_cache_hit_input_tokens: cache_hit_input_tokens,
                    actual_cache_write_tokens: cache_write_tokens,
                    actual_package_units: actual_units,
                    actual_wallet_amount: wallet_amount,
                    status_code,
                    success,
                    reason,
                })
                .await;
            if let Err(error) = result {
                tracing::error!(%error, "token reservation settlement failed");
                // Keep the guard retryable when PostgreSQL is temporarily
                // unavailable. The database state machine remains the final
                // idempotency boundary.
                finalized.store(false, Ordering::Release);
            }
        });
    }

    pub fn release_partial(
        &self,
        prompt_tokens: u64,
        completion_tokens: u64,
        cache_hit_input_tokens: u64,
        cache_write_tokens: u64,
        reason: &str,
    ) {
        let breakdown = calculate_settlement(
            TokenUsage {
                prompt_tokens,
                completion_tokens,
                cache_hit_input_tokens,
                cache_write_tokens,
            },
            cache_write_tokens,
            PriceSnapshot {
                prompt: self.handle.prompt_price,
                completion: self.handle.completion_price,
                cache_read: self.handle.cache_read_price,
                cache_write: self.handle.cache_write_price,
            },
            self.handle.accounting_mode,
            self.handle.input_factor,
            self.handle.output_factor,
            self.handle.cache_factor,
            self.handle.reserved_package_units,
            self.handle.billing_payment_mode,
        );
        self.settle(
            breakdown.actual_units,
            breakdown.wallet_amount,
            prompt_tokens,
            completion_tokens,
            cache_hit_input_tokens,
            cache_write_tokens,
            499,
            false,
            reason,
        );
    }

    /// Finalize a request with no provider usage.
    ///
    /// This releases the authorization hold and persists an explicit actual
    /// priced cost of zero. Keeping this on the settlement path prevents the
    /// reservation estimate from leaking into billing_events as actual cost.
    pub fn release(&self, reason: &str) {
        self.settle_usage(0, 0, 0, 0, false, reason);
    }
}

pub fn max_output_tokens(body: &Value) -> u64 {
    ["max_completion_tokens", "max_output_tokens", "max_tokens"]
        .into_iter()
        .find_map(|key| body.get(key).and_then(Value::as_u64))
        .unwrap_or(0)
}

pub fn estimate_input_tokens(body: &Value, anthropic: bool) -> u64 {
    let serialized = if anthropic {
        body.get("messages")
            .map(|v| v.to_string())
            .unwrap_or_default()
    } else {
        body.to_string()
    };
    (serialized.len() as u64 / 4).max(1)
}

pub fn estimated_cost(
    prompt_tokens: u64,
    completion_tokens: u64,
    cache_hit_input_tokens: u64,
    cache_write_tokens: u64,
    pricing: (Decimal, Decimal, Decimal, Decimal),
) -> Decimal {
    let (prompt, completion, cache_read, cache_write) = pricing;
    (Decimal::from(prompt_tokens) / Decimal::from(1_000_000u64)) * prompt
        + (Decimal::from(completion_tokens) / Decimal::from(1_000_000u64)) * completion
        + (Decimal::from(cache_hit_input_tokens) / Decimal::from(1_000_000u64)) * cache_read
        + (Decimal::from(cache_write_tokens) / Decimal::from(1_000_000u64)) * cache_write
}

/// Periodically recovers receivables that have an outstanding wallet balance.
pub async fn run_receivable_recovery(db: Arc<Database>) {
    let worker_id = format!("gateway-{}", uuid::Uuid::new_v4());
    tracing::info!(%worker_id, interval_secs = 10, "token settlement recovery worker started");
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        tracing::debug!(%worker_id, "token settlement recovery worker tick");
        match db
            .recover_token_settlement_receivables(100, &worker_id)
            .await
        {
            Ok(recovered) => {
                tracing::info!(
                    recovered,
                    "token settlement receivables recovery pass completed"
                );
            }
            Err(error) => tracing::warn!(%error, "token settlement receivable recovery failed"),
        }
    }
}

/// Periodically releases reservations that were never finalized before their
/// expiry. The database state transition is the idempotency boundary, so this
/// loop is safe to run on every gateway instance.
pub async fn run_expiry_reclaimer(db: Arc<Database>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        interval.tick().await;
        match db.reclaim_expired_token_reservations(100).await {
            Ok(reclaimed) if reclaimed > 0 => {
                tracing::info!(reclaimed, "expired token reservations reclaimed");
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "expired token reservation reclaim failed"),
        }
    }
}

pub async fn reserve(
    db: Arc<Database>,
    request_id: &str,
    user_id: &str,
    user_name: &str,
    api_key_name: &str,
    team_id: Option<&str>,
    billing_group_id: &str,
    billing_group_name: &str,
    billing_payment_mode: crate::domain::billing_group::BillingPaymentMode,
    model: &str,
    body: &Value,
    anthropic: bool,
    expires_at: &str,
) -> Result<TokenReservationHandle, crate::db::DbError> {
    let prompt = estimate_input_tokens(body, anthropic);
    let completion = max_output_tokens(body);
    let fingerprint_input = format!("{}:{}:{}:{}", user_id, api_key_name, model, body);
    let request_fingerprint = hex::encode(Sha256::digest(fingerprint_input.as_bytes()));
    let pricing = db.lookup_model_pricing(model).await?;
    let resolved_billing_group_name = if billing_group_name.is_empty() {
        db.get_billing_group(billing_group_id)
            .await?
            .map(|group| group.name)
            .unwrap_or_default()
    } else {
        billing_group_name.to_string()
    };
    db.reserve_token_request(&TokenReservationRequest {
        request_id: request_id.to_string(),
        request_fingerprint,
        user_id: user_id.to_string(),
        user_name: user_name.to_string(),
        api_key_name: api_key_name.to_string(),
        team_id: team_id.map(str::to_string),
        model: model.to_string(),
        prompt_tokens: prompt,
        completion_tokens: completion,
        cache_hit_input_tokens: 0,
        estimated_wallet_amount: estimated_cost(prompt, completion, 0, 0, pricing),
        estimated_priced_cost_amount: estimated_cost(prompt, completion, 0, 0, pricing),
        prompt_price: pricing.0,
        completion_price: pricing.1,
        cache_read_price: pricing.2,
        cache_write_price: pricing.3,
        billing_group_id: billing_group_id.to_string(),
        billing_group_name: resolved_billing_group_name,
        billing_payment_mode,
        expires_at: expires_at.to_string(),
    })
    .await
}
