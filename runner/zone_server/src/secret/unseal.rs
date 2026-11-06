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
