use zone_enterprise::{Keyring, Payload, SigningKey};

use crate::private_key::PrivateKey;
use crate::terms::Terms;
use crate::untrusted::Untrusted;

pub struct Signer {
    key: SigningKey,
    key_id: String,
}

impl Signer {
    pub fn new(key: PrivateKey, key_id: impl Into<String>) -> Self {
        Self {
            key: key.into_signing_key(),
            key_id: key_id.into(),
        }
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn public(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    pub fn sign(&self, terms: &Terms) -> Result<String, zone_enterprise::Error> {
        let payload = Payload {
            id: terms.id,
            customer: terms.customer.clone(),
            seats: terms.seats,
            issued_at: terms.issued_at,
            expires_at: terms.expires_at,
            key_id: self.key_id.clone(),
        };
        payload.validate()?;
        Ok(zone_enterprise::sign(&payload, &self.key))
    }

    pub fn trusted_by(&self, keyring: &Keyring) -> Result<(), Untrusted> {
        let trusted = keyring
            .get(&self.key_id)
            .ok_or_else(|| Untrusted::UnknownKeyId {
                id: self.key_id.clone(),
            })?;
        if trusted.bytes() != self.public() {
            return Err(Untrusted::PublicKeyMismatch {
                id: self.key_id.clone(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};
    use uuid::Uuid;
    use zone_enterprise::TrustedKey;

    use super::*;

    const KEY_ID: &str = "test-1";
    const SEED: [u8; 32] = [3; 32];

    fn signer() -> Signer {
        Signer::new(PrivateKey::new(SEED), KEY_ID)
    }

    fn keyring(id: &str, public: [u8; 32]) -> Keyring {
        Keyring::new([TrustedKey::new(id, public).unwrap()]).unwrap()
    }

    fn issued_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-01T00:00:00Z")
            .unwrap()
            .to_utc()
    }

    fn terms() -> Terms {
        Terms {
            id: Uuid::from_u128(0x0193_a8e2_7c4b_7d10_8f00_4d2c_5a9b_1e01),
            customer: "Acme Ltd".into(),
            seats: 25,
            issued_at: issued_at(),
            expires_at: issued_at() + TimeDelta::days(365),
        }
    }

    #[test]
    fn exposes_its_key_id_and_public_key() {
        let signer = signer();
        assert_eq!(signer.key_id(), KEY_ID);
        assert_eq!(signer.public(), PrivateKey::new(SEED).public());
    }

    #[test]
    fn a_signed_license_verifies_and_carries_the_terms() {
        let signer = signer();
        let text = signer.sign(&terms()).unwrap();
        let license = keyring(KEY_ID, signer.public()).verify(&text).unwrap();
        let payload = license.payload();
        let terms = terms();
        assert_eq!(payload.id, terms.id);
        assert_eq!(payload.customer, terms.customer);
        assert_eq!(payload.seats, terms.seats);
        assert_eq!(payload.issued_at, terms.issued_at);
        assert_eq!(payload.expires_at, terms.expires_at);
        assert_eq!(payload.key_id, KEY_ID);
        assert_eq!(license.key_id(), KEY_ID);
        assert_eq!(license.text(), text);
    }

    #[test]
    fn identical_terms_sign_identically() {
        assert_eq!(
            signer().sign(&terms()).unwrap(),
            signer().sign(&terms()).unwrap()
        );
    }

    #[test]
    fn different_terms_sign_differently() {
        let other = Terms {
            seats: 26,
            ..terms()
        };
        assert_ne!(
            signer().sign(&terms()).unwrap(),
            signer().sign(&other).unwrap()
        );
    }

    #[test]
    fn refuses_zero_seats() {
        let error = signer()
            .sign(&Terms {
                seats: 0,
                ..terms()
            })
            .unwrap_err();
        assert!(
            matches!(error, zone_enterprise::Error::Payload(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn refuses_expiry_at_or_before_issue() {
        for expires_at in [issued_at(), issued_at() - TimeDelta::seconds(1)] {
            let error = signer()
                .sign(&Terms {
                    expires_at,
                    ..terms()
                })
                .unwrap_err();
            assert!(
                matches!(error, zone_enterprise::Error::Payload(_)),
                "accepted {expires_at}"
            );
        }
    }

    #[test]
    fn refuses_a_blank_customer() {
        let error = signer()
            .sign(&Terms {
                customer: " ".into(),
                ..terms()
            })
            .unwrap_err();
        assert!(
            matches!(error, zone_enterprise::Error::Payload(_)),
            "got {error:?}"
        );
    }

    #[test]
    fn is_trusted_by_a_keyring_holding_its_public_key() {
        let signer = signer();
        assert_eq!(signer.trusted_by(&keyring(KEY_ID, signer.public())), Ok(()));
    }

    #[test]
    fn reports_an_unknown_key_id() {
        let signer = signer();
        assert_eq!(
            signer.trusted_by(&keyring("test-2", signer.public())),
            Err(Untrusted::UnknownKeyId { id: KEY_ID.into() })
        );
    }

    #[test]
    fn reports_a_public_key_mismatch() {
        let other = PrivateKey::new([4; 32]).public();
        assert_eq!(
            signer().trusted_by(&keyring(KEY_ID, other)),
            Err(Untrusted::PublicKeyMismatch { id: KEY_ID.into() })
        );
    }

    #[test]
    fn a_test_key_is_not_trusted_by_production() {
        let signer = Signer::new(PrivateKey::new(SEED), "2026-1");
        assert_eq!(
            signer.trusted_by(&Keyring::production()),
            Err(Untrusted::PublicKeyMismatch {
                id: "2026-1".into()
            })
        );
    }
}
