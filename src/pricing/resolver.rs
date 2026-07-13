use crate::pricing::types::*;

/// Interface for the pricing resolver. Pure logic — no I/O, no DB, no HTTP.
pub trait PricingResolver: Send + Sync {
    fn resolve(&self, input: &PricingInput, data: &PricingChainData) -> PricingResult;
}

/// Standard chain-of-responsibility resolver.
///
/// Priority order:
/// 1. Contract price (exact user+model match, within effective date range)
/// 2. Tenant discount (exact user+model match, percentage off or fixed override)
/// 3. List price (fallback from the model's base pricing)
pub struct ChainPricingResolver;

impl PricingResolver for ChainPricingResolver {
    fn resolve(&self, _input: &PricingInput, data: &PricingChainData) -> PricingResult {
        // Gate 1: Contract price
        if let Some(contract) = find_active_contract(&data.contract_prices) {
            return PricingResult {
                prompt_price: contract.prompt_price,
                completion_price: contract.completion_price,
                source: PricingSource::Contract,
                applied_discount_pct: 0.0,
            };
        }

        // Gate 2: Tenant discount
        if let Some(discount) = data.tenant_discounts.first() {
            let (p, c, pct) = apply_discount(
                data.list_price.0,
                data.list_price.1,
                discount.discount_type,
                discount.discount_value,
            );
            return PricingResult {
                prompt_price: p,
                completion_price: c,
                source: PricingSource::Discount,
                applied_discount_pct: pct,
            };
        }

        // Gate 3: List price (fallback)
        PricingResult {
            prompt_price: data.list_price.0,
            completion_price: data.list_price.1,
            source: PricingSource::ListPrice,
            applied_discount_pct: 0.0,
        }
    }
}

fn find_active_contract(prices: &[ContractPriceRow]) -> Option<&ContractPriceRow> {
    // The DB query already filters by effective_from/effective_until,
    // so the first match is the active one.
    prices.first()
}

fn apply_discount(
    base_prompt: f64,
    base_completion: f64,
    discount_type: DiscountType,
    discount_value: f64,
) -> (f64, f64, f64) {
    match discount_type {
        DiscountType::Percentage => {
            let factor = 1.0 - (discount_value / 100.0).clamp(0.0, 1.0);
            (base_prompt * factor, base_completion * factor, discount_value)
        }
        DiscountType::Fixed => {
            // discount_value is the per-1K-token price override for both fields
            (discount_value, discount_value, 0.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};

    fn utc_dt(s: &str) -> DateTime<Utc> {
        Utc.datetime_from_str(s, "%Y-%m-%dT%H:%M:%S")
            .unwrap()
    }

    fn make_data(
        contract_prompt: Option<(f64, f64)>,
        list_prompt: f64,
        list_completion: f64,
    ) -> PricingChainData {
        let contract_prices = contract_prompt
            .map(|(p, c)| {
                vec![ContractPriceRow {
                    prompt_price: p,
                    completion_price: c,
                    effective_from: utc_dt("2024-01-01T00:00:00"),
                    effective_until: None,
                }]
            })
            .unwrap_or_default();

        PricingChainData {
            contract_prices,
            tenant_discounts: vec![],
            list_price: (list_prompt, list_completion),
        }
    }

    #[test]
    fn contract_price_takes_priority() {
        let resolver = ChainPricingResolver;
        let input = PricingInput {
            user_id: "user-1".into(),
            model_name: "gpt-4".into(),
        };
        let data = make_data(Some((0.5, 1.5)), 2.0, 6.0);

        let result = resolver.resolve(&input, &data);
        assert_eq!(result.source, PricingSource::Contract);
        assert_eq!(result.prompt_price, 0.5);
        assert_eq!(result.completion_price, 1.5);
    }

    #[test]
    fn falls_back_to_list_price() {
        let resolver = ChainPricingResolver;
        let input = PricingInput {
            user_id: "user-2".into(),
            model_name: "gpt-4".into(),
        };
        let data = make_data(None, 2.0, 6.0);

        let result = resolver.resolve(&input, &data);
        assert_eq!(result.source, PricingSource::ListPrice);
        assert_eq!(result.prompt_price, 2.0);
        assert_eq!(result.completion_price, 6.0);
    }

    #[test]
    fn percentage_discount_applied_correctly() {
        let resolver = ChainPricingResolver;
        let input = PricingInput {
            user_id: "user-3".into(),
            model_name: "gpt-4".into(),
        };
        let data = PricingChainData {
            contract_prices: vec![],
            tenant_discounts: vec![TenantDiscountRow {
                discount_type: DiscountType::Percentage,
                discount_value: 20.0,
            }],
            list_price: (10.0, 30.0),
        };

        let result = resolver.resolve(&input, &data);
        assert_eq!(result.source, PricingSource::Discount);
        assert!((result.prompt_price - 8.0).abs() < 1e-9);
        assert!((result.completion_price - 24.0).abs() < 1e-9);
        assert!((result.applied_discount_pct - 20.0).abs() < 1e-9);
    }

    #[test]
    fn fixed_discount_overrides_price() {
        let resolver = ChainPricingResolver;
        let input = PricingInput {
            user_id: "user-4".into(),
            model_name: "gpt-4".into(),
        };
        let data = PricingChainData {
            contract_prices: vec![],
            tenant_discounts: vec![TenantDiscountRow {
                discount_type: DiscountType::Fixed,
                discount_value: 5.0,
            }],
            list_price: (10.0, 30.0),
        };

        let result = resolver.resolve(&input, &data);
        assert_eq!(result.source, PricingSource::Discount);
        assert!((result.prompt_price - 5.0).abs() < 1e-9);
        assert!((result.completion_price - 5.0).abs() < 1e-9);
    }

    #[test]
    fn contract_beats_discount() {
        let resolver = ChainPricingResolver;
        let input = PricingInput {
            user_id: "user-5".into(),
            model_name: "gpt-4".into(),
        };
        let data = PricingChainData {
            contract_prices: vec![ContractPriceRow {
                prompt_price: 1.0,
                completion_price: 3.0,
                effective_from: utc_dt("2024-01-01T00:00:00"),
                effective_until: None,
            }],
            tenant_discounts: vec![TenantDiscountRow {
                discount_type: DiscountType::Percentage,
                discount_value: 50.0,
            }],
            list_price: (10.0, 30.0),
        };

        let result = resolver.resolve(&input, &data);
        assert_eq!(result.source, PricingSource::Contract);
        assert_eq!(result.prompt_price, 1.0);
        assert_eq!(result.completion_price, 3.0);
    }

    #[test]
    fn percentage_discount_caps_at_100() {
        let resolver = ChainPricingResolver;
        let input = PricingInput {
            user_id: "user-6".into(),
            model_name: "gpt-4".into(),
        };
        let data = PricingChainData {
            contract_prices: vec![],
            tenant_discounts: vec![TenantDiscountRow {
                discount_type: DiscountType::Percentage,
                discount_value: 150.0, // 150% off → clamped to 100%
            }],
            list_price: (10.0, 30.0),
        };

        let result = resolver.resolve(&input, &data);
        assert_eq!(result.source, PricingSource::Discount);
        assert!((result.prompt_price - 0.0).abs() < 1e-9);
        assert!((result.completion_price - 0.0).abs() < 1e-9);
    }
}
