//! Who is reading an organization's agent statuses.

use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewer {
    pub user: Uuid,
    /// Whether they are one of the organization's admins or owners.
    pub manages: bool,
}

impl Viewer {
    /// Whether they may see the code of a sign-in `initiator` started. Whoever holds the code can
    /// finish the organization's sign-in with an account of their own.
    pub fn sees_code(self, initiator: Uuid) -> bool {
        self.manages || self.user == initiator
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_admins_and_whoever_started_the_sign_in_see_its_code() {
        let initiator = Uuid::new_v4();
        let viewer = |user, manages| Viewer { user, manages };

        assert!(viewer(Uuid::new_v4(), true).sees_code(initiator));
        assert!(viewer(initiator, false).sees_code(initiator));
        assert!(!viewer(Uuid::new_v4(), false).sees_code(initiator));
    }
}
