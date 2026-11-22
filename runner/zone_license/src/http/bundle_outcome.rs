use std::fmt;

use axum::http::StatusCode;

use crate::gate::UNADDRESSABLE;

pub const ISSUED: &str = "issued";
pub const DISABLED: &str = "disabled";
pub const UNKNOWN: &str = "unknown";
pub const LIMITED: &str = "limited";
pub const UNAVAILABLE: &str = "unavailable";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleOutcome {
    Issued,
    Disabled,
    Unknown,
    Limited,
    Unavailable,
    Unaddressable,
}

impl BundleOutcome {
    pub const ALL: [Self; 6] = [
        Self::Issued,
        Self::Disabled,
        Self::Unknown,
        Self::Limited,
        Self::Unavailable,
        Self::Unaddressable,
    ];

    pub fn status(self) -> StatusCode {
        match self {
            Self::Issued => StatusCode::OK,
            Self::Disabled | Self::Unknown => StatusCode::NOT_FOUND,
            Self::Limited => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Unaddressable => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issued => ISSUED,
            Self::Disabled => DISABLED,
            Self::Unknown => UNKNOWN,
            Self::Limited => LIMITED,
            Self::Unavailable => UNAVAILABLE,
            Self::Unaddressable => UNADDRESSABLE,
        }
    }
}

impl fmt::Display for BundleOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests;
