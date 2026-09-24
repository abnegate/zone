//! Claude sign-ins that were started and not yet finished, keyed by their OAuth state.
//!
//! One admin has at most one sign-in in flight per organization: starting another abandons the
//! first. That bounds how many are held, and a held state is unguessable, finishes once, and
//! expires with [`WINDOW`].

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use uuid::Uuid;
use zone_core::secret::SecretValue;

use super::claude::{Flow, Redirect, Scope};

pub const WINDOW: Duration = Duration::from_secs(600);

static PENDING: LazyLock<DashMap<String, Held>> = LazyLock::new(DashMap::new);

#[derive(Debug)]
pub struct Pending {
    pub organization: Uuid,
    pub user: Uuid,
    /// Whom the audit log names when the sign-in finishes, which may be from a request that
    /// carries no session to ask.
    pub email: String,
    pub verifier: SecretValue,
    pub scope: Scope,
    /// Where the authorize link sent the browser, which the exchange has to name again.
    pub redirect: Redirect,
}

struct Held {
    pending: Pending,
    expires: Instant,
}

pub fn hold(state: String, pending: Pending) {
    put(&PENDING, state, pending, Instant::now());
}

/// The sign-in `state` names, whichever way its code came back.
pub fn claim(state: &str) -> Option<Pending> {
    take(&PENDING, state, Instant::now(), |_| true)
}

/// The sign-in `state` names, only if claude.com was to send its code to Zone's callback
/// listener. A state issued for pasting stays held for the admin who will paste it.
pub fn claim_loopback(state: &str) -> Option<Pending> {
    take(&PENDING, state, Instant::now(), |pending| {
        pending.redirect.flow() == Flow::Loopback
    })
}

fn put(logins: &DashMap<String, Held>, state: String, pending: Pending, now: Instant) {
    logins.retain(|_, held| {
        held.expires > now
            && (held.pending.organization, held.pending.user)
                != (pending.organization, pending.user)
    });
    logins.insert(
        state,
        Held {
            pending,
            expires: now + WINDOW,
        },
    );
}

fn take(
    logins: &DashMap<String, Held>,
    state: &str,
    now: Instant,
    eligible: impl FnOnce(&Pending) -> bool,
) -> Option<Pending> {
    let (_, held) = logins.remove_if(state, |_, held| eligible(&held.pending))?;
    (held.expires > now).then_some(held.pending)
}

#[cfg(test)]
mod tests {
    use zone_core::secret::REDACTED;

    use super::*;

    const VERIFIER: &str = "fake-code-verifier";
    const LOOPBACK: Redirect = Redirect::Loopback(54_545);

    fn pending(organization: Uuid, user: Uuid) -> Pending {
        Pending {
            organization,
            user,
            email: "admin@example.com".to_string(),
            verifier: SecretValue::new(VERIFIER),
            scope: Scope::Full,
            redirect: Redirect::Paste,
        }
    }

    fn stranger() -> Pending {
        pending(Uuid::new_v4(), Uuid::new_v4())
    }

    fn looping() -> Pending {
        Pending {
            redirect: LOOPBACK,
            ..stranger()
        }
    }

    fn loopback(pending: &Pending) -> bool {
        pending.redirect.flow() == Flow::Loopback
    }

    #[test]
    fn a_login_is_claimed_once() {
        let state = format!("fake-state-{}", Uuid::new_v4());
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        hold(state.clone(), pending(organization, user));

        let claimed = claim(&state).expect("a held login is claimed");

        assert_eq!(
            (claimed.organization, claimed.user, claimed.scope),
            (organization, user, Scope::Full)
        );
        assert_eq!(claimed.verifier.expose(), VERIFIER);
        assert_eq!(claimed.redirect, Redirect::Paste);
        assert!(
            claim(&state).is_none(),
            "a state must finish one sign-in only"
        );
    }

    #[test]
    fn a_loopback_login_is_claimed_by_the_callback_once() {
        let state = format!("fake-state-{}", Uuid::new_v4());
        hold(state.clone(), looping());

        let claimed = claim_loopback(&state).expect("a held loopback login is claimed");

        assert_eq!(claimed.redirect, LOOPBACK);
        assert!(
            claim_loopback(&state).is_none(),
            "a callback replayed a state that was already spent"
        );
        assert!(claim(&state).is_none());
    }

    #[test]
    fn the_callback_never_claims_a_login_started_for_pasting() {
        let state = format!("fake-state-{}", Uuid::new_v4());
        hold(state.clone(), stranger());

        assert!(
            claim_loopback(&state).is_none(),
            "the callback finished a sign-in whose code was to be pasted"
        );
        assert!(
            claim(&state).is_some(),
            "the callback spent a sign-in that was waiting for its pasted code"
        );
    }

    #[test]
    fn a_login_expires_with_its_window() {
        let logins = DashMap::new();
        let now = Instant::now();
        put(&logins, "fresh".to_string(), stranger(), now);
        put(&logins, "stale".to_string(), stranger(), now);
        put(&logins, "stale-loopback".to_string(), looping(), now);

        assert!(
            take(
                &logins,
                "fresh",
                now + WINDOW - Duration::from_secs(1),
                |_| true
            )
            .is_some()
        );
        assert!(take(&logins, "stale", now + WINDOW, |_| true).is_none());
        assert!(
            take(&logins, "stale-loopback", now + WINDOW, loopback).is_none(),
            "the callback finished a sign-in after its window closed"
        );
        assert!(
            logins.is_empty(),
            "an expired login is dropped once it is looked up"
        );
    }

    #[test]
    fn a_new_login_replaces_the_same_users_earlier_one_only() {
        let logins = DashMap::new();
        let now = Instant::now();
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        put(
            &logins,
            "first".to_string(),
            Pending {
                redirect: LOOPBACK,
                ..pending(organization, user)
            },
            now,
        );
        put(
            &logins,
            "colleague".to_string(),
            pending(organization, Uuid::new_v4()),
            now,
        );
        put(
            &logins,
            "elsewhere".to_string(),
            pending(Uuid::new_v4(), user),
            now,
        );
        put(
            &logins,
            "second".to_string(),
            pending(organization, user),
            now,
        );

        assert!(
            take(&logins, "first", now, loopback).is_none(),
            "starting again abandons the earlier sign-in, whichever way its code was to return"
        );
        for state in ["second", "colleague", "elsewhere"] {
            assert!(take(&logins, state, now, |_| true).is_some(), "{state}");
        }
    }

    #[test]
    fn holding_a_login_sweeps_expired_ones() {
        let logins = DashMap::new();
        let now = Instant::now();
        put(&logins, "abandoned".to_string(), stranger(), now);

        put(&logins, "started".to_string(), stranger(), now + WINDOW);

        assert!(!logins.contains_key("abandoned"));
        assert!(logins.contains_key("started"));
    }

    #[test]
    fn debug_of_a_pending_login_hides_the_verifier() {
        let rendered = format!("{:?}", stranger());

        assert!(!rendered.contains(VERIFIER), "{rendered}");
        assert!(rendered.contains(REDACTED), "{rendered}");
    }
}
