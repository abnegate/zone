//! Claude sign-ins that were started and not yet finished, keyed by their OAuth state.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use uuid::Uuid;
use zone_core::secret::SecretValue;

use super::claude::{Flow, Redirect, Scope};
use super::console::Console;

pub const WINDOW: Duration = Duration::from_secs(600);

static PENDING: LazyLock<DashMap<String, (Pending, Instant)>> = LazyLock::new(DashMap::new);

#[derive(Debug)]
pub struct Pending {
    pub organization: Uuid,
    pub user: Uuid,
    pub email: String,
    /// The session that started the sign-in, which must still be active when it finishes.
    pub session: Uuid,
    pub attempt: Uuid,
    pub verifier: SecretValue,
    pub scope: Scope,
    /// Where the authorize link sent the browser, which the exchange has to name again.
    pub redirect: Redirect,
    /// Where the callback sends the browser to finish a loopback sign-in.
    pub console: Option<Console>,
}

/// Holds a sign-in under `state`, abandoning the same admin's earlier one in that organization.
pub fn hold(state: String, pending: Pending) {
    put(&PENDING, state, pending, Instant::now());
}

/// The sign-in `state` names, whichever way its code came back.
pub fn claim(state: &str) -> Option<Pending> {
    take(&PENDING, state, Instant::now(), |_| true).map(|(pending, _)| pending)
}

/// The sign-in `state` names, and when it expires, only if claude.com was to send its code to
/// Zone's callback listener. A state issued for pasting stays held.
pub fn claim_loopback(state: &str) -> Option<(Pending, Instant)> {
    take(&PENDING, state, Instant::now(), |pending| {
        pending.redirect.flow() == Flow::Loopback
    })
}

/// Drops the sign-ins `user` started in `organization`.
pub fn cancel(organization: Uuid, user: Uuid) {
    PENDING.retain(|_, (pending, _)| (pending.organization, pending.user) != (organization, user));
}

/// Drops every sign-in started in `organization`.
pub fn forget(organization: Uuid) {
    discard(&PENDING, organization);
}

fn discard(logins: &DashMap<String, (Pending, Instant)>, organization: Uuid) {
    logins.retain(|_, (pending, _)| pending.organization != organization);
}

fn put(
    logins: &DashMap<String, (Pending, Instant)>,
    state: String,
    pending: Pending,
    now: Instant,
) {
    logins.retain(|_, (held, expires)| {
        *expires > now && (held.organization, held.user) != (pending.organization, pending.user)
    });
    logins.insert(state, (pending, now + WINDOW));
}

fn take(
    logins: &DashMap<String, (Pending, Instant)>,
    state: &str,
    now: Instant,
    eligible: impl FnOnce(&Pending) -> bool,
) -> Option<(Pending, Instant)> {
    let (_, (pending, expires)) = logins.remove_if(state, |_, (pending, _)| eligible(pending))?;
    (expires > now).then_some((pending, expires))
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
            session: Uuid::new_v4(),
            attempt: Uuid::new_v4(),
            verifier: SecretValue::new(VERIFIER),
            scope: Scope::Full,
            redirect: Redirect::Paste,
            console: None,
        }
    }

    fn stranger() -> Pending {
        pending(Uuid::new_v4(), Uuid::new_v4())
    }

    fn looping() -> Pending {
        Pending {
            redirect: LOOPBACK,
            console: Console::at("http://localhost:3000"),
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
    fn a_loopback_login_is_claimed_by_the_callback_once_with_its_expiry() {
        let state = format!("fake-state-{}", Uuid::new_v4());
        let before = Instant::now();
        hold(state.clone(), looping());

        let (claimed, expires) = claim_loopback(&state).expect("a held loopback login is claimed");

        assert_eq!(claimed.redirect, LOOPBACK);
        assert!(expires >= before + WINDOW && expires <= Instant::now() + WINDOW);
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
    fn cancelling_drops_only_the_callers_logins_and_forgetting_the_organizations() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let mine = format!("fake-state-{}", Uuid::new_v4());
        let colleague = format!("fake-state-{}", Uuid::new_v4());
        let elsewhere = format!("fake-state-{}", Uuid::new_v4());
        hold(mine.clone(), pending(organization, user));
        hold(colleague.clone(), pending(organization, Uuid::new_v4()));
        hold(elsewhere.clone(), pending(Uuid::new_v4(), user));

        cancel(organization, user);
        assert!(
            claim(&mine).is_none(),
            "a cancelled sign-in could still finish"
        );

        forget(organization);
        assert!(
            claim(&colleague).is_none(),
            "a forgotten organization's sign-in could still finish"
        );
        assert!(
            claim(&elsewhere).is_some(),
            "another organization's was dropped"
        );
    }

    #[test]
    fn forgetting_an_organization_drops_its_logins_and_no_one_elses() {
        let logins = DashMap::new();
        let now = Instant::now();
        let organization = Uuid::new_v4();
        put(
            &logins,
            "admin".to_string(),
            pending(organization, Uuid::new_v4()),
            now,
        );
        put(
            &logins,
            "colleague".to_string(),
            Pending {
                redirect: LOOPBACK,
                ..pending(organization, Uuid::new_v4())
            },
            now,
        );
        put(&logins, "elsewhere".to_string(), stranger(), now);

        discard(&logins, organization);

        assert!(
            !logins.contains_key("admin") && !logins.contains_key("colleague"),
            "a deleted organization's sign-in could still finish"
        );
        assert!(logins.contains_key("elsewhere"));
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
