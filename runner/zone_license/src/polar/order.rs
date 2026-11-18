use abnegate_secret::sanitize;
use serde::Deserialize;

use super::billing_reason::BillingReason;
use super::customer::Customer;
use super::order_subscription::OrderSubscription;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Order {
    pub id: String,
    pub paid: bool,
    pub billing_reason: BillingReason,
    #[serde(default)]
    pub billing_name: Option<String>,
    #[serde(default)]
    pub product_id: Option<String>,
    pub customer: Customer,
    #[serde(default)]
    pub subscription: Option<OrderSubscription>,
}

impl Order {
    /// Only the subscription's seats count: the order's own `seats` is for
    /// one-time orders and may be a proration delta.
    pub fn seats(&self) -> Option<u32> {
        self.subscription
            .as_ref()
            .and_then(|subscription| subscription.seats)
            .and_then(|seats| u32::try_from(seats).ok())
            .filter(|seats| *seats >= 1)
    }

    pub fn email(&self) -> Option<&str> {
        filled(self.customer.email.as_deref())
    }

    /// The first name in the fallback chain that is still readable once
    /// control and invisible characters are stripped and its whitespace is
    /// collapsed, so the signed license and the email show the same name.
    pub fn customer_name(&self) -> Option<String> {
        [
            self.billing_name.as_deref(),
            self.customer.billing_name.as_deref(),
            self.customer.name.as_deref(),
            self.customer.email.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find_map(normalized)
    }
}

fn filled(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn normalized(name: &str) -> Option<String> {
    let name = sanitize(name)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn order(changes: serde_json::Value) -> Order {
        let mut value = json!({
            "id": "o",
            "status": "paid",
            "paid": true,
            "billing_reason": "subscription_create",
            "billing_name": null,
            "product_id": "p",
            "subscription_id": "s",
            "created_at": "2026-11-18T11:59:58Z",
            "seats": null,
            "customer": {"id": "c", "email": null, "name": null, "billing_name": null},
            "subscription": {
                "id": "s",
                "status": "active",
                "seats": null,
                "current_period_start": "2026-11-18T11:59:58Z",
                "current_period_end": "2027-11-18T11:59:58Z",
                "cancel_at_period_end": false
            }
        });
        merge(&mut value, changes);
        serde_json::from_value(value).unwrap()
    }

    fn merge(target: &mut serde_json::Value, changes: serde_json::Value) {
        match (target, changes) {
            (serde_json::Value::Object(target), serde_json::Value::Object(changes)) => {
                for (key, value) in changes {
                    merge(target.entry(key).or_insert(serde_json::Value::Null), value);
                }
            }
            (target, changes) => *target = changes,
        }
    }

    #[test]
    fn seats_come_from_the_subscription() {
        let order = order(json!({"seats": 3, "subscription": {"seats": 7}}));
        assert_eq!(order.seats(), Some(7));
    }

    #[test]
    fn order_seats_alone_are_none() {
        assert_eq!(order(json!({"seats": 3})).seats(), None);
    }

    #[test]
    fn zero_negative_or_missing_seats_are_none() {
        for seats in [json!(0), json!(-2), json!(null), json!(5_000_000_000_i64)] {
            let order = order(json!({"subscription": {"seats": seats}}));
            assert_eq!(order.seats(), None, "{seats}");
        }
    }

    #[test]
    fn email_ignores_blank_values() {
        assert_eq!(order(json!({"customer": {"email": "  "}})).email(), None);
        assert_eq!(
            order(json!({"customer": {"email": " a@b.test "}})).email(),
            Some("a@b.test")
        );
    }

    #[test]
    fn customer_name_walks_the_fallback_chain() {
        let full = json!({
            "billing_name": "Order Ltd",
            "customer": {"billing_name": "Customer Ltd", "name": "Jane", "email": "j@x.test"}
        });
        assert_eq!(order(full).customer_name().as_deref(), Some("Order Ltd"));
        let customer_billing = json!({
            "billing_name": " ",
            "customer": {"billing_name": "Customer Ltd", "name": "Jane", "email": "j@x.test"}
        });
        assert_eq!(
            order(customer_billing).customer_name().as_deref(),
            Some("Customer Ltd")
        );
        let name = json!({"customer": {"billing_name": "", "name": "Jane", "email": "j@x.test"}});
        assert_eq!(order(name).customer_name().as_deref(), Some("Jane"));
        let email = json!({"customer": {"email": "j@x.test"}});
        assert_eq!(order(email).customer_name().as_deref(), Some("j@x.test"));
        assert_eq!(order(json!({})).customer_name(), None);
    }

    #[test]
    fn customer_name_strips_control_and_invisible_characters_and_collapses_whitespace() {
        let order = order(json!({"billing_name": " Acme\u{1b}[31m\u{200b} \r\n Ltd\t"}));
        assert_eq!(order.customer_name().as_deref(), Some("Acme Ltd"));
    }

    #[test]
    fn a_name_with_nothing_readable_falls_through_the_chain() {
        let order = order(json!({
            "billing_name": "\u{1b}[31m\u{200b}",
            "customer": {"name": "Jane"}
        }));
        assert_eq!(order.customer_name().as_deref(), Some("Jane"));
    }
}
