use uuid::Uuid;

/// A pull request a chat's turn opened, as the turn saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub chat_id: Uuid,
    pub workspace_id: Uuid,
    pub url: String,
    pub title: Option<String>,
    /// The source it was opened through, when the turn knows. Otherwise the
    /// workspace's source for its repository, if it has one.
    pub source_id: Option<Uuid>,
    /// The member whose turn opened it, whom auto-fix acts as. A pull request
    /// opened with nobody to act as is recorded without auto-fix.
    pub opened_by: Option<Uuid>,
}
