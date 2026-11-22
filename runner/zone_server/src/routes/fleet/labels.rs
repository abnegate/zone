use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
use zone_enterprise::Feature;
use zone_enterprise::fleet::Labels;

use super::refusal::Refusal;
use super::requests::UpdateRunnerLabelsRequest;
use super::responses::{
    AvailableLabel, OrganizationRunnerLabelsResponse, WorkspaceRunnerLabelsResponse, effective,
    requirement,
};
use super::{caller, standing};
use crate::auth::AuthUser;
use crate::db::audit::{actions, resources};
use crate::db::fleet::labels::{self, Stored};
use crate::routes::common::{AuditEvent, audit};
use crate::routes::policy::access;
use crate::routes::policy::standing::Standing;
use crate::state::AppState;

const ORGANIZATION_MISSING: &str = "Organization not found";
const WORKSPACE_MISSING: &str = "Workspace not found";
const ORGANIZATION_FORBIDDEN: &str = "Only people who can change the organization's agent policy can change the runner labels its work needs";
const WORKSPACE_FORBIDDEN: &str = "Only people who can change this workspace's agent policy can change the runner labels its work needs";

pub type OrganizationPath = Result<Path<Uuid>, PathRejection>;
pub type WorkspacePath = Result<Path<(Uuid, Uuid)>, PathRejection>;
pub type Body = Result<Json<UpdateRunnerLabelsRequest>, JsonRejection>;

/// GET /api/organizations/{org_id}/runner-labels
pub async fn organization(
    State(state): State<AppState>,
    auth: AuthUser,
    path: OrganizationPath,
) -> Response {
    respond(read_organization(&state, &auth, path).await)
}

/// PATCH /api/organizations/{org_id}/runner-labels
pub async fn update_organization(
    State(state): State<AppState>,
    auth: AuthUser,
    path: OrganizationPath,
    body: Body,
) -> Response {
    respond(write_organization(&state, &auth, path, body).await)
}

/// GET /api/organizations/{org_id}/workspaces/{ws_id}/runner-labels
pub async fn workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    path: WorkspacePath,
) -> Response {
    respond(read_workspace(&state, &auth, path).await)
}

/// PATCH /api/organizations/{org_id}/workspaces/{ws_id}/runner-labels
pub async fn update_workspace(
    State(state): State<AppState>,
    auth: AuthUser,
    path: WorkspacePath,
    body: Body,
) -> Response {
    respond(write_workspace(&state, &auth, path, body).await)
}

