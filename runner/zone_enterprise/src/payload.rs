use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    pub id: Uuid,
    pub customer: String,
    pub seats: u32,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub key_id: String,
}

impl Payload {
    pub fn validate(&self) -> Result<(), Error> {
        if self.seats < 1 {
            return Err(Error::Payload("seats must be at least 1".into()));
        }
        if self.expires_at <= self.issued_at {
            return Err(Error::Payload("expires_at must be after issued_at".into()));
        }
        if self.customer.trim().is_empty() {
            return Err(Error::Payload("customer must not be blank".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::{Value, json};

    use super::*;
    use crate::fixtures;

    const KEYS: [&str; 6] = [
        "id",
        "customer",
        "seats",
        "issued_at",
        "expires_at",
        "key_id",
    ];

    #[test]
    fn serializes_the_wire_format() {
        let value = serde_json::to_value(fixtures::payload()).unwrap();
        assert_eq!(
            value,
            json!({
                "id": "0193a8e2-7c4b-7d10-8f00-4d2c5a9b1e01",
                "customer": "Acme Ltd",
                "seats": 25,
                "issued_at": "2026-11-01T00:00:00Z",
                "expires_at": "2027-11-01T00:00:00Z",
                "key_id": "test-1"
            })
        );
    }

    #[test]
    fn serializes_exactly_the_contract_keys() {
        let value = serde_json::to_value(fixtures::payload()).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = KEYS.to_vec();
        expected.sort_unstable();
        assert_eq!(keys, expected);
    }

    #[test]
    fn refuses_a_payload_missing_any_required_key() {
        for key in KEYS {
            let mut value = serde_json::to_value(fixtures::payload()).unwrap();
            value.as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<Payload>(value).is_err(),
                "accepted a payload without {key}"
            );
        }
    }

    #[test]
    fn tolerates_unknown_fields() {
        let mut value = serde_json::to_value(fixtures::payload()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("region".into(), Value::from("eu"));
        assert_eq!(
            serde_json::from_value::<Payload>(value).unwrap(),
            fixtures::payload()
        );
    }

    #[test]
    fn decodes_a_legacy_payload_that_names_a_tier() {
        for tier in ["team", "enterprise"] {
            let mut value = serde_json::to_value(fixtures::payload()).unwrap();
            value
                .as_object_mut()
                .unwrap()
                .insert("tier".into(), Value::from(tier));
            assert_eq!(
                serde_json::from_value::<Payload>(value).unwrap(),
                fixtures::payload(),
                "tier {tier}"
            );
        }
    }

    #[test]
    fn accepts_a_valid_payload() {
        assert_eq!(fixtures::payload().validate(), Ok(()));
    }

    #[test]
    fn accepts_a_single_seat() {
        assert_eq!(
            Payload {
                seats: 1,
                ..fixtures::payload()
            }
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn refuses_zero_seats() {
        let error = Payload {
            seats: 0,
            ..fixtures::payload()
        }
        .validate()
        .unwrap_err();
        assert!(matches!(error, Error::Payload(_)), "got {error:?}");
    }

    #[test]
    fn refuses_expiry_equal_to_issue() {
        let payload = fixtures::payload();
        let error = Payload {
            expires_at: payload.issued_at,
            ..payload
        }
        .validate()
        .unwrap_err();
        assert!(matches!(error, Error::Payload(_)), "got {error:?}");
    }

    #[test]
    fn refuses_expiry_before_issue() {
        let payload = fixtures::payload();
        let expires_at = payload.issued_at - TimeDelta::seconds(1);
        let error = Payload {
            expires_at,
            ..payload
        }
        .validate()
        .unwrap_err();
        assert!(matches!(error, Error::Payload(_)), "got {error:?}");
    }

    #[test]
    fn refuses_a_blank_customer() {
        for customer in ["", "   ", "\t\n"] {
            let error = Payload {
                customer: customer.into(),
                ..fixtures::payload()
            }
            .validate()
            .unwrap_err();
            assert!(
                matches!(error, Error::Payload(_)),
                "accepted customer {customer:?}"
            );
        }
    }
}
