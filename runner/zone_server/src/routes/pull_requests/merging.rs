use serde_json::json;
use uuid::Uuid;

use crate::auth::Permission;
use crate::auth::WorkspaceMember;
use crate::db::audit::actions;
use crate::db::audit::resources;
use crate::routes::common::AuditEvent;
use crate::routes::common::audit;
use crate::services::github::pull_requests;
use crate::services::github::pull_requests::Error;
use crate::services::github::pull_requests::Merged;
use crate::services::github::pull_requests::Method;
use crate::services::github::readiness::CommitSha;
use crate::state::AppState;

pub struct Merging {
    pub writer: WorkspaceMember,
    pub source_id: Uuid,
    pub number: u64,
    pub expected: CommitSha,
    pub method: Method,
}

impl Merging {
    /// Whether the merge may override the branch's protections, as an
    /// administrator's merge on GitHub may.
    fn administers(&self) -> bool {
        self.writer.allows(Permission::PullRequestsOverride)
    }

    pub async fn run(self, state: AppState) -> Result<Merged, Error> {
        let merged = pull_requests::merge(
            &state,
            self.writer.workspace_id,
            self.source_id,
            self.number,
            &self.expected,
            self.method,
            self.administers(),
        )
        .await?;
        audit(
            state.db(),
            AuditEvent {
                organization_id: None,
                workspace_id: Some(self.writer.workspace_id),
                actor_id: self.writer.user_id,
                actor_email: &self.writer.email,
                action: actions::PULL_REQUEST_MERGED,
                resource_type: resources::PULL_REQUEST,
                resource_id: Some(self.source_id),
                old_values: None,
                new_values: Some(json!({
                    "source_id": self.source_id,
                    "number": merged.number,
                    "head": self.expected.as_str(),
                    "method": self.method,
                    "sha": merged.sha,
                    "admin": merged.admin,
                    "url": merged.url,
                })),
            },
        )
        .await;
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{Grant, Scope};
    use crate::db::custom_roles::Summary;
    use crate::db::workspace_members::WorkspaceRole;

    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

    fn merging(role: WorkspaceRole, custom_role: Option<Summary>, grant: Grant) -> Merging {
        Merging {
            writer: WorkspaceMember {
                workspace_id: Uuid::new_v4(),
                user_id: Uuid::new_v4(),
                email: "merger@example.test".to_string(),
                role,
                custom_role,
                grant,
            },
            source_id: Uuid::new_v4(),
            number: 1,
            expected: CommitSha::parse(HEAD).expect("a commit"),
            method: Method::default(),
        }
    }

    fn holding(permissions: &[Permission]) -> Merging {
        let names: Vec<String> = permissions
            .iter()
            .map(|permission| permission.name().to_string())
            .collect();
        let summary = Summary {
            id: Uuid::new_v4(),
            name: "Reviewer".to_string(),
        };
        let grant = Grant::compose(Scope::Workspace, &names).expect("the role composes");
        merging(WorkspaceRole::Viewer, Some(summary), grant)
    }

    #[test]
    fn a_viewer_overrides_protections_exactly_when_their_role_grants_it() {
        let overriding = holding(&[Permission::WorkspaceWrite, Permission::PullRequestsOverride]);
        let writing = holding(&[Permission::WorkspaceWrite]);

        assert!(overriding.administers());
        assert!(!writing.administers());
    }

    #[test]
    fn built_in_admins_override_and_members_do_not() {
        for (role, administers) in [
            (WorkspaceRole::Owner, true),
            (WorkspaceRole::Admin, true),
            (WorkspaceRole::Member, false),
            (WorkspaceRole::Viewer, false),
        ] {
            let merging = merging(role, None, Grant::workspace(role));
            assert_eq!(merging.administers(), administers, "{role:?}");
        }
    }
}
