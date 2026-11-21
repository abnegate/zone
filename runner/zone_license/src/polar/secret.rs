use std::fmt;

use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig};

const PREFIX: &str = "whsec_";
const REDACTED: &str = "[redacted]";

const LENIENT: GeneralPurpose = GeneralPurpose::new(
    &STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

#[derive(Clone)]
pub struct WebhookSecret {
    keys: Vec<Vec<u8>>,
}

impl WebhookSecret {
    pub fn new(secret: &str) -> Self {
        if secret.is_empty() {
            return Self { keys: Vec::new() };
        }
        let whole = secret.as_bytes().to_vec();
        let remainder = secret.strip_prefix(PREFIX).unwrap_or(secret);
        let decoded = LENIENT
            .decode(remainder)
            .ok()
            .filter(|decoded| !decoded.is_empty() && *decoded != whole);
        let mut keys = vec![whole];
        keys.extend(decoded);
        Self { keys }
    }

    pub(crate) fn keys(&self) -> &[Vec<u8>] {
        &self.keys
    }
}

impl fmt::Debug for WebhookSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebhookSecret")
            .field("keys", &REDACTED)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD as ENCODER;

    use super::*;

    #[test]
    fn legacy_secret_keys_on_the_whole_string() {
        let secret = WebhookSecret::new("whsec_legacy-polar-secret!");
        assert_eq!(secret.keys(), [b"whsec_legacy-polar-secret!".to_vec()]);
    }

    #[test]
    fn standard_secret_also_keys_on_the_decoded_remainder() {
        let raw = [7u8; 32];
        let text = format!("{PREFIX}{}", ENCODER.encode(raw));
        let secret = WebhookSecret::new(&text);
        assert_eq!(secret.keys(), [text.as_bytes().to_vec(), raw.to_vec()]);
    }

    #[test]
    fn decodes_a_remainder_without_padding() {
        let raw = [1u8, 2, 3, 4];
        let text = format!("{PREFIX}{}", ENCODER.encode(raw).trim_end_matches('='));
        let secret = WebhookSecret::new(&text);
        assert_eq!(secret.keys()[1], raw);
    }

    #[test]
    fn decodes_a_secret_without_the_prefix() {
        let raw = [9u8; 32];
        let text = ENCODER.encode(raw);
        let secret = WebhookSecret::new(&text);
        assert_eq!(secret.keys(), [text.as_bytes().to_vec(), raw.to_vec()]);
    }

    #[test]
    fn empty_secret_has_no_keys() {
        assert!(WebhookSecret::new("").keys().is_empty());
    }

    #[test]
    fn debug_redacts_the_keys() {
        let rendered = format!("{:?}", WebhookSecret::new("whsec_c2VjcmV0"));
        assert_eq!(rendered, "WebhookSecret { keys: \"[redacted]\" }");
    }
}
