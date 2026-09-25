//! Codes claude.com sent back to Zone's callback listener, each parked under a one-time receipt
//! until the console hands the receipt back.

use std::sync::LazyLock;
use std::time::Instant;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use dashmap::DashMap;
use uuid::Uuid;

use super::claude::Code;
use super::pending::Pending;

const RECEIPT_BYTES: usize = 32;

static PARKED: LazyLock<DashMap<String, (Pending, Code, Instant)>> = LazyLock::new(DashMap::new);

/// Parks `code` for the sign-in it answers until `expires`, and names the receipt that takes it
/// back.
pub fn park(pending: Pending, code: Code, expires: Instant) -> String {
    put(&PARKED, pending, code, expires, Instant::now())
}

/// The sign-in and code `receipt` names. Taking them spends the receipt, and a receipt whose
/// sign-in expired names nothing.
pub fn take(receipt: &str) -> Option<(Pending, Code)> {
    claim(&PARKED, receipt, Instant::now())
}

/// Drops the codes of the sign-ins `user` started in `organization`.
pub fn cancel(organization: Uuid, user: Uuid) {
    PARKED
        .retain(|_, (pending, _, _)| (pending.organization, pending.user) != (organization, user));
}

/// Drops the codes of every sign-in started in `organization`.
pub fn forget(organization: Uuid) {
    PARKED.retain(|_, (pending, _, _)| pending.organization != organization);
}

fn put(
    parked: &DashMap<String, (Pending, Code, Instant)>,
    pending: Pending,
    code: Code,
    expires: Instant,
    now: Instant,
) -> String {
    parked.retain(|_, (_, _, expiry)| *expiry > now);
    let mut bytes = [0u8; RECEIPT_BYTES];
    rand::fill(&mut bytes);
    let receipt = URL_SAFE_NO_PAD.encode(bytes);
    parked.insert(receipt.clone(), (pending, code, expires));
    receipt
}

fn claim(
    parked: &DashMap<String, (Pending, Code, Instant)>,
    receipt: &str,
    now: Instant,
) -> Option<(Pending, Code)> {
    let (_, (pending, code, expires)) = parked.remove(receipt)?;
    (expires > now).then_some((pending, code))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use zone_core::secret::SecretValue;

    use super::*;
    use crate::services::login::claude::{Redirect, Scope};

    const CODE: &str = "fake-authorization-code";
    const LATER: Duration = Duration::from_secs(60);

    fn pending(organization: Uuid, user: Uuid) -> Pending {
        Pending {
            organization,
            user,
            email: "admin@example.com".to_string(),
            session: Uuid::new_v4(),
            attempt: Uuid::new_v4(),
            verifier: SecretValue::new("fake-code-verifier"),
            scope: Scope::Inference,
            redirect: Redirect::Loopback(54_545),
            console: None,
        }
    }

    fn code() -> Code {
        Code {
            value: SecretValue::new(CODE),
            state: "fake-state".to_string(),
        }
    }

    #[test]
    fn a_receipt_takes_back_its_code_once() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let receipt = park(pending(organization, user), code(), Instant::now() + LATER);

        let (taken, parked) = take(&receipt).expect("a parked code");

        assert_eq!((taken.organization, taken.user), (organization, user));
        assert_eq!(parked.value.expose(), CODE);
        assert!(take(&receipt).is_none(), "a receipt was spent twice");
    }

    #[test]
    fn every_receipt_is_new_and_unguessable() {
        let now = Instant::now();
        let parked = DashMap::new();
        let first = put(
            &parked,
            pending(Uuid::new_v4(), Uuid::new_v4()),
            code(),
            now + LATER,
            now,
        );
        let second = put(
            &parked,
            pending(Uuid::new_v4(), Uuid::new_v4()),
            code(),
            now + LATER,
            now,
        );

        assert_ne!(first, second);
        for receipt in [first, second] {
            assert_eq!(receipt.len(), 43, "{receipt} is not 32 random bytes");
            assert!(
                receipt
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
                "{receipt} does not travel in a URL as it is"
            );
        }
    }

    #[test]
    fn a_receipt_expires_with_its_sign_in() {
        let now = Instant::now();
        let parked = DashMap::new();
        let fresh = put(
            &parked,
            pending(Uuid::new_v4(), Uuid::new_v4()),
            code(),
            now + LATER,
            now,
        );
        let stale = put(
            &parked,
            pending(Uuid::new_v4(), Uuid::new_v4()),
            code(),
            now + LATER,
            now,
        );

        assert!(claim(&parked, &fresh, now + LATER - Duration::from_secs(1)).is_some());
        assert!(
            claim(&parked, &stale, now + LATER).is_none(),
            "a receipt outlived its sign-in"
        );
        assert!(parked.is_empty(), "an expired receipt was kept");
    }

    #[test]
    fn parking_a_code_sweeps_expired_ones() {
        let now = Instant::now();
        let parked = DashMap::new();
        let abandoned = put(
            &parked,
            pending(Uuid::new_v4(), Uuid::new_v4()),
            code(),
            now + LATER,
            now,
        );

        put(
            &parked,
            pending(Uuid::new_v4(), Uuid::new_v4()),
            code(),
            now + LATER * 2,
            now + LATER,
        );

        assert!(!parked.contains_key(&abandoned));
        assert_eq!(parked.len(), 1);
    }

    #[test]
    fn cancelling_drops_only_the_callers_codes_and_forgetting_the_organizations() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let expires = Instant::now() + LATER;
        let mine = park(pending(organization, user), code(), expires);
        let colleague = park(pending(organization, Uuid::new_v4()), code(), expires);
        let elsewhere = park(pending(Uuid::new_v4(), user), code(), expires);

        cancel(organization, user);
        assert!(take(&mine).is_none(), "a cancelled sign-in's code was kept");

        forget(organization);
        assert!(
            take(&colleague).is_none(),
            "a forgotten organization's code was kept"
        );
        assert!(
            take(&elsewhere).is_some(),
            "another organization's code was dropped"
        );
    }
}
