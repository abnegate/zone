//! What Claude's token endpoint grants.

use chrono::{DateTime, TimeDelta, Utc};
use serde::Deserialize;
use zone_core::secret::SecretValue;

use super::super::{Error, Tokens};
use super::UNREADABLE;

#[derive(Deserialize)]
pub(super) struct Granted {
    access_token: SecretValue,
    refresh_token: Option<SecretValue>,
    expires_in: u64,
    scope: Option<String>,
    subscription_type: Option<String>,
}

impl Granted {
    pub(super) fn tokens(self, now: DateTime<Utc>) -> Result<Tokens, Error> {
        let expires_at = i64::try_from(self.expires_in)
            .ok()
            .and_then(TimeDelta::try_seconds)
            .and_then(|lifetime| now.checked_add_signed(lifetime))
            .ok_or(Error::Malformed(UNREADABLE))?;
        if self.access_token.is_empty() {
            return Err(Error::Malformed(UNREADABLE));
        }
        Ok(Tokens {
            access: self.access_token,
            refresh: self.refresh_token.filter(|token| !token.is_empty()),
            expires_at,
            issued_at: Some(now),
            scope: self.scope.unwrap_or_default(),
            subscription: self.subscription_type.filter(|kind| !kind.is_empty()),
        })
    }
}
