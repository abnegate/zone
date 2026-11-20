use chrono::{DateTime, SubsecRound, Utc};

use crate::license_id;
use crate::terms::Terms;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub order: String,
    pub subscription: String,
    pub customer: String,
    pub email: String,
    pub seats: u32,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
}

impl Grant {
    /// Every term comes from the subscription period, so a retried or
    /// redelivered order signs to the same bytes. The period is cut to whole
    /// seconds, the precision the license id hashes.
    pub fn terms(&self) -> Terms {
        let expires_at = self.period_end.trunc_subsecs(0);
        Terms {
            id: license_id::derive(&self.subscription, expires_at, self.seats),
            customer: self.customer.clone(),
            seats: self.seats,
            issued_at: self.period_start.trunc_subsecs(0),
            expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text).unwrap().to_utc()
    }

    fn grant() -> Grant {
        Grant {
            order: "order".into(),
            subscription: "sub_1".into(),
            customer: "Acme Ltd".into(),
            email: "billing@acme.test".into(),
            seats: 5,
            period_start: at("2026-11-18T11:59:58Z"),
            period_end: at("2027-11-18T11:59:58Z"),
        }
    }

    #[test]
    fn the_terms_cover_the_subscription_period_with_no_grace() {
        let terms = grant().terms();
        assert_eq!(
            terms.id,
            license_id::derive("sub_1", at("2027-11-18T11:59:58Z"), 5)
        );
        assert_eq!(terms.customer, "Acme Ltd");
        assert_eq!(terms.seats, 5);
        assert_eq!(terms.issued_at, at("2026-11-18T11:59:58Z"));
        assert_eq!(terms.expires_at, at("2027-11-18T11:59:58Z"));
    }

    #[test]
    fn the_terms_ignore_the_order_and_the_address() {
        let other = Grant {
            order: "another order".into(),
            email: "someone@acme.test".into(),
            ..grant()
        };
        assert_eq!(other.terms(), grant().terms());
    }

    #[test]
    fn the_terms_keep_whole_seconds_like_the_id() {
        let precise = Grant {
            period_start: at("2026-11-18T11:59:58.123456Z"),
            period_end: at("2027-11-18T11:59:58.654321Z"),
            ..grant()
        };
        assert_eq!(precise.terms(), grant().terms());
    }

    #[test]
    fn a_seat_change_gets_a_new_id() {
        let more = Grant {
            seats: 8,
            ..grant()
        };
        assert_ne!(more.terms().id, grant().terms().id);
    }
}
