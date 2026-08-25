use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::domain::billing_group::BillingPaymentMode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenAccountingMode {
    RawTokens,
    StandardizedCredits,
}

impl TokenAccountingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RawTokens => "raw_tokens",
            Self::StandardizedCredits => "standardized_credits",
        }
    }
}

impl std::str::FromStr for TokenAccountingMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "raw_tokens" => Ok(Self::RawTokens),
            "standardized_credits" => Ok(Self::StandardizedCredits),
            other => Err(format!("unsupported token accounting mode: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenPackageExhaustionPolicy {
    PackageThenWallet,
    PackageOnly,
}

impl TokenPackageExhaustionPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PackageThenWallet => "package_then_wallet",
            Self::PackageOnly => "package_only",
        }
    }
}

impl std::str::FromStr for TokenPackageExhaustionPolicy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "package_then_wallet" => Ok(Self::PackageThenWallet),
            "package_only" => Ok(Self::PackageOnly),
            other => Err(format!(
                "unsupported token package exhaustion policy: {other}"
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_hit_input_tokens: u64,
    pub cache_write_tokens: u64,
}

impl TokenUsage {
    pub fn raw_units(self) -> u64 {
        self.prompt_tokens
            .saturating_add(self.completion_tokens)
            .saturating_add(self.cache_hit_input_tokens)
            .saturating_add(self.cache_write_tokens)
    }

    pub fn standardized_credits(
        self,
        input_factor: Decimal,
        output_factor: Decimal,
        cache_factor: Decimal,
        cache_write_factor: Decimal,
    ) -> u64 {
        let value = Decimal::from(self.prompt_tokens) * input_factor
            + Decimal::from(self.completion_tokens) * output_factor
            + Decimal::from(self.cache_hit_input_tokens) * cache_factor
            + Decimal::from(self.cache_write_tokens) * cache_write_factor;
        value.max(Decimal::ZERO).ceil().to_u64().unwrap_or(u64::MAX)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PriceSnapshot {
    pub prompt: Decimal,
    pub completion: Decimal,
    pub cache_read: Decimal,
    pub cache_write: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SettlementBreakdown {
    pub actual_units: u64,
    pub package_units: u64,
    pub actual_priced_cost: Decimal,
    pub package_priced_cost: Decimal,
    pub wallet_amount: Decimal,
}

/// Computes final usage cost independently from the reservation hold.
///
/// Package units cover every billable token component in a deterministic
/// order: uncached input, output, cache-read input, then cache-write input.
/// The monetary price remains independent from package-unit consumption.
pub fn settle_usage(
    usage: TokenUsage,
    cache_write_tokens: u64,
    prices: PriceSnapshot,
    accounting_mode: Option<TokenAccountingMode>,
    input_factor: Decimal,
    output_factor: Decimal,
    cache_factor: Decimal,
    reserved_package_units: u64,
    payment_mode: BillingPaymentMode,
) -> SettlementBreakdown {
    let actual_units = match accounting_mode {
        Some(TokenAccountingMode::StandardizedCredits) => {
            usage.standardized_credits(input_factor, output_factor, cache_factor, Decimal::ONE)
        }
        _ => usage.raw_units(),
    };
    let package_units = actual_units.min(reserved_package_units);
    let actual_priced_cost = (Decimal::from(usage.prompt_tokens) * prices.prompt
        + Decimal::from(usage.completion_tokens) * prices.completion
        + Decimal::from(usage.cache_hit_input_tokens) * prices.cache_read
        + Decimal::from(cache_write_tokens) * prices.cache_write)
        / Decimal::from(1_000_000u64);

    let mut remaining = Decimal::from(package_units);
    let mut package_priced_cost = Decimal::ZERO;
    let covered_components = if accounting_mode == Some(TokenAccountingMode::StandardizedCredits) {
        vec![
            (usage.prompt_tokens, input_factor, prices.prompt),
            (usage.completion_tokens, output_factor, prices.completion),
            (
                usage.cache_hit_input_tokens,
                cache_factor,
                prices.cache_read,
            ),
            (usage.cache_write_tokens, Decimal::ONE, prices.cache_write),
        ]
    } else {
        vec![
            (usage.prompt_tokens, Decimal::ONE, prices.prompt),
            (usage.completion_tokens, Decimal::ONE, prices.completion),
            (
                usage.cache_hit_input_tokens,
                Decimal::ONE,
                prices.cache_read,
            ),
            (usage.cache_write_tokens, Decimal::ONE, prices.cache_write),
        ]
    };
    for (tokens, factor, price) in covered_components {
        if remaining <= Decimal::ZERO || factor <= Decimal::ZERO {
            continue;
        }
        let covered_tokens = (remaining / factor).min(Decimal::from(tokens));
        package_priced_cost += covered_tokens * price / Decimal::from(1_000_000u64);
        remaining -= covered_tokens * factor;
    }
    let wallet_amount = if payment_mode == BillingPaymentMode::Prepaid {
        Decimal::ZERO
    } else {
        (actual_priced_cost - package_priced_cost).max(Decimal::ZERO)
    };

    SettlementBreakdown {
        actual_units,
        package_units,
        actual_priced_cost,
        package_priced_cost,
        wallet_amount,
    }
}

#[cfg(test)]
mod settlement_tests {
    use super::*;

    fn prices() -> PriceSnapshot {
        PriceSnapshot {
            prompt: Decimal::new(20, 2),
            completion: Decimal::new(120, 2),
            cache_read: Decimal::new(2, 2),
            cache_write: Decimal::ZERO,
        }
    }

    #[test]
    fn prices_actual_usage_without_using_reservation_hold() {
        let result = settle_usage(
            TokenUsage {
                prompt_tokens: 23,
                completion_tokens: 8,
                cache_hit_input_tokens: 0,
                cache_write_tokens: 0,
            },
            0,
            prices(),
            Some(TokenAccountingMode::RawTokens),
            Decimal::ONE,
            Decimal::ONE,
            Decimal::ZERO,
            0,
            BillingPaymentMode::Metered,
        );
        assert_eq!(result.actual_priced_cost, Decimal::new(142, 7));
        assert_eq!(result.wallet_amount, result.actual_priced_cost);
    }

    #[test]
    fn package_units_cover_input_before_output() {
        let result = settle_usage(
            TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                cache_hit_input_tokens: 0,
                cache_write_tokens: 0,
            },
            0,
            prices(),
            Some(TokenAccountingMode::RawTokens),
            Decimal::ONE,
            Decimal::ONE,
            Decimal::ZERO,
            10,
            BillingPaymentMode::Metered,
        );
        assert_eq!(result.package_units, 10);
        assert_eq!(result.package_priced_cost, Decimal::new(2, 6));
        assert_eq!(result.wallet_amount, Decimal::new(12, 6));
    }

    #[test]
    fn raw_package_units_cover_cache_hit_and_cache_write_before_wallet() {
        let mut snapshot = prices();
        snapshot.cache_write = Decimal::ONE;
        let result = settle_usage(
            TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 10,
                cache_hit_input_tokens: 10,
                cache_write_tokens: 10,
            },
            10,
            snapshot,
            Some(TokenAccountingMode::RawTokens),
            Decimal::ONE,
            Decimal::ONE,
            Decimal::ZERO,
            40,
            BillingPaymentMode::Prepaid,
        );
        assert_eq!(result.actual_units, 40);
        assert_eq!(result.package_units, 40);
        assert_eq!(result.wallet_amount, Decimal::ZERO);
    }

    #[test]
    fn prepaid_keeps_theoretical_cost_but_does_not_debit_wallet() {
        let result = settle_usage(
            TokenUsage {
                prompt_tokens: 1,
                completion_tokens: 1,
                cache_hit_input_tokens: 0,
                cache_write_tokens: 0,
            },
            5,
            prices(),
            Some(TokenAccountingMode::RawTokens),
            Decimal::ONE,
            Decimal::ONE,
            Decimal::ZERO,
            0,
            BillingPaymentMode::Prepaid,
        );
        assert_eq!(result.actual_priced_cost, Decimal::new(14, 7));
        assert_eq!(result.wallet_amount, Decimal::ZERO);
    }

    #[test]
    fn cache_write_is_included_in_money_even_without_package_factor() {
        let mut snapshot = prices();
        snapshot.cache_write = Decimal::ONE;
        let result = settle_usage(
            TokenUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                cache_hit_input_tokens: 0,
                cache_write_tokens: 0,
            },
            10,
            snapshot,
            Some(TokenAccountingMode::RawTokens),
            Decimal::ONE,
            Decimal::ONE,
            Decimal::ZERO,
            0,
            BillingPaymentMode::Metered,
        );
        assert_eq!(result.actual_priced_cost, Decimal::new(1, 5));
        assert_eq!(result.wallet_amount, result.actual_priced_cost);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenReservationRequest {
    pub request_id: String,
    #[serde(default)]
    pub request_fingerprint: String,
    pub user_id: String,
    pub user_name: String,
    pub api_key_name: String,
    pub team_id: Option<String>,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_hit_input_tokens: u64,
    pub estimated_wallet_amount: Decimal,
    pub estimated_priced_cost_amount: Decimal,
    /// Four per-million price components captured at reservation time.
    pub prompt_price: Decimal,
    pub completion_price: Decimal,
    pub cache_read_price: Decimal,
    pub cache_write_price: Decimal,
    pub billing_group_id: String,
    pub billing_group_name: String,
    pub billing_payment_mode: BillingPaymentMode,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSettlementRequest {
    pub reservation_id: String,
    pub actual_prompt_tokens: u64,
    pub actual_completion_tokens: u64,
    pub actual_cache_hit_input_tokens: u64,
    #[serde(default)]
    pub actual_cache_write_tokens: u64,
    pub actual_package_units: u64,
    pub actual_wallet_amount: Decimal,
    pub status_code: u16,
    pub success: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenReservationHandle {
    pub reservation_id: String,
    pub request_id: String,
    pub package_grant_id: Option<String>,
    pub accounting_mode: Option<TokenAccountingMode>,
    pub input_factor: Decimal,
    pub output_factor: Decimal,
    pub cache_factor: Decimal,
    pub reserved_package_units: u64,
    pub reserved_total_units: u64,
    pub reserved_wallet_amount: Decimal,
    /// The exact four-component model price snapshot used by estimate and settlement.
    pub prompt_price: Decimal,
    pub completion_price: Decimal,
    pub cache_read_price: Decimal,
    pub cache_write_price: Decimal,
    pub billing_group_id: String,
    pub billing_group_name: String,
    pub billing_payment_mode: BillingPaymentMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPackagePlanRow {
    pub id: String,
    pub code: String,
    pub name: String,
    pub accounting_mode: TokenAccountingMode,
    pub display_token_amount: u64,
    pub total_units: u64,
    pub input_credit_factor: Decimal,
    pub output_credit_factor: Decimal,
    pub cache_credit_factor: Decimal,
    pub exhaustion_policy: TokenPackageExhaustionPolicy,
    pub priority: i32,
    pub validity_days: Option<i32>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPackageGrantRow {
    pub id: String,
    pub plan_id: String,
    pub plan_code: String,
    pub plan_name: String,
    pub user_id: Option<String>,
    pub team_id: Option<String>,
    pub accounting_mode: TokenAccountingMode,
    pub display_token_amount: u64,
    pub total_units: u64,
    pub consumed_units: u64,
    pub reserved_units: u64,
    pub priority: i32,
    pub exhaustion_policy: TokenPackageExhaustionPolicy,
    pub status: String,
    pub expires_at: Option<String>,
    pub created_at: String,
}
