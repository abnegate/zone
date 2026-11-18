use serde::Deserialize;

use crate::grant::Grant;
use crate::ignored::Ignored;
use crate::order_error::OrderError;
use crate::polar::{Event, EventType, Order};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Issue(Grant),
    Ignore(Ignored),
}

pub fn decide(event: &Event, products: &[String]) -> Result<Decision, OrderError> {
    if event.kind != EventType::OrderPaid {
        return Ok(Decision::Ignore(Ignored::EventType));
    }
    let order = Order::deserialize(&event.data).map_err(|_| OrderError::Malformed)?;
    if !order.paid {
        return Ok(Decision::Ignore(Ignored::Unpaid));
    }
    if !order.billing_reason.issues() {
        return Ok(Decision::Ignore(Ignored::BillingReason));
    }
    let Some(product) = sold(&order, products) else {
        return Ok(Decision::Ignore(Ignored::Product));
    };
    let (Some(subscription), Some(seats)) = (&order.subscription, order.seats()) else {
        return Err(OrderError::Misconfigured {
            order: order.id.clone(),
            product: product.to_owned(),
        });
    };
    let email = order.email().ok_or(OrderError::Email)?;
    let customer = order.customer_name().unwrap_or(email);
    Ok(Decision::Issue(Grant {
        order: order.id.clone(),
        subscription: subscription.id.clone(),
        customer: customer.to_owned(),
        email: email.to_owned(),
        seats,
        period_start: subscription.current_period_start,
        period_end: subscription.current_period_end,
    }))
}

fn sold<'a>(order: &'a Order, products: &[String]) -> Option<&'a str> {
    order
        .product_id
        .as_deref()
        .filter(|product| products.iter().any(|listed| listed == product))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn event(kind: &str, data: serde_json::Value) -> Event {
        serde_json::from_value(json!({
            "type": kind,
            "timestamp": "2026-11-18T11:59:58Z",
            "api_version": "2026-10",
            "data": data
        }))
        .unwrap()
    }

    #[test]
    fn other_events_are_ignored_without_reading_data() {
        let event = event("subscription.updated", json!("not an order"));
        assert_eq!(
            decide(&event, &[]),
            Ok(Decision::Ignore(Ignored::EventType))
        );
    }

    #[test]
    fn malformed_order_is_an_error() {
        let event = event("order.paid", json!({"id": "o"}));
        assert_eq!(decide(&event, &[]), Err(OrderError::Malformed));
    }
}
