use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use zone_enterprise::policy::{Patch, Rules};
use zone_enterprise::{Edition, Status};

use super::access::{self, Access};
use super::refusal::Refusal;
use super::standing::Standing;
use super::{caller, rules, serialized};
use crate::auth::AuthUser;
use crate::db::audit::{actions, resources};
use crate::db::policies::{self, Stored};
use crate::routes::common::{AuditEvent, audit};
use crate::state::AppState;

const MISSING: &str = "Organization not found";
const FORBIDDEN: &str =
    "Only organization owners and admins can change the organization's agent policy";

#[derive(Debug, Serialize)]
pub struct OrganizationResponse {
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
    pub editable: bool,
    pub policy: Rules,
    pub updated_at: Option<DateTime<Utc>>,
}

impl OrganizationResponse {
    fn new(standing: Standing, access: Access, policy: Rules, stored: Option<&Stored>) -> Self {
        Self {
            entitled: standing.entitled,
            enforced: standing.enforced,
            edition: standing.edition,
            status: standing.status,
            editable: standing.editable(access),
            policy,
            updated_at: stored.map(|stored| stored.updated_at),
        }
    }
}

/// GET /api/organizations/{org_id}/agent-policy
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    respond(read(&state, &auth, path).await)
}

/// PATCH /api/organizations/{org_id}/agent-policy
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<Patch>, JsonRejection>,
) -> Response {
    respond(write(&state, &auth, path, body).await)
}

fn respond(result: Result<OrganizationResponse, Refusal>) -> Response {
    match result {
        Ok(response) => Json(response).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

async fn read(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<OrganizationResponse, Refusal> {
    let user = caller(auth)?;
    let Ok(Path(organization)) = path else {
        return Err(Refusal::Missing(MISSING));
    };
    let mut transaction = state.db().begin().await?;
    let access = access::organization(&mut transaction, organization, user).await?;
    let stored = policies::organization(&mut *transaction, organization).await?;
    transaction.commit().await?;
    let standing = Standing::of(&state.stored_license().await?, Utc::now());
    let policy = rules(stored.as_ref())?;
    Ok(OrganizationResponse::new(
        standing,
        access,
        policy,
        stored.as_ref(),
    ))
}

async fn write(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<Patch>, JsonRejection>,
) -> Result<OrganizationResponse, Refusal> {
    let user = caller(auth)?;
    let Ok(Path(organization)) = path else {
        return Err(Refusal::Missing(MISSING));
    };
    let license = state.stored_license().await;
    let mut transaction = state.db().begin().await?;
    let access = access::organization(&mut transaction, organization, user).await?;
    access.write(FORBIDDEN)?;
    let standing = Standing::of(&license?, Utc::now());
    standing.require()?;
    let Json(patch) = body.map_err(Refusal::Body)?;
    let current = policies::lock_organization(&mut transaction, organization).await?;
    let previous = rules(current.as_ref())?;
    let next = previous.patched(&patch).normalized()?;
    if next == previous {
        transaction.rollback().await?;
        return Ok(OrganizationResponse::new(
            standing,
            access,
            previous,
            current.as_ref(),
        ));
    }
    let saved =
        policies::save_organization(&mut transaction, organization, &serialized(&next)?, user)
            .await?;
    transaction.commit().await?;
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(organization),
            workspace_id: None,
            actor_id: user,
            actor_email: &auth.0.email,
            action: actions::AGENT_POLICY_UPDATED,
            resource_type: resources::ORGANIZATION_AGENT_POLICY,
            resource_id: Some(organization),
            old_values: Some(serialized(&previous)?),
            new_values: Some(serialized(&next)?),
        },
    )
    .await;
    Ok(OrganizationResponse::new(
        standing,
        access,
        next,
        Some(&saved),
    ))
}
