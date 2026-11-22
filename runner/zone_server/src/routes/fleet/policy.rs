use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::refusal::Refusal;
use super::requests::UpdateFleetPolicyRequest;
use super::responses::FleetPolicyResponse;
use super::{guard, standing};
use crate::auth::ServerAdmin;
use crate::db::audit::{actions, resources};
use crate::db::fleet::minimum::Minimum;
use crate::db::fleet::policy::{self, Policy};
use crate::db::fleet::runners::Change;
use crate::db::users::UserRow;
use crate::routes::common::{AuditEvent, audit};
use crate::state::AppState;

pub type Body = Result<Json<UpdateFleetPolicyRequest>, JsonRejection>;

/// GET /api/fleet-policy
pub async fn get(State(state): State<AppState>, _admin: ServerAdmin) -> Response {
    respond(read(&state).await)
}

/// PATCH /api/fleet-policy
pub async fn update(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    body: Body,
) -> Response {
    respond(write(&state, &admin, body).await)
}

fn respond(result: Result<FleetPolicyResponse, Refusal>) -> Response {
    match result {
        Ok(response) => Json(response).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

async fn read(state: &AppState) -> Result<FleetPolicyResponse, Refusal> {
    let standing = standing(state).await?;
    let policy = policy::get(state.db()).await?;
    Ok(FleetPolicyResponse::new(standing, &policy))
}

pub(super) async fn write(
    state: &AppState,
    admin: &UserRow,
    body: Body,
) -> Result<FleetPolicyResponse, Refusal> {
    let standing = standing(state).await?;
    standing.require()?;
    let Json(request) = body.map_err(Refusal::Body)?;
    let minimum = request.minimum()?;
    let mut transaction = state.db().begin().await?;
    let previous = policy::lock(&mut transaction).await?;
    if tightens(previous.minimum.as_ref(), minimum.as_ref()) {
        guard(
            &mut transaction,
            Change::Minimum(minimum.clone()),
            request.confirm,
        )
        .await?;
    }
    let saved = policy::save(&mut transaction, minimum.as_ref(), admin.id).await?;
    transaction.commit().await?;
    if saved.changed() {
        audit(
            state.db(),
            AuditEvent {
                organization_id: None,
                workspace_id: None,
                actor_id: admin.id,
                actor_email: &admin.email,
                action: actions::FLEET_POLICY_UPDATED,
                resource_type: resources::FLEET_POLICY,
                resource_id: None,
                old_values: Some(values(&saved.previous)),
                new_values: Some(values(&saved.current)),
            },
        )
        .await;
    }
    Ok(FleetPolicyResponse::new(standing, &saved.current))
}

/// Whether moving the minimum from `previous` to `next` can turn a runner away: setting one or
/// raising it can, while clearing or lowering it never does.
pub(super) fn tightens(previous: Option<&Minimum>, next: Option<&Minimum>) -> bool {
    match (previous, next) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(previous), Some(next)) => next.version() > previous.version(),
    }
}

fn values(policy: &Policy) -> Value {
    json!({ "minimum_version": policy.minimum.as_ref().map(Minimum::to_string) })
}
