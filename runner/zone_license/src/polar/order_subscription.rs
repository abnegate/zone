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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seats_may_be_absent() {
        let subscription: OrderSubscription = serde_json::from_str(
            r#"{"id":"s","status":"active","current_period_start":"2026-11-18T11:59:58Z","current_period_end":"2027-11-18T11:59:58Z","cancel_at_period_end":false,"amount":100}"#,
        )
        .unwrap();
        assert_eq!(subscription.seats, None);
    }
}
