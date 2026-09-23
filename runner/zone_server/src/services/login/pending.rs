//! Claude sign-ins that were started and not yet finished, keyed by their OAuth state.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use uuid::Uuid;
use zone_core::secret::SecretValue;

use super::claude::Scope;

pub const WINDOW: Duration = Duration::from_secs(600);

static PENDING: LazyLock<DashMap<String, Held>> = LazyLock::new(DashMap::new);

#[derive(Debug)]
pub struct Pending {
    pub organization: Uuid,
    pub user: Uuid,
    pub verifier: SecretValue,
    pub scope: Scope,
}

struct Held {
    pending: Pending,
    expires: Instant,
}

pub fn hold(state: String, pending: Pending) {
    put(&PENDING, state, pending, Instant::now());
}

pub fn claim(state: &str) -> Option<Pending> {
    take(&PENDING, state, Instant::now())
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

fn take(logins: &DashMap<String, Held>, state: &str, now: Instant) -> Option<Pending> {
    let (_, held) = logins.remove(state)?;
    (held.expires > now).then_some(held.pending)
}

#[cfg(test)]
mod tests {
    use zone_core::secret::REDACTED;

    use super::*;

    const VERIFIER: &str = "fake-code-verifier";

    fn pending(organization: Uuid, user: Uuid) -> Pending {
        Pending {
            organization,
            user,
            verifier: SecretValue::new(VERIFIER),
            scope: Scope::Full,
        }
    }

    fn stranger() -> Pending {
        pending(Uuid::new_v4(), Uuid::new_v4())
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
        assert!(
            claim(&state).is_none(),
            "a state must finish one sign-in only"
        );
    }

    #[test]
    fn a_login_expires_with_its_window() {
        let logins = DashMap::new();
        let now = Instant::now();
        put(&logins, "fresh".to_string(), stranger(), now);
        put(&logins, "stale".to_string(), stranger(), now);

        assert!(take(&logins, "fresh", now + WINDOW - Duration::from_secs(1)).is_some());
        assert!(take(&logins, "stale", now + WINDOW).is_none());
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
            pending(organization, user),
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
            take(&logins, "first", now).is_none(),
            "starting again abandons the earlier sign-in"
        );
        for state in ["second", "colleague", "elsewhere"] {
            assert!(take(&logins, state, now).is_some(), "{state}");
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
