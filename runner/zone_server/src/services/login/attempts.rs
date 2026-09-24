//! Claude sign-ins that may still finish, and why each one that failed away from its panel
//! failed. Every change for an organization is made under its lock.

use std::sync::LazyLock;

use dashmap::DashMap;
use uuid::Uuid;

static LIVE: LazyLock<DashMap<Uuid, Attempt>> = LazyLock::new(DashMap::new);
static FAILED: LazyLock<DashMap<Uuid, (Attempt, String)>> = LazyLock::new(DashMap::new);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub id: Uuid,
    pub organization: Uuid,
    pub user: Uuid,
}

/// Starts `attempt`, which abandons every earlier attempt of the same admin in the same
/// organization, and forgets why they failed.
pub fn begin(attempt: Attempt) {
    cancel(attempt.organization, attempt.user);
    LIVE.insert(attempt.id, attempt);
}

/// Whether the attempt may still finish.
pub fn live(id: Uuid) -> bool {
    LIVE.contains_key(&id)
}

/// Ends the attempt, which then can never finish.
pub fn end(id: Uuid) {
    LIVE.remove(&id);
}

/// Ends the attempt and keeps why it failed, unless it had already ended.
pub fn fail(id: Uuid, reason: String) {
    if let Some((_, attempt)) = LIVE.remove(&id) {
        FAILED.insert(id, (attempt, reason));
    }
}

/// Why the attempt failed, for the admin who made it in the organization it was for.
pub fn failure(id: Uuid, organization: Uuid, user: Uuid) -> Option<String> {
    FAILED
        .get(&id)
        .filter(|failed| (failed.0.organization, failed.0.user) == (organization, user))
        .map(|failed| failed.1.clone())
}

/// Ends every attempt of `user` in `organization`, and forgets why they failed.
pub fn cancel(organization: Uuid, user: Uuid) {
    let mine = |attempt: &Attempt| (attempt.organization, attempt.user) == (organization, user);
    LIVE.retain(|_, attempt| !mine(attempt));
    FAILED.retain(|_, (attempt, _)| !mine(attempt));
}

/// Ends every attempt in `organization`, and forgets why they failed.
pub fn forget(organization: Uuid) {
    LIVE.retain(|_, attempt| attempt.organization != organization);
    FAILED.retain(|_, (attempt, _)| attempt.organization != organization);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(organization: Uuid, user: Uuid) -> Attempt {
        Attempt {
            id: Uuid::new_v4(),
            organization,
            user,
        }
    }

    #[test]
    fn a_new_attempt_abandons_the_same_admins_earlier_one_in_that_organization_only() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let first = attempt(organization, user);
        let colleague = attempt(organization, Uuid::new_v4());
        let elsewhere = attempt(Uuid::new_v4(), user);
        for started in [first, colleague, elsewhere] {
            begin(started);
        }

        let second = attempt(organization, user);
        begin(second);

        assert!(!live(first.id), "an abandoned attempt could still finish");
        for kept in [second, colleague, elsewhere] {
            assert!(live(kept.id), "{kept:?}");
        }
    }

    #[test]
    fn a_failure_is_kept_for_its_own_admin_and_organization_once() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let failing = attempt(organization, user);
        begin(failing);

        fail(failing.id, "Claude refused".to_string());
        fail(failing.id, "a second reason".to_string());

        assert!(!live(failing.id));
        assert_eq!(
            failure(failing.id, organization, user).as_deref(),
            Some("Claude refused"),
            "an ended attempt took a second reason"
        );
        assert_eq!(failure(failing.id, organization, Uuid::new_v4()), None);
        assert_eq!(failure(failing.id, Uuid::new_v4(), user), None);
        assert_eq!(failure(Uuid::new_v4(), organization, user), None);
    }

    #[test]
    fn an_attempt_that_already_ended_keeps_no_failure() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let ended = attempt(organization, user);
        begin(ended);
        forget(organization);

        fail(ended.id, "too late".to_string());

        assert_eq!(
            failure(ended.id, organization, user),
            None,
            "a failure was kept for an organization that is gone"
        );
    }

    #[test]
    fn cancelling_or_forgetting_ends_attempts_and_their_failures() {
        let (organization, user) = (Uuid::new_v4(), Uuid::new_v4());
        let theirs = attempt(organization, Uuid::new_v4());
        let failed = attempt(organization, user);
        begin(failed);
        fail(failed.id, "an earlier failure".to_string());
        begin(theirs);

        cancel(organization, user);

        assert_eq!(failure(failed.id, organization, user), None);
        assert!(live(theirs.id), "a colleague's attempt was cancelled");

        let mine = attempt(organization, user);
        begin(mine);
        cancel(organization, user);
        assert!(!live(mine.id));
        assert!(live(theirs.id));

        forget(organization);
        assert!(!live(theirs.id));
    }
}
