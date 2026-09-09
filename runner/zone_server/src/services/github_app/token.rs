//! An installation access token and the window it may be used in.

use chrono::{DateTime, TimeDelta, Utc};
use serde::Deserialize;
use zone_core::secret::SecretValue;

use super::error::GithubAppError;

/// How long before expiry a token stops being handed out.
///
/// GitHub issues installation tokens with an hour of life. Presenting one at
/// 59:59 races the round trip and both clocks, and the request that loses that
/// race fails as a `401` in the middle of somebody's work. The last five
/// minutes are treated as already spent instead.
pub const SAFETY_MARGIN: TimeDelta = TimeDelta::minutes(5);

#[derive(Debug, Clone)]
pub struct InstallationToken {
    token: SecretValue,
    expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct Payload {
    token: SecretValue,
    expires_at: DateTime<Utc>,
}

impl InstallationToken {
    pub fn new(token: SecretValue, expires_at: DateTime<Utc>) -> Self {
        Self { token, expires_at }
    }

    /// Read GitHub's `access_tokens` response body.
    ///
    /// The parse error is discarded: the body that failed to parse is the body
    /// that may hold the token.
    pub fn parse(body: &str) -> Result<Self, GithubAppError> {
        let payload: Payload = serde_json::from_str(body).map_err(|_| GithubAppError::Response)?;

        Ok(Self::new(payload.token, payload.expires_at))
    }

    pub fn secret(&self) -> &SecretValue {
        &self.token
    }

    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// Whether this token can still be used at `now`, [`SAFETY_MARGIN`] aside.
    pub fn is_usable_at(&self, now: DateTime<Utc>) -> bool {
        now + SAFETY_MARGIN < self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::github_app::testing::at;

    fn expiring_in(minutes: i64) -> InstallationToken {
        InstallationToken::new(
            SecretValue::new("ghs_installationtokenvalue"),
            at(1_700_000_000) + TimeDelta::minutes(minutes),
        )
    }

    #[test]
    fn a_fresh_token_is_usable() {
        assert!(expiring_in(60).is_usable_at(at(1_700_000_000)));
    }

    #[test]
    fn a_token_inside_the_safety_margin_is_spent() {
        let now = at(1_700_000_000);

        assert!(
            expiring_in(6).is_usable_at(now),
            "six minutes of life is outside the margin"
        );
        assert!(
            !expiring_in(4).is_usable_at(now),
            "four minutes of life is inside the margin and must not be used"
        );
        assert!(
            !expiring_in(5).is_usable_at(now),
            "the margin itself is not usable time"
        );
    }

    #[test]
    fn an_expired_token_is_spent() {
        assert!(!expiring_in(-1).is_usable_at(at(1_700_000_000)));
    }

    #[test]
    fn a_response_body_parses_into_a_token_and_an_expiry() {
        let token = InstallationToken::parse(
            r#"{"token":"ghs_installationtokenvalue","expires_at":"2026-01-15T12:00:00Z"}"#,
        )
        .expect("parse");

        assert_eq!(token.secret().expose(), "ghs_installationtokenvalue");
        assert_eq!(token.expires_at().to_rfc3339(), "2026-01-15T12:00:00+00:00");
    }

    #[test]
    fn a_malformed_response_does_not_echo_the_body() {
        let error = InstallationToken::parse(r#"{"token":"ghs_leaky","expires_at":"soon"}"#)
            .expect_err("an unparseable expiry is a failure");

        assert!(matches!(error, GithubAppError::Response));
        assert!(!error.to_string().contains("ghs_leaky"));
    }

    #[test]
    fn the_token_never_renders_itself() {
        let token = expiring_in(60);
        let rendered = format!("{token:?}");

        assert!(!rendered.contains("ghs_installationtokenvalue"));
        assert!(rendered.contains("[REDACTED]"));
    }
}
