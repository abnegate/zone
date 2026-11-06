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

    /// The credential `stored` seals, or `stored` itself when it is not an envelope and does
    /// not open as a value sealed before envelopes existed. An envelope that does not open is
    /// an error: no credential is ever written in that shape, so handing it on as one only
    /// moves the failure somewhere it is harder to read.
    fn open_lenient(&self, stored: &str) -> Result<SecretValue, Error>;
}

impl Unseal for MasterKey {
    fn open_strict(&self, stored: &str) -> Result<SecretValue, Error> {
        decrypt_legacy_value(stored, self)
    }

    fn open_lenient(&self, stored: &str) -> Result<SecretValue, Error> {
        match self.open_strict(stored) {
            Ok(opened) => Ok(opened),
            Err(error) if is_encrypted(stored) => {
                tracing::warn!(%error, "A sealed credential did not open under this deployment's key");
                Err(error)
            }
            Err(_) => Ok(SecretValue::new(stored)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::derive;
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
            assert_eq!(
                key.open_lenient(&stored).unwrap().expose(),
                expected,
                "{stored}"
            );
        }
    }

    #[test]
    fn lenient_refuses_an_envelope_the_key_cannot_open() {
        let sealed = fixture::seal(CREDENTIAL, &key());
        assert!(matches!(
            key().open_lenient(&sealed),
            Err(Error::Decryption)
        ));
    }

    #[test]
    fn lenient_hands_back_a_legacy_body_the_key_cannot_open() {
        let sealed = fixture::seal_legacy(CREDENTIAL, &key());
        assert_eq!(key().open_lenient(&sealed).unwrap().expose(), sealed);
    }

    #[test]
    fn strict_opens_what_the_previous_writer_sealed() {
        let key = derive(fixture::PREVIOUS_WRITER_PASSPHRASE).unwrap();
        let opened = key.open_strict(fixture::PREVIOUS_WRITER_SEALED).unwrap();
        assert_eq!(opened.expose(), fixture::PREVIOUS_WRITER_CREDENTIAL);
    }
}
