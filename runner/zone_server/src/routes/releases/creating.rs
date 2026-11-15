use serde_json::json;

use super::create_request::CreateRequest;
use crate::auth::WorkspaceWriter;
use crate::db::audit::actions;
use crate::db::audit::resources;
use crate::routes::common::AuditEvent;
use crate::routes::common::audit;
use crate::services::github::releases;
use crate::services::github::releases::Created;
use crate::services::github::releases::Failure;
use crate::state::AppState;

pub struct Creating {
    pub writer: WorkspaceWriter,
    pub request: CreateRequest,
}

impl Creating {
    pub async fn run(self, state: AppState) -> Result<Created, Failure> {
        let release = releases::create(
            &state,
            self.writer.workspace_id,
            &self.request.proposal(),
            self.request.prerelease,
            &self.request.digest,
        )
        .await?;
        audit(
            state.db(),
            AuditEvent {
                organization_id: None,
                workspace_id: Some(self.writer.workspace_id),
                actor_id: self.writer.user_id,
                actor_email: &self.writer.email,
                action: actions::RELEASE_CREATED,
                resource_type: resources::RELEASE,
                resource_id: Some(release.source_id),
                old_values: None,
                new_values: Some(json!({
                    "source_id": release.source_id,
                    "release_id": release.id,
                    "tag": release.tag,
                    "url": release.url,
                    "prerelease": self.request.prerelease,
                })),
            },
        )
        .await;
        Ok(release)
    }
}