fn respond<T: Serialize>(result: Result<T, Refusal>) -> Response {
    match result {
        Ok(response) => Json(response).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

async fn read_organization(
    state: &AppState,
    auth: &AuthUser,
    path: OrganizationPath,
) -> Result<OrganizationRunnerLabelsResponse, Refusal> {
    let user = caller(auth)?;
    let Ok(Path(organization)) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let mut transaction = state.db().begin().await?;
    let access = access::organization(&mut transaction, organization, user).await?;
    let stored = labels::organization(&mut *transaction, organization).await?;
    transaction.commit().await?;
    let standing = standing(state).await?;
    let (available, matching) = placement(state.db(), &requirement(stored.as_ref())).await?;
    Ok(OrganizationRunnerLabelsResponse::new(
        standing,
        access,
        stored.as_ref(),
        available,
        matching,
    ))
}

async fn write_organization(
    state: &AppState,
    auth: &AuthUser,
    path: OrganizationPath,
    body: Body,
) -> Result<OrganizationRunnerLabelsResponse, Refusal> {
    let user = caller(auth)?;
    let Ok(Path(organization)) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let license = state.stored_license().await;
    let mut transaction = state.db().begin().await?;
    let access = access::organization(&mut transaction, organization, user).await?;
    access.write(ORGANIZATION_FORBIDDEN)?;
    let standing = Standing::of(&license?, Utc::now(), Feature::FleetPolicy);
    standing.require()?;
    let Json(request) = body.map_err(Refusal::Body)?;
    let required = request.required()?;
    let saved = labels::save_organization(&mut transaction, organization, &required, user).await?;
    transaction.commit().await?;
    if saved.changed() {
        audit(
            state.db(),
            AuditEvent {
                organization_id: Some(organization),
                workspace_id: None,
                actor_id: user,
                actor_email: &auth.0.email,
                action: actions::RUNNER_LABELS_UPDATED,
                resource_type: resources::ORGANIZATION_RUNNER_LABELS,
                resource_id: Some(organization),
                old_values: Some(values(saved.previous.as_ref())),
                new_values: Some(values(saved.current.as_ref())),
            },
        )
        .await;
    }
    let (available, matching) = placement(state.db(), &required).await?;
    Ok(OrganizationRunnerLabelsResponse::new(
        standing,
        access,
        saved.current.as_ref(),
        available,
        matching,
    ))
}

async fn read_workspace(
    state: &AppState,
    auth: &AuthUser,
    path: WorkspacePath,
) -> Result<WorkspaceRunnerLabelsResponse, Refusal> {
    let user = caller(auth)?;
    let Ok(Path((organization, workspace))) = path else {
        return Err(Refusal::Missing(WORKSPACE_MISSING));
    };
    let mut transaction = state.db().begin().await?;
    let access = access::workspace(&mut transaction, organization, workspace, user).await?;
    let outer = labels::organization(&mut *transaction, organization).await?;
    let stored = labels::workspace(&mut *transaction, workspace).await?;
    transaction.commit().await?;
    let standing = standing(state).await?;
    let (available, matching) =
        placement(state.db(), &effective(outer.as_ref(), stored.as_ref())).await?;
    Ok(WorkspaceRunnerLabelsResponse::new(
        standing,
        access,
        outer.as_ref(),
        stored.as_ref(),
        available,
        matching,
    ))
}

async fn write_workspace(
    state: &AppState,
    auth: &AuthUser,
    path: WorkspacePath,
    body: Body,
) -> Result<WorkspaceRunnerLabelsResponse, Refusal> {
    let user = caller(auth)?;
    let Ok(Path((organization, workspace))) = path else {
        return Err(Refusal::Missing(WORKSPACE_MISSING));
    };
    let license = state.stored_license().await;
    let mut transaction = state.db().begin().await?;
    let access = access::workspace(&mut transaction, organization, workspace, user).await?;
    access.write(WORKSPACE_FORBIDDEN)?;
    let standing = Standing::of(&license?, Utc::now(), Feature::FleetPolicy);
    standing.require()?;
    let Json(request) = body.map_err(Refusal::Body)?;
    let required = request.required()?;
    let outer = labels::organization(&mut *transaction, organization).await?;
    let saved = labels::save_workspace(&mut transaction, workspace, &required, user).await?;
    transaction.commit().await?;
    if saved.changed() {
        audit(
            state.db(),
            AuditEvent {
                organization_id: Some(organization),
                workspace_id: Some(workspace),
                actor_id: user,
                actor_email: &auth.0.email,
                action: actions::RUNNER_LABELS_UPDATED,
                resource_type: resources::WORKSPACE_RUNNER_LABELS,
                resource_id: Some(workspace),
                old_values: Some(values(saved.previous.as_ref())),
                new_values: Some(values(saved.current.as_ref())),
            },
        )
        .await;
    }
    let (available, matching) = placement(
        state.db(),
        &effective(outer.as_ref(), saved.current.as_ref()),
    )
    .await?;
    Ok(WorkspaceRunnerLabelsResponse::new(
        standing,
        access,
        outer.as_ref(),
        saved.current.as_ref(),
        available,
        matching,
    ))
}

/// The labels the fleet's runners carry, and how many runners could take work requiring
/// `required`: counts only, never which runners.
async fn placement(
    pool: &PgPool,
    required: &Labels,
) -> Result<(Vec<AvailableLabel>, i64), Refusal> {
    let (available, matching) =
        tokio::try_join!(labels::catalogue(pool), labels::matching(pool, required))?;
    Ok((
        available.into_iter().map(AvailableLabel::from).collect(),
        matching,
    ))
}

fn values(stored: Option<&Stored>) -> Value {
    json!({ "labels": requirement(stored).to_vec() })
}
