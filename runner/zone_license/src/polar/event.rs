use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::event_type::EventType;
use crate::order_error::OrderError;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub kind: EventType,
    pub timestamp: DateTime<Utc>,
    pub api_version: String,
    pub data: Value,
}

impl Event {
    pub fn parse(body: &[u8]) -> Result<Self, OrderError> {
        serde_json::from_slice(body).map_err(|_| OrderError::Malformed)
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn parses_the_envelope() {
        let body = br#"{"type":"order.paid","timestamp":"2026-11-18T11:59:58.123456Z","api_version":"2026-10","data":{"id":"x"}}"#;
        let event = Event::parse(body).unwrap();
        assert_eq!(event.kind, EventType::OrderPaid);
        assert_eq!(
            event.timestamp,
            Utc.with_ymd_and_hms(2026, 11, 18, 11, 59, 58).unwrap()
                + chrono::Duration::microseconds(123_456)
        );
        assert_eq!(event.api_version, "2026-10");
        assert_eq!(event.data["id"], "x");
    }

    #[test]
    fn refuses_a_body_that_is_not_an_event() {
        assert_eq!(Event::parse(b"{\"type\":1}"), Err(OrderError::Malformed));
        assert_eq!(Event::parse(b"not json"), Err(OrderError::Malformed));
    }
}
