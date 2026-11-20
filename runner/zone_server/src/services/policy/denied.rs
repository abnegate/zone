use uuid::Uuid;
use zone_enterprise::policy::Denial;

pub use super::subject::Subject;

/// A denial to audit: what `actor` started in `workspace`, one of `organization`'s, and what the
/// policy refused about it. `detail` names the model, host, server or tool refused, when one was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Denied<'a> {
    pub workspace: Uuid,
    /// Looked up from the workspace when the caller does not have it to hand.
    pub organization: Option<Uuid>,
    pub actor: Option<Uuid>,
    pub subject: Subject,
    pub denial: Denial,
    pub detail: Option<&'a str>,
}
