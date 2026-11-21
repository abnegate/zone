use uuid::Uuid;
use zone_enterprise::{Keyring, Payload, SigningKey, TrustedKey, sign};

use super::*;

const KEY_ID: &str = "test-1";
const SEED: [u8; 32] = [0x11; 32];

fn license(subscription: Option<&str>) -> License {
    let signing = SigningKey::from_bytes(&SEED);
    let keyring =
        Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
            .unwrap();
    let text = sign(
        &Payload {
            id: Uuid::new_v4(),
            customer: "Acme Ltd".into(),
            seats: 3,
            issued_at: "2026-11-01T00:00:00Z".parse().unwrap(),
            expires_at: "2027-11-01T00:00:00Z".parse().unwrap(),
            key_id: KEY_ID.into(),
            subscription: subscription.map(str::to_owned),
            plan: None,
        },
        &signing,
    );
    keyring.verify(&text).unwrap()
}

fn subscribed() -> License {
    license(Some("4f6c1a2e-8b3d-4c5e-9f70-1a2b3c4d5e6f"))
}

#[test]
fn a_subscribed_license_renews_automatically_while_renewal_is_enabled() {
    assert_eq!(Mode::of(&subscribed(), true), Mode::Automatic);
}

#[test]
fn a_subscribed_license_is_disabled_while_renewal_is_off() {
    assert_eq!(Mode::of(&subscribed(), false), Mode::Disabled);
}

#[test]
fn a_license_without_a_subscription_is_renewed_by_hand_either_way() {
    let license = license(None);

    for enabled in [true, false] {
        assert_eq!(
            Mode::of(&license, enabled),
            Mode::Manual,
            "enabled: {enabled}"
        );
    }
}

#[test]
fn wire_strings_are_pinned() {
    let cases = [
        (Mode::Automatic, "automatic"),
        (Mode::Disabled, "disabled"),
        (Mode::Manual, "manual"),
    ];
    for (mode, wire) in cases {
        assert_eq!(serde_json::to_value(mode).unwrap(), wire);
    }
}
