use std::fmt;

use axum::http::StatusCode;

pub const SENT: &str = "sent";
pub const IGNORED: &str = "ignored";
pub const UNSIGNED: &str = "unsigned";
pub const MALFORMED: &str = "malformed";
pub const UNSIGNABLE: &str = "unsignable";
pub const UNDELIVERED: &str = "undelivered";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    Ignored,
    Unsigned,
    Malformed,
    Unsignable,
    Undelivered,
}

impl Outcome {
    pub fn status(self) -> StatusCode {
        match self {
            Self::Sent | Self::Ignored => StatusCode::ACCEPTED,
            Self::Unsigned => StatusCode::UNAUTHORIZED,
            Self::Malformed | Self::Unsignable => StatusCode::UNPROCESSABLE_ENTITY,
            Self::Undelivered => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sent => SENT,
            Self::Ignored => IGNORED,
            Self::Unsigned => UNSIGNED,
            Self::Malformed => MALFORMED,
            Self::Unsignable => UNSIGNABLE,
            Self::Undelivered => UNDELIVERED,
        }
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_handled_delivery_is_a_success() {
        assert_eq!(Outcome::Sent.status(), StatusCode::ACCEPTED);
        assert_eq!(Outcome::Ignored.status(), StatusCode::ACCEPTED);
        assert_eq!(Outcome::Unsigned.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            Outcome::Malformed.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            Outcome::Unsignable.status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            Outcome::Undelivered.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
