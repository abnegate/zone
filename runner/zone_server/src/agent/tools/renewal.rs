use std::path::PathBuf;
use std::sync::Arc;

use zone_enterprise::policy::Effective;

use super::ChatTools;
use super::WorkspaceScope;
use super::egress::Refusals;

/// A chat's tool set is taken by the run that holds it, so a run that resumes
/// the turn builds its own from this.
#[derive(Clone)]
pub struct Renewal {
    pub(super) scope: Option<WorkspaceScope>,
    pub(super) directory: PathBuf,
    pub(super) policy: Option<Arc<Effective>>,
    pub(super) refusals: Refusals,
}

impl Renewal {
    pub async fn build(&self) -> ChatTools {
        let Some(scope) = self.scope.clone() else {
            return ChatTools::empty();
        };
        let tools = ChatTools::build_with(scope, self.policy.clone())
            .await
            .starting_in(Some(self.directory.clone()))
            .await;
        if let Some(refusing) = self.refusals.current() {
            tools.set_recorder(refusing);
        }
        tools
    }
}
