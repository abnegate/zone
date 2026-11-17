pub const LAPSED: &str = "license_lapsed";

#[derive(Debug, thiserror::Error)]
pub enum SetError {
    #[error(transparent)]
    Refused(#[from] zone_enterprise::Error),
    #[error("The license key's grace period has ended. Paste a renewed key.")]
    Lapsed,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl SetError {
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Refused(error) => Some(error.code()),
            Self::Lapsed => Some(LAPSED),
            Self::Database(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use zone_enterprise::{MALFORMED, PAYLOAD_INVALID, SIGNATURE_INVALID, UNKNOWN_KEY};

    use super::*;

    #[test]
    fn lapsed_code_is_the_wire_contract() {
        assert_eq!(LAPSED, "license_lapsed");
    }

    #[test]
    fn each_refusal_returns_its_code() {
        let cases = [
            (
                SetError::Refused(zone_enterprise::Error::Malformed),
                MALFORMED,
            ),
            (
                SetError::Refused(zone_enterprise::Error::UnknownKey { id: "x".into() }),
                UNKNOWN_KEY,
            ),
            (
                SetError::Refused(zone_enterprise::Error::Signature),
                SIGNATURE_INVALID,
            ),
            (
                SetError::Refused(zone_enterprise::Error::Payload("seats".into())),
                PAYLOAD_INVALID,
            ),
            (SetError::Lapsed, LAPSED),
        ];
        for (error, code) in cases {
            assert_eq!(error.code(), Some(code), "{error:?}");
        }
    }

    #[test]
    fn a_database_failure_has_no_refusal_code() {
        assert_eq!(SetError::Database(sqlx::Error::PoolClosed).code(), None);
    }
}
