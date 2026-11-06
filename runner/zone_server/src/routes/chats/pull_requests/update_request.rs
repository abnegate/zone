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
}
