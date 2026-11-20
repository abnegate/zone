use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::subscription_status::SubscriptionStatus;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OrderSubscription {
    pub id: String,
    pub status: SubscriptionStatus,
    #[serde(default)]
    pub seats: Option<i64>,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    #[serde(default)]
    pub product_id: Option<String>,
    #[serde(default)]
    pub cancel_at_period_end: bool,
    #[serde(default)]
    pub ended_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::Value;

    use super::*;

    const SUBSCRIPTION: &str = include_str!("../../tests/fixtures/polar/subscription.json");
    const PRODUCT: &str = "5f0c8a9e-6b1d-4c2e-9a3f-7d4e1b2c3a40";
    const SUBSCRIPTION_EVENTS: [&str; 3] = [
        include_str!("../../tests/fixtures/polar/subscription_updated.json"),
        include_str!("../../tests/fixtures/polar/subscription_canceled.json"),
        include_str!("../../tests/fixtures/polar/subscription_cycled.json"),
    ];
    const ORDER_EVENTS: [&str; 3] = [
        include_str!("../../tests/fixtures/polar/order_paid_subscription_create.json"),
        include_str!("../../tests/fixtures/polar/order_paid_subscription_cycle.json"),
        include_str!("../../tests/fixtures/polar/order_paid_subscription_update.json"),
    ];

    fn at(text: &str, pointer: &str) -> OrderSubscription {
        let event: Value = serde_json::from_str(text).unwrap();
        serde_json::from_value(event.pointer(pointer).unwrap().clone()).unwrap()
    }

    #[test]
    fn parses_a_subscription_from_the_api() {
        let subscription: OrderSubscription = serde_json::from_str(SUBSCRIPTION).unwrap();
        assert_eq!(
            subscription,
            OrderSubscription {
                id: "b7e4c2a1-3d5f-4e6a-9b8c-0d1e2f3a4b5c".to_owned(),
                status: SubscriptionStatus::Active,
                seats: Some(5),
                current_period_start: Utc.with_ymd_and_hms(2026, 11, 18, 11, 59, 58).unwrap(),
                current_period_end: Utc.with_ymd_and_hms(2027, 11, 18, 11, 59, 58).unwrap(),
                product_id: Some(PRODUCT.to_owned()),
                cancel_at_period_end: false,
                ended_at: None,
            }
        );
    }

    #[test]
    fn webhook_subscriptions_still_parse() {
        for text in SUBSCRIPTION_EVENTS {
            let subscription = at(text, "/data");
            assert_eq!(subscription.product_id.as_deref(), Some(PRODUCT));
        }
        for text in ORDER_EVENTS {
            at(text, "/data/subscription");
        }
    }

    #[test]
    fn reads_a_scheduled_cancellation_and_an_end() {
        let subscription: OrderSubscription = serde_json::from_str(
            r#"{"id":"s","status":"canceled","current_period_start":"2026-11-18T11:59:58Z","current_period_end":"2027-11-18T11:59:58Z","cancel_at_period_end":true,"ended_at":"2027-01-02T03:04:05Z","product_id":"p"}"#,
        )
        .unwrap();
        assert!(subscription.cancel_at_period_end);
        assert_eq!(
            subscription.ended_at,
            Some(Utc.with_ymd_and_hms(2027, 1, 2, 3, 4, 5).unwrap())
        );
        assert_eq!(subscription.product_id.as_deref(), Some("p"));
    }

    #[test]
    fn the_new_fields_may_be_absent() {
        let subscription: OrderSubscription = serde_json::from_str(
            r#"{"id":"s","status":"active","current_period_start":"2026-11-18T11:59:58Z","current_period_end":"2027-11-18T11:59:58Z"}"#,
        )
        .unwrap();
        assert_eq!(subscription.product_id, None);
        assert!(!subscription.cancel_at_period_end);
        assert_eq!(subscription.ended_at, None);
    }

    #[test]
    fn seats_may_be_absent() {
        let subscription: OrderSubscription = serde_json::from_str(
            r#"{"id":"s","status":"active","current_period_start":"2026-11-18T11:59:58Z","current_period_end":"2027-11-18T11:59:58Z","cancel_at_period_end":false,"amount":100}"#,
        )
        .unwrap();
        assert_eq!(subscription.seats, None);
    }
}
