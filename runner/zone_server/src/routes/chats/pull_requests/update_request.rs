use serde::Deserialize;

/// What to change about one of a chat's pull requests. Only the fields given
/// change.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    /// Hide it from the chat, or with `false` show it again.
    pub dismissed: Option<bool>,
    /// Make it a draft, or with `false` mark it ready for review.
    pub draft: Option<bool>,
    /// Turn GitHub's auto-merge on or off.
    pub auto_merge: Option<bool>,
    /// Start a run to fix it whenever it fails or cannot merge, or with
    /// `false` leave it alone.
    pub auto_fix: Option<bool>,
}

impl UpdateRequest {
    pub fn is_empty(&self) -> bool {
        self.dismissed.is_none()
            && self.draft.is_none()
            && self.auto_merge.is_none()
            && self.auto_fix.is_none()
    }

    /// Whether this changes anything on GitHub, or starts runs that do.
    /// Dismissing only hides the pull request in zone.
    pub fn reaches_github(&self) -> bool {
        self.draft.is_some() || self.auto_merge.is_some() || self.auto_fix.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_dismissal_stays_off_github() {
        let dismissal = UpdateRequest {
            dismissed: Some(true),
            ..UpdateRequest::default()
        };
        assert!(!dismissal.reaches_github());
        for change in [
            UpdateRequest {
                draft: Some(false),
                ..dismissal
            },
            UpdateRequest {
                auto_merge: Some(false),
                ..dismissal
            },
            UpdateRequest {
                auto_fix: Some(false),
                ..dismissal
            },
        ] {
            assert!(change.reaches_github(), "{change:?}");
        }
    }
}
