use crate::db::Database;
use crate::pricing::resolver::{ChainPricingResolver, PricingResolver};
use crate::pricing::types::*;
use std::sync::Arc;

/// Orchestrates pricing resolution by loading data from the DB and running the resolver.
pub struct PricingChainService {
    db: Arc<Database>,
    resolver: Box<dyn PricingResolver>,
}

impl PricingChainService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            resolver: Box::new(ChainPricingResolver),
        }
    }

    /// Resolve pricing for a given user + model.
    pub async fn resolve(&self, input: &PricingInput) -> Result<PricingResult, crate::db::DbError> {
        let contract_prices = self
            .db
            .get_contract_prices_for_user(&input.user_id, &input.model_name)
            .await?;

        let tenant_discount = self
            .db
            .get_tenant_discount_for_user(&input.user_id, &input.model_name)
            .await?;

        // Fallback: get list price from the models table
        let list_price = self
            .db
            .lookup_model_pricing(&input.model_name)
            .await
            .unwrap_or((0.0, 0.0));

        let data = PricingChainData {
            contract_prices,
            tenant_discounts: tenant_discount.into_iter().collect(),
            list_price,
        };

        Ok(self.resolver.resolve(input, &data))
    }
}
