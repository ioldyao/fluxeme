use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use rust_decimal::Decimal;
use serde_json::Value;
use tokio::task;

use crate::db::Database;
use crate::domain::token_package::{
    TokenReservationHandle, TokenReservationRequest, TokenSettlementRequest,
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
        success: bool,
        reason: &str,
    ) {
        let actual_units = if self.handle.accounting_mode
            == Some(crate::domain::token_package::TokenAccountingMode::StandardizedCredits)
        {
            crate::domain::token_package::TokenUsage {
                prompt_tokens,
                completion_tokens,
                cache_hit_input_tokens,
            }
            .standardized_credits(
                self.handle.input_factor,
                self.handle.output_factor,
                self.handle.cache_factor,
            )
        } else {
            prompt_tokens.saturating_add(completion_tokens)
        };
        let reserved_total = self.handle.reserved_total_units.max(1);
        let wallet_units = actual_units.saturating_sub(self.handle.reserved_package_units);
        let wallet_amount = if wallet_units == 0 {
            Decimal::ZERO
        } else {
            self.handle.reserved_wallet_amount * Decimal::from(wallet_units.min(reserved_total))
                / Decimal::from(reserved_total)
        };
        self.settle(
            actual_units,
            wallet_amount,
            prompt_tokens,
            completion_tokens,
            cache_hit_input_tokens,
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
                    actual_package_units: actual_units,
                    actual_wallet_amount: wallet_amount,
                    status_code: if success { 200 } else { 502 },
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
        reason: &str,
    ) {
        let actual_units = if self.handle.accounting_mode
            == Some(crate::domain::token_package::TokenAccountingMode::StandardizedCredits)
        {
            crate::domain::token_package::TokenUsage {
                prompt_tokens,
                completion_tokens,
                cache_hit_input_tokens,
            }
            .standardized_credits(
                self.handle.input_factor,
                self.handle.output_factor,
                self.handle.cache_factor,
            )
        } else {
            prompt_tokens.saturating_add(completion_tokens)
        };
        let reserved_total = self.handle.reserved_total_units.max(1);
        let wallet_units = actual_units.saturating_sub(self.handle.reserved_package_units);
        let wallet_amount = if wallet_units == 0 {
            Decimal::ZERO
        } else {
            self.handle.reserved_wallet_amount * Decimal::from(wallet_units.min(reserved_total))
                / Decimal::from(reserved_total)
        };
        self.settle(
            actual_units,
            wallet_amount,
            prompt_tokens,
            completion_tokens,
            cache_hit_input_tokens,
            false,
            reason,
        );
    }

    pub fn release(&self, reason: &str) {
        if self.finalized.swap(true, Ordering::AcqRel) {
            return;
        }
        let db = self.db.clone();
        let reservation_id = self.handle.reservation_id.clone();
        let finalized = self.finalized.clone();
        let reason = reason.to_string();
        task::spawn(async move {
            if let Err(error) = db.release_token_request(&reservation_id, &reason).await {
                tracing::error!(%error, "token reservation release failed");
                finalized.store(false, Ordering::Release);
            }
        });
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
    pricing: (Decimal, Decimal, Decimal, Decimal),
) -> Decimal {
    let (prompt, completion, cache_read, _) = pricing;
    (Decimal::from(prompt_tokens) / Decimal::from(1_000_000u64)) * prompt
        + (Decimal::from(completion_tokens) / Decimal::from(1_000_000u64)) * completion
        + (Decimal::from(cache_hit_input_tokens) / Decimal::from(1_000_000u64)) * cache_read
}

pub async fn reserve(
    db: Arc<Database>,
    request_id: &str,
    user_id: &str,
    team_id: Option<&str>,
    model: &str,
    body: &Value,
    anthropic: bool,
    expires_at: &str,
) -> Result<TokenReservationHandle, crate::db::DbError> {
    let prompt = estimate_input_tokens(body, anthropic);
    let completion = max_output_tokens(body);
    let pricing = db.lookup_model_pricing(model).await?;
    db.reserve_token_request(&TokenReservationRequest {
        request_id: request_id.to_string(),
        user_id: user_id.to_string(),
        team_id: team_id.map(str::to_string),
        model: model.to_string(),
        prompt_tokens: prompt,
        completion_tokens: completion,
        cache_hit_input_tokens: 0,
        estimated_wallet_amount: estimated_cost(prompt, completion, 0, pricing),
        expires_at: expires_at.to_string(),
    })
    .await
}
