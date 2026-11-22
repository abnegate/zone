use std::fmt;

use axum::http::StatusCode;

pub const ISSUED: &str = "issued";
pub const DISABLED: &str = "disabled";
pub const UNKNOWN: &str = "unknown";
pub const LIMITED: &str = "limited";
pub const UNAVAILABLE: &str = "unavailable";

/// What a `/bundles` request came to, besides the gate's shared refusals,
/// which log their own names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleOutcome {
    Issued,
    Disabled,
    Unknown,
    Limited,
    Unavailable,
}

impl BundleOutcome {
    pub const ALL: [Self; 5] = [
        Self::Issued,
        Self::Disabled,
        Self::Unknown,
        Self::Limited,
        Self::Unavailable,
    ];

    pub fn status(self) -> StatusCode {
        match self {
            Self::Issued => StatusCode::OK,
            Self::Disabled | Self::Unknown => StatusCode::NOT_FOUND,
            Self::Limited => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Issued => ISSUED,
            Self::Disabled => DISABLED,
            Self::Unknown => UNKNOWN,
            Self::Limited => LIMITED,
            Self::Unavailable => UNAVAILABLE,
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
