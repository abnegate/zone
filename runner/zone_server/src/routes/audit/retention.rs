//! An organization's audit retention period, and the server's for audit rows that belong to no
//! organization. Admins read it with the rows it is waiting on; changing or clearing it takes an
//! Enterprise license that is active or in grace, because clearing loosens a compliance control.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Number, json};
use uuid::Uuid;
use zone_enterprise::Feature;
use zone_enterprise::audit::{Standing, Wait};

use super::access::{self, ORGANIZATION_MISSING};
use super::refusal::Refusal;
use crate::auth::entitlement::Unentitled;
use crate::auth::{AuthUser, ServerAdmin};
use crate::db::audit::scope::Scope;
use crate::db::audit::{actions, resources};
use crate::routes::common::{AuditEvent, audit};
use crate::services::audit::retention::{self, Change, Holder, Status};
use crate::services::audit::{Actor, standing};
use crate::state::AppState;

const DAYS: &str = "days";

#[derive(Debug, Serialize)]
pub struct RetentionResponse {
    pub standing: Standing,
    pub days: Option<i32>,
    pub cutoff: Option<DateTime<Utc>>,
    pub expired: i64,
    pub expired_capped: bool,
    pub held: i64,
    pub held_capped: bool,
    pub holders: Vec<HolderResponse>,
    pub swept_at: Option<DateTime<Utc>>,
    pub deleted_count: i64,
    pub updated_at: Option<DateTime<Utc>>,
}

impl RetentionResponse {
    fn new(standing: Standing, status: Status) -> Self {
        Self {
            standing,
            days: status.days,
            cutoff: status.cutoff,
            expired: status.expired,
            expired_capped: status.expired_capped,
            held: status.held,
            held_capped: status.held_capped,
            holders: status
                .holders
                .into_iter()
                .map(HolderResponse::from)
                .collect(),
            swept_at: status.swept_at,
            deleted_count: status.deleted_count,
            updated_at: status.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct HolderResponse {
    pub destination_id: Uuid,
    pub name: String,
    pub wait: Wait,
}

impl From<Holder> for HolderResponse {
    fn from(holder: Holder) -> Self {
        Self {
            destination_id: holder.destination_id,
            name: holder.name,
            wait: holder.wait,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRetentionRequest {
    #[serde(default)]
    pub days: Option<Number>,
}

/// GET /api/organizations/{org_id}/audit-retention
pub async fn organization(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    respond(read_organization(&state, &auth, path).await)
}

/// PUT /api/organizations/{org_id}/audit-retention
pub async fn update_organization(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<UpdateRetentionRequest>, JsonRejection>,
) -> Response {
    respond(write_organization(&state, &auth, path, body).await)
}

/// GET /api/audit-retention
pub async fn server(State(state): State<AppState>, _: ServerAdmin) -> Response {
    respond(read(&state, Scope::Server).await)
}

/// PUT /api/audit-retention
pub async fn update_server(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    body: Result<Json<UpdateRetentionRequest>, JsonRejection>,
) -> Response {
    let actor = Actor {
        id: admin.id,
        email: &admin.email,
    };
    respond(write(&state, Scope::Server, &actor, body).await)
}

fn respond(result: Result<RetentionResponse, Refusal>) -> Response {
    match result {
        Ok(response) => Json(response).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

async fn read_organization(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<RetentionResponse, Refusal> {
    let user = auth.0.user_id().map_err(|_| Refusal::Unauthenticated)?;
    let Ok(Path(organization)) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let mut transaction = state.db().begin().await?;
    access::organization(&mut transaction, organization, user).await?;
    transaction.commit().await?;
    read(state, Scope::Organization(organization)).await
}

async fn write_organization(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<UpdateRetentionRequest>, JsonRejection>,
) -> Result<RetentionResponse, Refusal> {
    let user = auth.0.user_id().map_err(|_| Refusal::Unauthenticated)?;
    let Ok(Path(organization)) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let scope = Scope::Organization(organization);
    let actor = Actor {
        id: user,
        email: &auth.0.email,
    };
    let standing = standing(state).await?;
    let mut transaction = state.db().begin().await?;
    access::organization(&mut transaction, organization, user).await?;
    let days = requested(&standing, body)?;
    let change = retention::update(&mut transaction, scope, days, user).await?;
    transaction.commit().await?;
    settled(state, scope, &actor, standing, change).await
}

async fn read(state: &AppState, scope: Scope) -> Result<RetentionResponse, Refusal> {
    let standing = standing(state).await?;
    let status = retention::status(state.db(), scope, &standing, Utc::now()).await?;
    Ok(RetentionResponse::new(standing, status))
}

async fn write(
    state: &AppState,
    scope: Scope,
    actor: &Actor<'_>,
    body: Result<Json<UpdateRetentionRequest>, JsonRejection>,
) -> Result<RetentionResponse, Refusal> {
    let standing = standing(state).await?;
    let days = requested(&standing, body)?;
    let mut transaction = state.db().begin().await?;
    let change = retention::update(&mut transaction, scope, days, actor.id).await?;
    transaction.commit().await?;
    settled(state, scope, actor, standing, change).await
}

fn requested(
    standing: &Standing,
    body: Result<Json<UpdateRetentionRequest>, JsonRejection>,
) -> Result<Option<i32>, Refusal> {
    if !standing.retention {
        return Err(Unentitled(Feature::AuditRetention).into());
    }
    let Json(request) = body.map_err(Refusal::Body)?;
    Ok(retention::period(request.days.as_ref())?)
}

async fn settled(
    state: &AppState,
    scope: Scope,
    actor: &Actor<'_>,
    standing: Standing,
    change: Change,
) -> Result<RetentionResponse, Refusal> {
    if change.changed() {
        record(state, scope, actor, change).await;
    }
    let status = retention::status(state.db(), scope, &standing, Utc::now()).await?;
    Ok(RetentionResponse::new(standing, status))
}

async fn record(state: &AppState, scope: Scope, actor: &Actor<'_>, change: Change) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: scope.organization_id(),
            workspace_id: None,
            actor_id: actor.id,
            actor_email: actor.email,
            action: actions::AUDIT_RETENTION_UPDATED,
            resource_type: resources::AUDIT_RETENTION,
            resource_id: scope.organization_id(),
            old_values: Some(json!({ DAYS: change.previous })),
            new_values: Some(json!({ DAYS: change.next })),
        },
    )
    .await;
}
