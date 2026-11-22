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

impl Subject {
    /// Whose spend `row`'s key carries, none once its workspace or member is deleted.
    pub fn of(row: &KeyRow) -> Option<Self> {
        let workspace = row.workspace_id?;
        if !row.automated && row.user_id.is_none() {
            return None;
        }
        Some(Self {
            organization: row.organization_id,
            workspace,
            user: row.user_id,
        })
    }
}
