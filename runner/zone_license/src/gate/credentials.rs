use std::fmt;

use axum::http::HeaderMap;
use axum::http::header::AUTHORIZATION;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::gate::refusal::Refusal;

pub const SCHEME: &str = "Basic";

pub struct Credentials {
    user: String,
    password: String,
}

impl Credentials {
    pub fn from_headers(headers: &HeaderMap) -> Result<Self, Refusal> {
        let mut values = headers.get_all(AUTHORIZATION).iter();
        let Some(value) = values.next() else {
            return Err(Refusal::Anonymous);
        };
        if values.next().is_some() {
            return Err(Refusal::Malformed);
        }
        Self::parse(value.as_bytes()).ok_or(Refusal::Malformed)
    }

    fn parse(value: &[u8]) -> Option<Self> {
        let (scheme, encoded) = std::str::from_utf8(value).ok()?.trim().split_once(' ')?;
        if !scheme.eq_ignore_ascii_case(SCHEME) {
            return None;
        }
        let decoded = String::from_utf8(STANDARD.decode(encoded.trim()).ok()?).ok()?;
        let (user, password) = decoded.split_once(':')?;
        Some(Self {
            user: user.to_owned(),
            password: password.to_owned(),
        })
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("user", &self.user)
            .field("password", &"[redacted]")
            .finish()
    }
}
