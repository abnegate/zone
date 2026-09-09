//! The claims of a JWT that authenticates as the App itself.

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use super::identifier::ApplicationId;

/// How far `iat` is backdated.
///
/// GitHub rejects a JWT whose `iat` is in the future, and it compares against
/// its own clock, not ours. A server running a few seconds fast is enough to
/// be refused, so every JWT is issued as of a minute ago.
pub const CLOCK_SKEW: TimeDelta = TimeDelta::seconds(60);

/// How long a signed JWT stays valid.
///
/// GitHub caps `exp - iat` at ten minutes and refuses the request outright
/// when it is exceeded. The bound is measured from the backdated `iat` rather
/// than from now, so backdating cannot eat into it, and nine minutes leaves a
/// minute of headroom against rounding.
pub const LIFETIME: TimeDelta = TimeDelta::minutes(9);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub iat: i64,
    pub exp: i64,
    pub iss: String,
}

impl Claims {
    pub fn issue(application_id: ApplicationId, now: DateTime<Utc>) -> Self {
        let issued_at = now - CLOCK_SKEW;
        Self {
            iat: issued_at.timestamp(),
            exp: (issued_at + LIFETIME).timestamp(),
            iss: application_id.get().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(timestamp: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(timestamp, 0).expect("timestamp in range")
    }

    #[test]
    fn issued_at_is_backdated_for_clock_skew() {
        let now = at(1_700_000_000);
        let claims = Claims::issue(ApplicationId::new(12345), now);

        assert_eq!(claims.iat, now.timestamp() - 60);
        assert!(
            claims.iat < now.timestamp(),
            "a JWT issued in the future is rejected by GitHub"
        );
    }

    #[test]
    fn expiry_is_bounded_by_githubs_ten_minute_cap() {
        let claims = Claims::issue(ApplicationId::new(12345), at(1_700_000_000));

        assert_eq!(claims.exp - claims.iat, 9 * 60);
        assert!(
            claims.exp - claims.iat <= 10 * 60,
            "GitHub refuses a JWT whose lifetime exceeds ten minutes"
        );
    }

    #[test]
    fn expiry_is_still_in_the_future_after_backdating() {
        let now = at(1_700_000_000);
        let claims = Claims::issue(ApplicationId::new(12345), now);

        assert!(claims.exp > now.timestamp());
    }

    #[test]
    fn issuer_is_the_application_id() {
        let claims = Claims::issue(ApplicationId::new(12345), at(1_700_000_000));
        assert_eq!(claims.iss, "12345");
    }
}
