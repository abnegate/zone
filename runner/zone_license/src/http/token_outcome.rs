use std::fmt;

use axum::http::StatusCode;

use crate::gate::Refusal;

pub const ISSUED: &str = "issued";
pub const DISABLED: &str = "disabled";
pub const UNREADABLE: &str = "unreadable";
pub const LIMITED: &str = "limited";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenOutcome {
    Issued,
    Disabled,
    Unreadable,
    Refused(Refusal),
    Limited,
}

impl TokenOutcome {
    pub const ALL: [Self; 11] = [
        Self::Issued,
        Self::Disabled,
        Self::Unreadable,
        Self::Refused(Refusal::Anonymous),
        Self::Refused(Refusal::Malformed),
        Self::Refused(Refusal::Unverified),
        Self::Refused(Refusal::Mismatched),
        Self::Refused(Refusal::Revoked),
        Self::Refused(Refusal::Ineligible),
        Self::Refused(Refusal::Lapsed),
        Self::Limited,
    ];

    pub fn status(self) -> StatusCode {
        match self {
            Self::Issued => StatusCode::OK,
            Self::Disabled => StatusCode::NOT_FOUND,
            Self::Unreadable => StatusCode::BAD_REQUEST,
            Self::Refused(refusal) => refusal.status(),
            Self::Limited => StatusCode::TOO_MANY_REQUESTS,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issued => ISSUED,
            Self::Disabled => DISABLED,
            Self::Unreadable => UNREADABLE,
            Self::Refused(refusal) => refusal.as_str(),
            Self::Limited => LIMITED,
        }
    }
}

impl From<Refusal> for TokenOutcome {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

impl fmt::Display for TokenOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests;
