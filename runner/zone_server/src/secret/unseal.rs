use abnegate_secret::Error;
use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_secret::decrypt_legacy_value;
use abnegate_secret::is_encrypted;

/// How a credential read from storage is opened under the deployment's key.
pub trait Unseal {
    /// The credential `stored` seals, as an envelope or as a value sealed before envelopes
    /// existed. Anything else, plaintext included, is an error.
    fn open_strict(&self, stored: &str) -> Result<SecretValue, Error>;

    /// The credential `stored` seals, or `stored` itself when it opens as neither.
    fn open_lenient(&self, stored: &str) -> SecretValue;
}

impl Unseal for MasterKey {
    fn open_strict(&self, stored: &str) -> Result<SecretValue, Error> {
        decrypt_legacy_value(stored, self)
    }

    fn open_lenient(&self, stored: &str) -> SecretValue {
        self.open_strict(stored).unwrap_or_else(|error| {
            if is_encrypted(stored) {
                tracing::warn!(%error, "A sealed credential did not open under this deployment's key");
            }
            SecretValue::new(stored)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::fixture;

    const CREDENTIAL: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
    const PLAINTEXT: &str = "ghp_plaintext_token";
    const BASE64_PLAINTEXT: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    const UNCLOSED: &str = "ENC[v1:abc";

    fn key() -> MasterKey {
        MasterKey::generate().expect("a key")
    }

    #[test]
    fn strict_opens_an_envelope() {
        let key = key();
        let opened = key.open_strict(&fixture::seal(CREDENTIAL, &key)).unwrap();
        assert_eq!(opened.expose(), CREDENTIAL);
    }

    #[test]
    fn strict_opens_a_value_sealed_before_envelopes_existed() {
        let key = key();
        let opened = key
            .open_strict(&fixture::seal_legacy(CREDENTIAL, &key))
            .unwrap();
        assert_eq!(opened.expose(), CREDENTIAL);
    }

    #[test]
    fn strict_refuses_plaintext() {
        assert!(matches!(
            key().open_strict(PLAINTEXT),
            Err(Error::InvalidBase64)
        ));
    }

    #[test]
    fn strict_refuses_plaintext_long_enough_to_be_a_sealed_body() {
        assert!(matches!(
            key().open_strict(BASE64_PLAINTEXT),
            Err(Error::Decryption)
        ));
    }

    #[test]
    fn strict_refuses_an_envelope_sealed_under_another_key() {
        let sealed = fixture::seal(CREDENTIAL, &key());
        assert!(matches!(key().open_strict(&sealed), Err(Error::Decryption)));
    }

    #[test]
    fn strict_refuses_an_envelope_that_is_never_closed() {
        assert!(matches!(key().open_strict(UNCLOSED), Err(Error::Truncated)));
    }

    #[test]
    fn lenient_opens_an_envelope_and_a_legacy_value_and_passes_plaintext_through() {
        let key = key();
        for (stored, expected) in [
            (fixture::seal(CREDENTIAL, &key), CREDENTIAL),
            (fixture::seal_legacy(CREDENTIAL, &key), CREDENTIAL),
            (PLAINTEXT.to_string(), PLAINTEXT),
            (BASE64_PLAINTEXT.to_string(), BASE64_PLAINTEXT),
        ] {
            assert_eq!(key.open_lenient(&stored).expose(), expected, "{stored}");
        }
    }

    #[test]
    fn lenient_hands_back_what_the_wrong_key_cannot_open() {
        let sealed = fixture::seal(CREDENTIAL, &key());
        assert_eq!(key().open_lenient(&sealed).expose(), sealed);
    }
}
