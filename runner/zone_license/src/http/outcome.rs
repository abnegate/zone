use std::fmt;

use axum::http::StatusCode;

use crate::order_error::OrderError;

pub const SENT: &str = "sent";
pub const IGNORED: &str = "ignored";
pub const UNSIGNED: &str = "unsigned";
pub const MALFORMED: &str = "malformed";
pub const MISCONFIGURED: &str = "misconfigured";
pub const UNSIGNABLE: &str = "unsignable";
pub const UNDELIVERED: &str = "undelivered";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    Ignored,
    Unsigned,
    Malformed,
    Misconfigured,
    Unsignable,
    Undelivered,
}

impl Outcome {
    pub fn status(self) -> StatusCode {
        match self {
            Self::Sent | Self::Ignored => StatusCode::ACCEPTED,
            Self::Unsigned => StatusCode::UNAUTHORIZED,
            Self::Malformed | Self::Misconfigured | Self::Unsignable => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            Self::Undelivered => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sent => SENT,
            Self::Ignored => IGNORED,
            Self::Unsigned => UNSIGNED,
            Self::Malformed => MALFORMED,
            Self::Misconfigured => MISCONFIGURED,
            Self::Unsignable => UNSIGNABLE,
            Self::Undelivered => UNDELIVERED,
        }
    }
}

impl From<&OrderError> for Outcome {
    fn from(error: &OrderError) -> Self {
        match error {
            OrderError::Malformed | OrderError::Email => Self::Malformed,
            OrderError::Misconfigured { .. } => Self::Misconfigured,
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
        for unprocessable in [
            Outcome::Malformed,
            Outcome::Misconfigured,
            Outcome::Unsignable,
        ] {
            assert_eq!(
                unprocessable.status(),
                StatusCode::UNPROCESSABLE_ENTITY,
                "{unprocessable}"
            );
        }
        assert_eq!(
            Outcome::Undelivered.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn a_listed_product_without_seats_is_a_misconfiguration() {
        let error = OrderError::Misconfigured {
            order: "order".into(),
            product: "product".into(),
        };
        assert_eq!(Outcome::from(&error), Outcome::Misconfigured);
        assert_eq!(Outcome::from(&OrderError::Malformed), Outcome::Malformed);
    }
}
