pub const EVENT_TYPE: &str = "event_type";
pub const BILLING_REASON: &str = "billing_reason";
pub const UNPAID: &str = "unpaid";
pub const PRODUCT: &str = "product";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignored {
    EventType,
    BillingReason,
    Unpaid,
    Product,
}

impl Ignored {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EventType => EVENT_TYPE,
            Self::BillingReason => BILLING_REASON,
            Self::Unpaid => UNPAID,
            Self::Product => PRODUCT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_are_the_wire_contract() {
        assert_eq!(Ignored::EventType.as_str(), "event_type");
        assert_eq!(Ignored::BillingReason.as_str(), "billing_reason");
        assert_eq!(Ignored::Unpaid.as_str(), "unpaid");
        assert_eq!(Ignored::Product.as_str(), "product");
    }
}
