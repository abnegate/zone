use uuid::Uuid;

use crate::db::spend::keys::KeyRow;

/// Whose spend a request is: a member's in a workspace, or the workspace's own automation when
/// `user` is none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Subject {
    pub organization: Uuid,
    pub workspace: Uuid,
    pub user: Option<Uuid>,
}

impl From<&KeyRow> for Subject {
    fn from(row: &KeyRow) -> Self {
        Self {
            organization: row.organization_id,
            workspace: row.workspace_id,
            user: row.user_id,
        }
    }
}
