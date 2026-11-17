use chrono::{TimeZone, Utc};
use ed25519_dalek::{SECRET_KEY_LENGTH, SigningKey};
use uuid::Uuid;

use crate::keyring::Keyring;
use crate::license::License;
use crate::payload::Payload;
use crate::signing::sign;
use crate::tier::Tier;
use crate::trusted_key::TrustedKey;

pub(crate) const FIRST: &str = "test-1";
pub(crate) const SECOND: &str = "test-2";

const FIRST_SEED: [u8; SECRET_KEY_LENGTH] = [0x11; SECRET_KEY_LENGTH];
const SECOND_SEED: [u8; SECRET_KEY_LENGTH] = [0x22; SECRET_KEY_LENGTH];

pub(crate) fn first() -> SigningKey {
    SigningKey::from_bytes(&FIRST_SEED)
}

pub(crate) fn second() -> SigningKey {
    SigningKey::from_bytes(&SECOND_SEED)
}

pub(crate) fn trusted(id: &str, key: &SigningKey) -> TrustedKey {
    TrustedKey::new(id, key.verifying_key().to_bytes()).unwrap()
}

pub(crate) fn keyring() -> Keyring {
    Keyring::new([trusted(FIRST, &first()), trusted(SECOND, &second())]).unwrap()
}

pub(crate) fn payload() -> Payload {
    Payload {
        id: Uuid::parse_str("0193a8e2-7c4b-7d10-8f00-4d2c5a9b1e01").unwrap(),
        customer: "Acme Ltd".into(),
        tier: Tier::Team,
        seats: 25,
        issued_at: Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap(),
        expires_at: Utc.with_ymd_and_hms(2027, 11, 1, 0, 0, 0).unwrap(),
        key_id: FIRST.into(),
    }
}

pub(crate) fn license(payload: &Payload) -> License {
    let key = if payload.key_id == SECOND {
        second()
    } else {
        first()
    };
    keyring().verify(&sign(payload, &key)).unwrap()
}
