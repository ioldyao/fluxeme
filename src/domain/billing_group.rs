use serde::{Deserialize, Serialize};

pub const DEFAULT_BILLING_GROUP_ID: &str = "billing-group-default-prepaid";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingPaymentMode {
    Prepaid,
    Postpaid,
}

impl BillingPaymentMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepaid => "prepaid",
            Self::Postpaid => "postpaid",
        }
    }
}

impl std::str::FromStr for BillingPaymentMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "prepaid" => Ok(Self::Prepaid),
            "postpaid" => Ok(Self::Postpaid),
            other => Err(format!("unsupported billing payment mode: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingGroupRow {
    pub id: String,
    pub name: String,
    pub payment_mode: BillingPaymentMode,
    pub status: String,
    pub is_default: bool,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub deleted_at: Option<String>,
    #[serde(default)]
    pub deleted_by: Option<String>,
}

impl BillingGroupRow {
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
}
