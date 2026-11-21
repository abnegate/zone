use chrono::{DateTime, TimeDelta, Utc};
use uuid::Uuid;
use zone_enterprise::{Keyring, Payload, Plan, SigningKey, TrustedKey, sign};

use super::*;

const KEY_ID: &str = "test-1";
const SEED: [u8; 32] = [0x11; 32];
const SUBSCRIPTION: &str = "6f1f4b1e-4a8e-4f4e-9a55-0d6f3b8c2a11";
const OTHER_SUBSCRIPTION: &str = "0b9e8f0a-2c55-4b1a-8f3e-7d4c1a2b3c4d";

fn verified(payload: &Payload) -> License {
    let signing = SigningKey::from_bytes(&SEED);
    let keyring =
        Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
            .unwrap();
    keyring.verify(&sign(payload, &signing)).unwrap()
}

fn expiry() -> DateTime<Utc> {
    "2027-11-01T00:00:00Z".parse().unwrap()
}

fn current() -> Payload {
    Payload {
        id: Uuid::new_v4(),
        customer: "Acme Ltd".into(),
        seats: 5,
        issued_at: expiry() - TimeDelta::days(365),
        expires_at: expiry(),
        key_id: KEY_ID.into(),
        subscription: Some(SUBSCRIPTION.into()),
        plan: None,
    }
}

fn renewal(current: &Payload) -> Payload {
    Payload {
        id: Uuid::new_v4(),
        issued_at: current.expires_at,
        expires_at: current.expires_at + TimeDelta::days(365),
        ..current.clone()
    }
}

#[test]
fn a_key_for_the_next_period_is_accepted() {
    let current = current();
    assert!(accepts(&verified(&current), &verified(&renewal(&current))));
}

#[test]
fn a_key_for_the_same_period_with_other_seats_is_accepted() {
    let current = current();
    let renewed = Payload {
        id: Uuid::new_v4(),
        seats: 8,
        ..current.clone()
    };
    assert!(accepts(&verified(&current), &verified(&renewed)));
}

#[test]
fn a_key_for_another_customer_is_refused() {
    let current = current();
    let renewed = Payload {
        customer: "Globex Corp".into(),
        ..renewal(&current)
    };
    assert!(!accepts(&verified(&current), &verified(&renewed)));
}

#[test]
fn a_key_for_another_subscription_is_refused() {
    let current = current();
    let renewed = Payload {
        subscription: Some(OTHER_SUBSCRIPTION.into()),
        ..renewal(&current)
    };
    assert!(!accepts(&verified(&current), &verified(&renewed)));
}

#[test]
fn a_key_without_a_subscription_is_refused() {
    let current = current();
    let renewed = Payload {
        subscription: None,
        ..renewal(&current)
    };
    assert!(!accepts(&verified(&current), &verified(&renewed)));
}

#[test]
fn nothing_replaces_a_key_without_a_subscription() {
    let current = Payload {
        subscription: None,
        ..current()
    };
    assert!(!accepts(&verified(&current), &verified(&renewal(&current))));
}

#[test]
fn a_key_expiring_earlier_is_refused() {
    let current = current();
    let renewed = Payload {
        issued_at: current.issued_at,
        expires_at: current.expires_at - TimeDelta::seconds(1),
        ..renewal(&current)
    };
    assert!(!accepts(&verified(&current), &verified(&renewed)));
}

#[test]
fn a_key_with_the_same_id_is_refused() {
    let current = current();
    let renewed = Payload {
        id: current.id,
        ..renewal(&current)
    };
    assert!(!accepts(&verified(&current), &verified(&renewed)));
}

#[test]
fn a_key_for_the_same_period_on_another_plan_is_accepted() {
    let current = Payload {
        plan: Some(Plan::Team),
        ..current()
    };
    let renewed = Payload {
        id: Uuid::new_v4(),
        plan: Some(Plan::Enterprise),
        ..current.clone()
    };
    let (current, renewed) = (verified(&current), verified(&renewed));

    assert_eq!(renewed.expires_at(), current.expires_at());
    assert_eq!(renewed.seats(), current.seats());
    assert_eq!(renewed.subscription(), current.subscription());
    assert_ne!(renewed.id(), current.id());
    assert!(accepts(&current, &renewed));
}
