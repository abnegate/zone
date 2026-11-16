use ed25519_dalek::{PUBLIC_KEY_LENGTH, VerifyingKey};

use crate::error::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedKey {
    id: String,
    key: VerifyingKey,
}

impl TrustedKey {
    pub fn new(id: impl Into<String>, public: [u8; PUBLIC_KEY_LENGTH]) -> Result<Self, Error> {
        let key = VerifyingKey::from_bytes(&public).map_err(|_| Error::Key)?;
        if key.is_weak() {
            return Err(Error::Key);
        }
        Ok(Self { id: id.into(), key })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn bytes(&self) -> [u8; PUBLIC_KEY_LENGTH] {
        self.key.to_bytes()
    }

    pub(crate) fn key(&self) -> &VerifyingKey {
        &self.key
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    const IDENTITY: [u8; PUBLIC_KEY_LENGTH] = {
        let mut bytes = [0; PUBLIC_KEY_LENGTH];
        bytes[0] = 1;
        bytes
    };

    const NOT_A_POINT: [u8; PUBLIC_KEY_LENGTH] = {
        let mut bytes = [0; PUBLIC_KEY_LENGTH];
        bytes[0] = 2;
        bytes
    };

    #[test]
    fn accepts_a_valid_key() {
        let public = fixtures::first().verifying_key().to_bytes();
        let trusted = TrustedKey::new(fixtures::FIRST, public).unwrap();
        assert_eq!(trusted.id(), fixtures::FIRST);
        assert_eq!(trusted.bytes(), public);
    }

    #[test]
    fn refuses_bytes_that_are_not_a_point() {
        assert!(VerifyingKey::from_bytes(&NOT_A_POINT).is_err());
        assert_eq!(TrustedKey::new("broken", NOT_A_POINT), Err(Error::Key));
    }

    #[test]
    fn refuses_the_identity_point_as_weak() {
        assert!(VerifyingKey::from_bytes(&IDENTITY).unwrap().is_weak());
        assert_eq!(TrustedKey::new("weak", IDENTITY), Err(Error::Key));
    }

    #[test]
    fn refuses_a_small_order_point_as_weak() {
        let mut negative_identity = [0xff; PUBLIC_KEY_LENGTH];
        negative_identity[0] = 0xec;
        negative_identity[PUBLIC_KEY_LENGTH - 1] = 0x7f;
        assert!(
            VerifyingKey::from_bytes(&negative_identity)
                .unwrap()
                .is_weak()
        );
        assert_eq!(TrustedKey::new("weak", negative_identity), Err(Error::Key));
    }
}
