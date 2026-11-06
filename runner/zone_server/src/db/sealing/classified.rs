use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_secret::decrypt_legacy_value;
use abnegate_secret::decrypt_value;
use abnegate_secret::is_encrypted;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::class::Class;

/// The text every envelope, of any version, starts with.
pub const ANY_ENVELOPE_PREFIX: &str = "ENC[v";

/// The fewest bytes a sealed body decodes to: a 12-byte nonce and a 16-byte tag.
pub const LEGACY_MINIMUM_BYTES: usize = 28;

/// A stored credential's class, with the credential it holds when sealing would write one.
pub struct Classified {
    class: Class,
    credential: Option<SecretValue>,
}

impl Classified {
    /// Classify `stored` under `key`.
    pub fn of(stored: &str, key: &MasterKey) -> Self {
        if stored.is_empty() {
            return Self::bare(Class::Empty);
        }
        if is_encrypted(stored) {
            return match decrypt_value(stored, key) {
                Ok(_) => Self::bare(Class::Envelope),
                Err(_) => Self::bare(Class::Unopenable),
            };
        }
        if let Ok(credential) = decrypt_legacy_value(stored, key) {
            return Self {
                class: Class::Legacy,
                credential: Some(credential),
            };
        }
        if stored.starts_with(ANY_ENVELOPE_PREFIX) || legacy_shaped(stored) {
            return Self::bare(Class::Ambiguous);
        }
        Self {
            class: Class::Plaintext,
            credential: Some(SecretValue::new(stored)),
        }
    }

    /// The class.
    pub fn class(&self) -> Class {
        self.class
    }

    /// The credential to seal: the opened value of a legacy body, or plaintext as stored.
    pub fn credential(&self) -> Option<&SecretValue> {
        self.credential.as_ref()
    }

    fn bare(class: Class) -> Self {
        Self {
            class,
            credential: None,
        }
    }
}

/// Whether `stored` could be a bare sealed body: padded standard base64 that decodes to at
/// least [`LEGACY_MINIMUM_BYTES`].
pub fn legacy_shaped(stored: &str) -> bool {
    STANDARD
        .decode(stored)
        .is_ok_and(|bytes| bytes.len() >= LEGACY_MINIMUM_BYTES)
}

#[cfg(test)]
mod tests {
    use abnegate_secret::MasterKey;
    use abnegate_secret::SecretValue;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    use crate::secret::fixture::seal;
    use crate::secret::fixture::seal_legacy;

    use super::Class;
    use super::Classified;
    use super::LEGACY_MINIMUM_BYTES;
    use super::legacy_shaped;

    fn key() -> MasterKey {
        MasterKey::generate().expect("a key")
    }

    #[test]
    fn legacy_shape_starts_at_a_nonce_and_a_tag() {
        assert!(!legacy_shaped(
            &STANDARD.encode([7_u8; LEGACY_MINIMUM_BYTES - 1])
        ));
        assert!(legacy_shaped(
            &STANDARD.encode([7_u8; LEGACY_MINIMUM_BYTES])
        ));
        assert!(!legacy_shaped(
            "ghp_not-base64-at-all-but-long-enough-to-count"
        ));
        assert!(!legacy_shaped(""));
    }

    #[test]
    fn every_class_is_told_apart() {
        let key = key();
        let other = self::key();

        let cases = [
            (String::new(), Class::Empty),
            (seal("ghp_token", &key), Class::Envelope),
            (seal("ghp_token", &other), Class::Unopenable),
            ("ENC[v2:AAAA]".to_string(), Class::Unopenable),
            (seal_legacy("ghp_token", &key), Class::Legacy),
            ("ghp_plaintext_token".to_string(), Class::Plaintext),
            ("ENC[v1:abc".to_string(), Class::Ambiguous),
            (seal_legacy("ghp_token", &other), Class::Ambiguous),
        ];
        for (stored, expected) in cases {
            assert_eq!(Classified::of(&stored, &key).class(), expected, "{stored}");
        }
    }

    #[test]
    fn legacy_and_plaintext_carry_the_credential_to_seal() {
        let key = key();
        let opened = Classified::of(&seal_legacy("ghp_legacy", &key), &key);
        assert_eq!(
            opened.credential().map(SecretValue::expose),
            Some("ghp_legacy")
        );
        let plaintext = Classified::of("ghp_plaintext", &key);
        assert_eq!(
            plaintext.credential().map(SecretValue::expose),
            Some("ghp_plaintext")
        );
        assert!(
            Classified::of(&seal("ghp_token", &key), &key)
                .credential()
                .is_none()
        );
    }
}
