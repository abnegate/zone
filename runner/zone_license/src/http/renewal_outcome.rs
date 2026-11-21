use std::fmt;

use axum::http::StatusCode;

use crate::polar::ApiError;

pub const RENEWED: &str = "renewed";
pub const CURRENT: &str = "current";
pub const UNREADABLE: &str = "unreadable";
pub const DISABLED: &str = "disabled";
pub const OVERSIZE: &str = "oversize";
pub const UNVERIFIED: &str = "unverified";
pub const UNSUBSCRIBED: &str = "unsubscribed";
pub const MALFORMED: &str = "malformed";
pub const ENDED: &str = "ended";
pub const UNSIGNABLE: &str = "unsignable";
pub const UNAUTHORIZED: &str = "unauthorized";
pub const UNEXPECTED: &str = "unexpected";
pub const MISCONFIGURED: &str = "misconfigured";
pub const UNAVAILABLE: &str = "unavailable";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenewalOutcome {
    Renewed,
    Current,
    Unreadable,
    Disabled,
    Oversize,
    Unverified,
    Unsubscribed,
    Malformed,
    Ended,
    Unsignable,
    Unauthorized,
    Unexpected,
    Misconfigured,
    Unavailable,
}

impl RenewalOutcome {
    pub const ALL: [Self; 14] = [
        Self::Renewed,
        Self::Current,
        Self::Unreadable,
        Self::Disabled,
        Self::Oversize,
        Self::Unverified,
        Self::Unsubscribed,
        Self::Malformed,
        Self::Ended,
        Self::Unsignable,
        Self::Unauthorized,
        Self::Unexpected,
        Self::Misconfigured,
        Self::Unavailable,
    ];

    pub fn status(self) -> StatusCode {
        match self {
            Self::Renewed => StatusCode::OK,
            Self::Current => StatusCode::NO_CONTENT,
            Self::Unreadable => StatusCode::BAD_REQUEST,
            Self::Disabled => StatusCode::NOT_FOUND,
            Self::Oversize => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Unverified | Self::Unsubscribed | Self::Malformed => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::Ended => StatusCode::GONE,
            Self::Unsignable => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unauthorized | Self::Unexpected | Self::Misconfigured => StatusCode::BAD_GATEWAY,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Renewed => RENEWED,
            Self::Current => CURRENT,
            Self::Unreadable => UNREADABLE,
            Self::Disabled => DISABLED,
            Self::Oversize => OVERSIZE,
            Self::Unverified => UNVERIFIED,
            Self::Unsubscribed => UNSUBSCRIBED,
            Self::Malformed => MALFORMED,
            Self::Ended => ENDED,
            Self::Unsignable => UNSIGNABLE,
            Self::Unauthorized => UNAUTHORIZED,
            Self::Unexpected => UNEXPECTED,
            Self::Misconfigured => MISCONFIGURED,
            Self::Unavailable => UNAVAILABLE,
        }
    }

    pub fn from_body(status: StatusCode) -> Self {
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            Self::Oversize
        } else {
            Self::Unreadable
        }
    }
}

impl From<ApiError> for RenewalOutcome {
    fn from(error: ApiError) -> Self {
        match error {
            ApiError::Unavailable => Self::Unavailable,
            ApiError::Refused { status: 401 | 403 } => Self::Unauthorized,
            ApiError::Refused { .. } | ApiError::Malformed => Self::Unexpected,
        }
    }
}

impl fmt::Display for RenewalOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests;
