use std::fmt;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::payload::Payload;
use crate::status::{self, Status};
use crate::tier::Tier;

#[derive(Clone, PartialEq, Eq)]
pub struct License {
    payload: Payload,
    text: String,
    key_id: String,
}

impl License {
    pub(crate) fn new(payload: Payload, text: String, key_id: String) -> Self {
        Self {
            payload,
            text,
            key_id,
        }
    }

    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn id(&self) -> Uuid {
        self.payload.id
    }

    pub fn customer(&self) -> &str {
        &self.payload.customer
    }

    pub fn tier(&self) -> Tier {
        self.payload.tier
    }

    pub fn seats(&self) -> u32 {
        self.payload.seats
    }

    pub fn issued_at(&self) -> DateTime<Utc> {
        self.payload.issued_at
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.payload.expires_at
    }

    pub fn grace_ends_at(&self) -> DateTime<Utc> {
        self.payload
            .expires_at
            .checked_add_signed(status::grace())
            .unwrap_or(DateTime::<Utc>::MAX_UTC)
    }

    pub fn status(&self, now: DateTime<Utc>) -> Status {
        status::status(Some(self), now)
    }
}

impl fmt::Debug for License {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("License")
            .field("payload", &self.payload)
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::fixtures;

    #[test]
    fn exposes_the_payload_fields() {
        let payload = fixtures::payload();
        let license = fixtures::license(&payload);
        assert_eq!(license.id(), payload.id);
        assert_eq!(license.customer(), payload.customer);
        assert_eq!(license.tier(), payload.tier);
        assert_eq!(license.seats(), payload.seats);
        assert_eq!(license.issued_at(), payload.issued_at);
        assert_eq!(license.expires_at(), payload.expires_at);
        assert_eq!(license.key_id(), payload.key_id);
    }

    #[test]
    fn grace_ends_thirty_days_after_expiry() {
        let license = fixtures::license(&fixtures::payload());
        assert_eq!(
            license.grace_ends_at(),
            license.expires_at() + TimeDelta::days(30)
        );
        assert_eq!(
            license.grace_ends_at().to_rfc3339(),
            "2027-12-01T00:00:00+00:00"
        );
    }

    #[test]
    fn status_matches_the_free_function() {
        let license = fixtures::license(&fixtures::payload());
        for now in [
            license.issued_at(),
            license.expires_at(),
            license.grace_ends_at(),
        ] {
            assert_eq!(license.status(now), status::status(Some(&license), now));
        }
    }

    #[test]
    fn debug_leaves_out_the_key_text() {
        let license = fixtures::license(&fixtures::payload());
        let debug = format!("{license:?}");
        assert!(
            !debug.contains(license.text()),
            "debug output leaked the key: {debug}"
        );
        assert!(debug.contains("Acme Ltd"));
    }
}
