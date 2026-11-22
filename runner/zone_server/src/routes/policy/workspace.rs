use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use zone_enterprise::policy::{Effective, Patch, Rules};
use zone_enterprise::{Edition, Status};

use super::access::{self, Access};
use super::refusal::Refusal;
use super::standing::Standing;
use super::{caller, rules, serialized};
use crate::auth::{AuthUser, Permission};
use crate::db::audit::{actions, resources};
use crate::db::policies::{self, Stored};
use crate::routes::common::{AuditEvent, audit};
use crate::state::AppState;

const MISSING: &str = "Workspace not found";
const NEEDED: Permission = Permission::WorkspacePoliciesUpdate;

type WorkspacePath = Result<Path<(Uuid, Uuid)>, PathRejection>;

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
    pub editable: bool,
    pub organization: Rules,
    pub workspace: Rules,
    pub effective: Rules,
    pub updated_at: Option<DateTime<Utc>>,
}

impl WorkspaceResponse {
    fn new(
        standing: Standing,
        access: Access,
        organization: Rules,
        workspace: Rules,
        stored: Option<&Stored>,
    ) -> Self {
        let effective = Effective::new(organization.clone(), workspace.clone()).rules();
        Self {
            entitled: standing.entitled,
            enforced: standing.enforced,
            edition: standing.edition,
            status: standing.status,
            editable: standing.editable(access),
            organization,
            workspace,
            effective,
            updated_at: stored.map(|stored| stored.updated_at),
        }
    }
}

/// GET /api/organizations/{org_id}/workspaces/{ws_id}/agent-policy
pub async fn get(State(state): State<AppState>, auth: AuthUser, path: WorkspacePath) -> Response {
    respond(read(&state, &auth, path).await)
}

/// PATCH /api/organizations/{org_id}/workspaces/{ws_id}/agent-policy
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    path: WorkspacePath,
    body: Result<Json<Patch>, JsonRejection>,
) -> Response {
    respond(write(&state, &auth, path, body).await)
}

fn respond(result: Result<WorkspaceResponse, Refusal>) -> Response {
    match result {
        Ok(response) => Json(response).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

async fn read(
    state: &AppState,
    auth: &AuthUser,
    path: WorkspacePath,
) -> Result<WorkspaceResponse, Refusal> {
    let user = caller(auth)?;
    let Ok(Path((organization, workspace))) = path else {
        return Err(Refusal::Missing(MISSING));
    };
    let mut transaction = state.db().begin().await?;
    let access = access::workspace(&mut transaction, organization, workspace, user).await?;
    let outer = policies::organization(&mut *transaction, organization).await?;
    let stored = policies::workspace(&mut *transaction, workspace).await?;
    transaction.commit().await?;
    let standing = Standing::of(&state.stored_license().await?, Utc::now());
    Ok(WorkspaceResponse::new(
        standing,
        access,
        rules(outer.as_ref())?,
        rules(stored.as_ref())?,
        stored.as_ref(),
    ))
}

async fn write(
    state: &AppState,
    auth: &AuthUser,
    path: WorkspacePath,
    body: Result<Json<Patch>, JsonRejection>,
) -> Result<WorkspaceResponse, Refusal> {
    let user = caller(auth)?;
    let Ok(Path((organization, workspace))) = path else {
        return Err(Refusal::Missing(MISSING));
    };
    let license = state.stored_license().await;
    let mut transaction = state.db().begin().await?;
    let access = access::workspace(&mut transaction, organization, workspace, user).await?;
    access.write(NEEDED)?;
    let standing = Standing::of(&license?, Utc::now());
    standing.require()?;
    let Json(patch) = body.map_err(Refusal::Body)?;
    let outer = rules(
        policies::organization(&mut *transaction, organization)
            .await?
            .as_ref(),
    )?;
    let current = policies::lock_workspace(&mut transaction, workspace).await?;
    let previous = rules(current.as_ref())?;
    let next = previous.patched(&patch).normalized()?;
    next.within(&outer, &patch.fields())?;
    if next == previous {
        transaction.rollback().await?;
        return Ok(WorkspaceResponse::new(
            standing,
            access,
            outer,
            previous,
            current.as_ref(),
        ));
    }
    let saved =
        policies::save_workspace(&mut transaction, workspace, &serialized(&next)?, user).await?;
    transaction.commit().await?;
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(organization),
            workspace_id: Some(workspace),
            actor_id: user,
            actor_email: &auth.0.email,
            action: actions::AGENT_POLICY_UPDATED,
            resource_type: resources::WORKSPACE_AGENT_POLICY,
            resource_id: Some(workspace),
            old_values: Some(serialized(&previous)?),
            new_values: Some(serialized(&next)?),
        },
    )
    .await;
    Ok(WorkspaceResponse::new(
        standing,
        access,
        outer,
        next,
        Some(&saved),
    ))
}
