use serde::Deserialize;

pub const ORDER_PAID: &str = "order.paid";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "String")]
pub enum EventType {
    OrderPaid,
    Other(String),
}

impl EventType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::OrderPaid => ORDER_PAID,
            Self::Other(name) => name,
        }
    }
}

impl From<String> for EventType {
    fn from(name: String) -> Self {
        if name == ORDER_PAID {
            Self::OrderPaid
        } else {
            Self::Other(name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_order_paid() {
        let kind: EventType = serde_json::from_str("\"order.paid\"").unwrap();
        assert_eq!(kind, EventType::OrderPaid);
        assert_eq!(kind.as_str(), ORDER_PAID);
    }

    #[test]
    fn keeps_any_other_name() {
        let kind: EventType = serde_json::from_str("\"subscription.updated\"").unwrap();
        assert_eq!(kind, EventType::Other("subscription.updated".to_owned()));
        assert_eq!(kind.as_str(), "subscription.updated");
    }
}
