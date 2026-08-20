use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
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
            other => Err(format!("unsupported token package exhaustion policy: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_hit_input_tokens: u64,
}

impl TokenUsage {
    pub fn raw_units(self) -> u64 {
        self.prompt_tokens.saturating_add(self.completion_tokens)
    }

    pub fn standardized_credits(
        self,
        input_factor: Decimal,
        output_factor: Decimal,
        cache_factor: Decimal,
    ) -> u64 {
        let value = Decimal::from(self.prompt_tokens) * input_factor
            + Decimal::from(self.completion_tokens) * output_factor
            + Decimal::from(self.cache_hit_input_tokens) * cache_factor;
        value.max(Decimal::ZERO).ceil().to_u64().unwrap_or(u64::MAX)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenReservationRequest {
    pub request_id: String,
    pub user_id: String,
    pub team_id: Option<String>,
    pub model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cache_hit_input_tokens: u64,
    pub estimated_wallet_amount: Decimal,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSettlementRequest {
    pub reservation_id: String,
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
