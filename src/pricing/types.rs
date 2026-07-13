use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Source of the resolved price in the priority chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PricingSource {
    Contract,
    Discount,
    ListPrice,
}

impl PricingSource {
    pub fn priority(&self) -> u8 {
        match self {
            Self::Contract => 0,
            Self::Discount => 1,
            Self::ListPrice => 2,
        }
    }
}

/// Discount type for tenant-specific pricing overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscountType {
    /// Percentage off (e.g. 20 means 20% off list price).
    Percentage,
    /// Fixed override price per 1K tokens.
    Fixed,
}

/// A contract price entry — an exact per-model price for a specific user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractPrice {
    pub id: String,
    pub user_id: String,
    pub model_id: String,
    pub prompt_price: f64,
    pub completion_price: f64,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A tenant discount entry — a per-model discount/override for a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantDiscount {
    pub id: String,
    pub user_id: String,
    pub model_id: String,
    pub discount_type: DiscountType,
    /// For Percentage: the percent off (0-100). For Fixed: the override price per 1K tokens.
    pub discount_value: f64,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input to the pricing resolver.
#[derive(Debug, Clone)]
pub struct PricingInput {
    pub user_id: String,
    pub model_name: String,
}

/// Result from the pricing resolver.
#[derive(Debug, Clone)]
pub struct PricingResult {
    pub prompt_price: f64,
    pub completion_price: f64,
    pub source: PricingSource,
    pub applied_discount_pct: f64,
}

/// Slim row returned by the DB for contract price lookups.
#[derive(Debug, Clone)]
pub struct ContractPriceRow {
    pub prompt_price: f64,
    pub completion_price: f64,
    pub effective_from: DateTime<Utc>,
    pub effective_until: Option<DateTime<Utc>>,
}

/// Slim row returned by the DB for tenant discount lookups.
#[derive(Debug, Clone)]
pub struct TenantDiscountRow {
    pub discount_type: DiscountType,
    pub discount_value: f64,
}

/// All data needed by the resolver to resolve a single pricing request.
#[derive(Debug, Clone)]
pub struct PricingChainData {
    pub contract_prices: Vec<ContractPriceRow>,
    pub tenant_discounts: Vec<TenantDiscountRow>,
    pub list_price: (f64, f64),
}
