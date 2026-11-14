use super::WorkspaceScope;
use crate::db::tasks::Origin;
use crate::services::github::testing::connected::Connected;

impl Connected {
    pub(crate) fn scope(&self) -> WorkspaceScope {
        WorkspaceScope {
            state: self.state.clone(),
            workspace_id: self.workspace,
            chat_id: None,
            user_id: self.user,
            offline: false,
            origin: Origin::Member,
        }
    }
}
