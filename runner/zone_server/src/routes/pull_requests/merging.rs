use serde_json::json;
use uuid::Uuid;

use crate::auth::WorkspaceWriter;
use crate::db::audit::actions;
use crate::db::audit::resources;
use crate::db::workspace_members::WorkspaceRole;
use crate::routes::common::AuditEvent;
use crate::routes::common::audit;
use crate::services::github::pull_requests;
use crate::services::github::pull_requests::Error;
use crate::services::github::pull_requests::Merged;
use crate::services::github::pull_requests::Method;
use crate::services::github::readiness::CommitSha;
use crate::state::AppState;

pub struct Merging {
    pub writer: WorkspaceWriter,
    pub source_id: Uuid,
    pub number: u64,
    pub expected: CommitSha,
    pub method: Method,
}

impl Merging {
    pub async fn run(self, state: AppState) -> Result<Merged, Error> {
        let merged = pull_requests::merge(
            &state,
            self.writer.workspace_id,
            self.source_id,
            self.number,
            &self.expected,
            self.method,
            self.writer.role >= WorkspaceRole::Admin,
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
