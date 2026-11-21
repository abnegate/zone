use uuid::Uuid;
use zone_enterprise::{Keyring, Payload, Plan, SigningKey, TrustedKey, sign};

use super::*;

const KEY_ID: &str = "test-1";
const SEED: [u8; 32] = [0x11; 32];
const SEATS: u32 = 3;

fn license() -> License {
    issued(None)
}

fn issued(plan: Option<Plan>) -> License {
    let signing = SigningKey::from_bytes(&SEED);
    let keyring =
        Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
            .unwrap();
    let text = sign(
        &Payload {
            id: Uuid::new_v4(),
            customer: "Acme Ltd".into(),
            seats: SEATS,
            issued_at: "2026-11-01T00:00:00Z".parse().unwrap(),
            expires_at: "2027-11-01T00:00:00Z".parse().unwrap(),
            key_id: KEY_ID.into(),
            subscription: None,
            plan,
        },
        &signing,
    );
    keyring.verify(&text).unwrap()
}

#[test]
fn the_audit_record_keeps_the_license_but_never_its_key() {
    let license = license();

    let record = summary(&license);

    assert_eq!(
        record,
        json!({
            "id": license.id(),
            "plan": "enterprise",
            "customer": "Acme Ltd",
            "seats": SEATS,
            "expires_at": "2027-11-01T00:00:00Z",
            "key_id": KEY_ID,
        })
    );
    assert!(!record.to_string().contains(license.text()));
}

#[test]
fn the_audit_record_keeps_the_plan_the_license_grants() {
    for (plan, expected) in [
        (None, "enterprise"),
        (Some(Plan::Enterprise), "enterprise"),
        (Some(Plan::Team), "team"),
    ] {
        assert_eq!(summary(&issued(plan))["plan"], expected, "{plan:?}");
    }
}
