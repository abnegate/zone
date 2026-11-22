use std::fmt;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;

use crate::registry::LIFETIME_SECONDS;

/// Docker reads `token`, containerd and the OAuth2 clients `access_token`, so
/// both carry the same JWT. `issued_at` is whole seconds, like the JWT's `iat`.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct TokenResponse {
    pub token: String,
    pub access_token: String,
    pub expires_in: i64,
    pub issued_at: String,
}

impl TokenResponse {
    pub fn new(token: String, issued_at: DateTime<Utc>) -> Self {
        Self {
            access_token: token.clone(),
            token,
            expires_in: LIFETIME_SECONDS,
            issued_at: issued_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        }
    }
}

impl fmt::Debug for TokenResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenResponse")
            .field("token", &"[redacted]")
            .field("access_token", &"[redacted]")
            .field("expires_in", &self.expires_in)
            .field("issued_at", &self.issued_at)
            .finish()
    }
}
