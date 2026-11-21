use chrono::{SubsecRound, TimeDelta};
use uuid::Uuid;
use zone_enterprise::{Keyring, Payload, SigningKey, TrustedKey, sign};

use super::*;

const KEY_ID: &str = "test-1";
const SEED: [u8; 32] = [0x11; 32];
const SUBSCRIPTION: &str = "6f1f4b1e-4a8e-4f4e-9a55-0d6f3b8c2a11";

fn license(expires_at: DateTime<Utc>, subscription: Option<&str>) -> License {
    let signing = SigningKey::from_bytes(&SEED);
    let keyring =
        Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
            .unwrap();
    let text = sign(
        &Payload {
            id: Uuid::new_v4(),
            customer: "Acme Ltd".into(),
            seats: 5,
            issued_at: expires_at - TimeDelta::days(365),
            expires_at,
            key_id: KEY_ID.into(),
            subscription: subscription.map(str::to_owned),
            plan: None,
        },
        &signing,
    );
    keyring.verify(&text).unwrap()
}

fn now() -> DateTime<Utc> {
    Utc::now().trunc_subsecs(0)
}

#[test]
fn the_window_is_fourteen_days() {
    assert_eq!(WINDOW, TimeDelta::days(14));
}

#[test]
fn a_license_expiring_exactly_at_the_window_edge_is_due() {
    let now = now();
    assert!(due(&license(now + WINDOW, Some(SUBSCRIPTION)), now));
}

#[test]
fn a_license_expiring_a_second_past_the_window_is_not_due() {
    let now = now();
    assert!(!due(
        &license(now + WINDOW + TimeDelta::seconds(1), Some(SUBSCRIPTION)),
        now
    ));
}

#[test]
fn a_license_expiring_within_the_window_is_due() {
    let now = now();
    assert!(due(
        &license(now + TimeDelta::days(3), Some(SUBSCRIPTION)),
        now
    ));
}

#[test]
fn a_license_a_year_from_expiry_is_not_due() {
    let now = now();
    assert!(!due(
        &license(now + TimeDelta::days(365), Some(SUBSCRIPTION)),
        now
    ));
}

#[test]
fn an_expired_license_in_its_grace_period_is_due() {
    let now = now();
    assert!(due(
        &license(now - TimeDelta::days(1), Some(SUBSCRIPTION)),
        now
    ));
}

#[test]
fn a_license_past_its_grace_period_is_due() {
    let now = now();
    assert!(due(
        &license(now - TimeDelta::days(90), Some(SUBSCRIPTION)),
        now
    ));
}

#[test]
fn a_license_without_a_subscription_is_never_due() {
    let now = now();
    for expires_at in [
        now + TimeDelta::days(3),
        now - TimeDelta::days(1),
        now - TimeDelta::days(90),
    ] {
        assert!(!due(&license(expires_at, None), now), "{expires_at}");
    }
}
