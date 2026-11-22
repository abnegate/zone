use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::refusal::Refusal;
use super::requests::UpdateRunnerRequest;
use super::responses::{RunnerResponse, RunnersResponse};
use super::{guard, standing};
use crate::auth::ServerAdmin;
use crate::db::audit::{actions, resources};
use crate::db::fleet::policy;
use crate::db::fleet::revocation::{self, Revoked};
use crate::db::fleet::runners::{self, Change, Removal, Runner};
use crate::db::fleet::state::State as RunnerState;
use crate::db::fleet::transition::Transition;
use crate::db::users::UserRow;
use crate::routes::common::{AuditEvent, audit};
use crate::state::AppState;
use crate::workers;

const MISSING: &str = "Runner not found";

pub type RunnerPath = Result<Path<Uuid>, PathRejection>;
pub type Body = Result<Json<UpdateRunnerRequest>, JsonRejection>;

/// GET /api/runners
pub async fn list(State(state): State<AppState>, _admin: ServerAdmin) -> Response {
    match listing(&state).await {
        Ok(response) => Json(response).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

/// PATCH /api/runners/{runner_id}
pub async fn update(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    path: RunnerPath,
    body: Body,
) -> Response {
    match write(&state, &admin, path, body).await {
        Ok(response) => Json(response).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

/// DELETE /api/runners/{runner_id}
pub async fn remove(
    State(state): State<AppState>,
    ServerAdmin(admin): ServerAdmin,
    path: RunnerPath,
) -> Response {
    match removal(&state, &admin, path).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

pub(super) async fn listing(state: &AppState) -> Result<RunnersResponse, Refusal> {
    let standing = standing(state).await?;
    let (policy, listed) = tokio::try_join!(policy::get(state.db()), runners::list(state.db()))?;
    Ok(RunnersResponse::new(
        standing,
        &listed,
        policy.minimum.as_ref(),
        state.runner(),
        Utc::now(),
    ))
}

pub(super) async fn write(
    state: &AppState,
    admin: &UserRow,
    path: RunnerPath,
    body: Body,
) -> Result<RunnerResponse, Refusal> {
    let Ok(Path(id)) = path else {
        return Err(Refusal::Missing(MISSING));
    };
    standing(state).await?.require()?;
    let Json(request) = body.map_err(Refusal::Body)?;
    let requested = request.requested()?;
    let mut transaction = state.db().begin().await?;
    let policy = policy::lock(&mut transaction).await?;
    let runner = runners::get(&mut *transaction, id)
        .await?
        .ok_or(Refusal::Missing(MISSING))?;
    let previous = runner.state;
    let mut revoked = None;
    if previous != requested {
        if let Some(change) = reduction(id, previous, requested) {
            guard(&mut transaction, change, request.confirm).await?;
        }
        revoked = apply(&mut transaction, id, requested, admin.id).await?;
    }
    let listed = runners::listed(&mut *transaction, id)
        .await?
        .ok_or(Refusal::Missing(MISSING))?;
    transaction.commit().await?;
    if let Some(revoked) = revoked {
        stop(state, revoked);
    }
    if listed.runner.state != previous {
        audit(
            state.db(),
            AuditEvent {
                organization_id: None,
                workspace_id: None,
                actor_id: admin.id,
                actor_email: &admin.email,
                action: actions::RUNNER_UPDATED,
                resource_type: resources::RUNNER,
                resource_id: Some(id),
                old_values: Some(json!({ "name": runner.name, "state": previous.as_str() })),
                new_values: Some(json!({
                    "name": listed.runner.name,
                    "state": listed.runner.state.as_str(),
                })),
            },
        )
        .await;
    }
    Ok(RunnerResponse::new(
        &listed,
        policy.minimum.as_ref(),
        state.runner(),
        Utc::now(),
    ))
}

/// The change that taking a runner from `previous` to `next` makes to the fleet when it reduces
/// what the runner may take. Activating, and moving a revoked runner to cordoned, reduce nothing.
pub(super) fn reduction(id: Uuid, previous: RunnerState, next: RunnerState) -> Option<Change> {
    match (previous, next) {
        (RunnerState::Active, RunnerState::Cordoned) => Some(Change::Cordon(id)),
        (RunnerState::Active | RunnerState::Cordoned, RunnerState::Revoked) => {
            Some(Change::Revoke(id))
        }
        (RunnerState::Active, RunnerState::Active)
        | (RunnerState::Cordoned, RunnerState::Active | RunnerState::Cordoned)
        | (RunnerState::Revoked, _) => None,
    }
}

async fn apply(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    requested: RunnerState,
    actor: Uuid,
) -> Result<Option<Revoked>, Refusal> {
    let transition = match requested {
        RunnerState::Active => Transition::Activate,
        RunnerState::Cordoned => Transition::Cordon,
        RunnerState::Revoked => {
            return revocation::revoke(transaction, id, Some(actor))
                .await?
                .map(Some)
                .ok_or(Refusal::Missing(MISSING));
        }
    };
    runners::set_state(transaction, id, transition, actor)
        .await?
        .map(|_| None)
        .ok_or(Refusal::Missing(MISSING))
}

/// Signals the owners of a revoked runner's stopped runs, as a user's stop does. Only after the
/// revoke commits.
fn stop(state: &AppState, revoked: Revoked) {
    for run in revoked.publish() {
        workers::task::stop::request(run.id);
        workers::auto_project::poke_task(state, run.task_id);
    }
}

pub(super) async fn removal(
    state: &AppState,
    admin: &UserRow,
    path: RunnerPath,
) -> Result<(), Refusal> {
    let Ok(Path(id)) = path else {
        return Err(Refusal::Missing(MISSING));
    };
    standing(state).await?.require()?;
    let runner = match runners::remove(state.db(), id).await? {
        Removal::Removed(runner) => runner,
        Removal::Online => return Err(Refusal::Online),
        Removal::Missing => return Err(Refusal::Missing(MISSING)),
    };
    audit(
        state.db(),
        AuditEvent {
            organization_id: None,
            workspace_id: None,
            actor_id: admin.id,
            actor_email: &admin.email,
            action: actions::RUNNER_REMOVED,
            resource_type: resources::RUNNER,
            resource_id: Some(id),
            old_values: Some(described(&runner)),
            new_values: None,
        },
    )
    .await;
    Ok(())
}

fn described(runner: &Runner) -> Value {
    json!({
        "name": runner.name,
        "version": runner.version.to_string(),
        "labels": runner.labels.to_vec(),
        "state": runner.state.as_str(),
    })
}
