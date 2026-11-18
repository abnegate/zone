use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingReason {
    Purchase,
    SubscriptionCreate,
    SubscriptionCycle,
    SubscriptionUpdate,
    SubscriptionMeterCycle,
    #[serde(other)]
    Other,
}

impl BillingReason {
    pub fn issues(self) -> bool {
        matches!(
            self,
            Self::SubscriptionCreate | Self::SubscriptionCycle | Self::SubscriptionUpdate
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> BillingReason {
        serde_json::from_str(&format!("\"{text}\"")).unwrap()
    }

    #[test]
    fn parses_every_documented_reason() {
        assert_eq!(parse("purchase"), BillingReason::Purchase);
        assert_eq!(
            parse("subscription_create"),
            BillingReason::SubscriptionCreate
        );
        assert_eq!(
            parse("subscription_cycle"),
            BillingReason::SubscriptionCycle
        );
        assert_eq!(
            parse("subscription_update"),
            BillingReason::SubscriptionUpdate
        );
        assert_eq!(
            parse("subscription_meter_cycle"),
            BillingReason::SubscriptionMeterCycle
        );
    }

    #[test]
    fn unknown_reason_is_other() {
        assert_eq!(parse("subscription_pause"), BillingReason::Other);
    }

    #[test]
    fn only_subscription_charges_issue() {
        assert!(BillingReason::SubscriptionCreate.issues());
        assert!(BillingReason::SubscriptionCycle.issues());
        assert!(BillingReason::SubscriptionUpdate.issues());
        assert!(!BillingReason::Purchase.issues());
        assert!(!BillingReason::SubscriptionMeterCycle.issues());
        assert!(!BillingReason::Other.issues());
    }
}
