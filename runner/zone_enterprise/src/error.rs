pub const MALFORMED: &str = "license_malformed";
pub const UNKNOWN_KEY: &str = "license_unknown_key";
pub const SIGNATURE_INVALID: &str = "license_signature_invalid";
pub const PAYLOAD_INVALID: &str = "license_payload_invalid";
pub const KEY_INVALID: &str = "license_key_invalid";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("The license key is not in the expected format.")]
    Malformed,
    #[error("The license key was signed with a key this server does not trust ({id}).")]
    UnknownKey { id: String },
    #[error("The license key's signature is invalid.")]
    Signature,
    #[error("The license key's contents are invalid: {0}")]
    Payload(String),
    #[error("The trusted public key is not a valid, strong Ed25519 key.")]
    Key,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Malformed => MALFORMED,
            Self::UnknownKey { .. } => UNKNOWN_KEY,
            Self::Signature => SIGNATURE_INVALID,
            Self::Payload(_) => PAYLOAD_INVALID,
            Self::Key => KEY_INVALID,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_returns_its_code() {
        assert_eq!(Error::Malformed.code(), MALFORMED);
        assert_eq!(
            Error::UnknownKey {
                id: "2026-1".into()
            }
            .code(),
            UNKNOWN_KEY
        );
        assert_eq!(Error::Signature.code(), SIGNATURE_INVALID);
        assert_eq!(Error::Payload("seats".into()).code(), PAYLOAD_INVALID);
        assert_eq!(Error::Key.code(), KEY_INVALID);
    }

    #[test]
    fn codes_are_the_wire_contract() {
        assert_eq!(MALFORMED, "license_malformed");
        assert_eq!(UNKNOWN_KEY, "license_unknown_key");
        assert_eq!(SIGNATURE_INVALID, "license_signature_invalid");
        assert_eq!(PAYLOAD_INVALID, "license_payload_invalid");
    }

    #[test]
    fn unknown_key_message_names_the_key() {
        let message = Error::UnknownKey {
            id: "2031-4".into(),
        }
        .to_string();
        assert!(message.contains("2031-4"), "message was {message}");
    }
}
