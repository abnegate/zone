use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgConnection;
use uuid::Uuid;
use zone_enterprise::spend::{Draft, Error, Month, Patch, Target};

use super::access::{self, ORGANIZATION_MISSING};
use super::audited_budget::AuditedBudget;
use super::budget_response::BudgetResponse;
use super::budgets_response::BudgetsResponse;
use super::refusal::Refusal;
use super::{caller, organization};
use crate::auth::{AuthUser, Permission};
use crate::db::audit::{actions, resources};
use crate::db::organization_members;
use crate::db::spend::budgets::{self, BudgetRow};
use crate::db::workspaces;
use crate::routes::common::{AuditEvent, audit};
use crate::services::spend::Standing;
use crate::state::AppState;

pub const BUDGET_MISSING: &str = "Budget not found";

/// GET /api/organizations/{org_id}/budgets
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Response {
    respond(listed(&state, &auth, path).await, StatusCode::OK)
}

/// POST /api/organizations/{org_id}/budgets
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<Draft>, JsonRejection>,
) -> Response {
    respond(
        created(&state, &auth, path, body).await,
        StatusCode::CREATED,
    )
}

/// PATCH /api/organizations/{org_id}/budgets/{budget_id}
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<Patch>, JsonRejection>,
) -> Response {
    respond(updated(&state, &auth, path, body).await, StatusCode::OK)
}

/// DELETE /api/organizations/{org_id}/budgets/{budget_id}
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Response {
    match deleted(&state, &auth, path).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

fn respond<T: Serialize>(result: Result<T, Refusal>, status: StatusCode) -> Response {
    match result {
        Ok(response) => (status, Json(response)).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

async fn listed(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<BudgetsResponse, Refusal> {
    let user = caller(auth)?;
    let organization = organization(path)?;
    let month = Month::current();
    let grant = access::admit(state.db(), organization, user).await?;
    let rows = budgets::list(state.db(), organization, month).await?;
    let standing = Standing::read(state).await;
    Ok(BudgetsResponse {
        entitled: standing.entitled,
        enforced: standing.enforced,
        edition: standing.edition,
        status: standing.status,
        editable: standing.editable(grant.allows(Permission::SpendUpdate)),
        month: month.label(),
        budgets: rows.into_iter().map(BudgetResponse::from).collect(),
    })
}

async fn created(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<Draft>, JsonRejection>,
) -> Result<BudgetResponse, Refusal> {
    let user = caller(auth)?;
    let organization = organization(path)?;
    let standing = Standing::read(state).await;
    let mut transaction = state.db().begin().await?;
    access::administer(&mut transaction, organization, user).await?;
    standing.require()?;
    let Json(draft) = body.map_err(Refusal::Body)?;
    let (target, budget) = draft.validate()?;
    covered(&mut transaction, organization, &target).await?;
    let month = Month::current();
    let row = budgets::create(
        &mut transaction,
        organization,
        &target,
        &budget,
        user,
        month,
    )
    .await?;
    budgets::refresh(&mut transaction, row.id, month).await?;
    transaction.commit().await?;
    record(
        state,
        auth,
        user,
        &row,
        actions::BUDGET_CREATED,
        None,
        Some(audited(&row)),
    )
    .await;
    Ok(row.into())
}

async fn updated(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
    body: Result<Json<Patch>, JsonRejection>,
) -> Result<BudgetResponse, Refusal> {
    let user = caller(auth)?;
    let (organization, budget) = identified(path)?;
    let standing = Standing::read(state).await;
    let mut transaction = state.db().begin().await?;
    access::administer(&mut transaction, organization, user).await?;
    standing.require()?;
    let Json(patch) = body.map_err(Refusal::Body)?;
    let month = Month::current();
    let current = locked(&mut transaction, organization, budget, month).await?;
    let next = patch.applied(&current.budget)?;
    if next == current.budget {
        transaction.rollback().await?;
        return Ok(current.into());
    }
    let saved = budgets::update(
        &mut transaction,
        organization,
        current.id,
        &next,
        user,
        month,
    )
    .await?;
    budgets::refresh(&mut transaction, current.id, month).await?;
    transaction.commit().await?;
    record(
        state,
        auth,
        user,
        &saved,
        actions::BUDGET_UPDATED,
        Some(audited(&current)),
        Some(audited(&saved)),
    )
    .await;
    Ok(saved.into())
}

async fn deleted(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<(), Refusal> {
    let user = caller(auth)?;
    let (organization, budget) = identified(path)?;
    let standing = Standing::read(state).await;
    let mut transaction = state.db().begin().await?;
    access::administer(&mut transaction, organization, user).await?;
    standing.require()?;
    let current = locked(&mut transaction, organization, budget, Month::current()).await?;
    budgets::delete(&mut transaction, organization, current.id).await?;
    transaction.commit().await?;
    record(
        state,
        auth,
        user,
        &current,
        actions::BUDGET_DELETED,
        Some(audited(&current)),
        None,
    )
    .await;
    Ok(())
}

/// The organization and, when it reads as one, the budget a budget path names. A budget that
/// isn't an id is only reported missing once the caller is known to administer the organization.
fn identified(
    path: Result<Path<(String, String)>, PathRejection>,
) -> Result<(Uuid, Option<Uuid>), Refusal> {
    let Ok(Path((organization, budget))) = path else {
        return Err(Refusal::Missing(ORGANIZATION_MISSING));
    };
    let organization =
        Uuid::parse_str(&organization).map_err(|_| Refusal::Missing(ORGANIZATION_MISSING))?;
    Ok((organization, Uuid::parse_str(&budget).ok()))
}

async fn locked(
    connection: &mut PgConnection,
    organization: Uuid,
    budget: Option<Uuid>,
    month: Month,
) -> Result<BudgetRow, Refusal> {
    let Some(budget) = budget else {
        return Err(Refusal::Missing(BUDGET_MISSING));
    };
    budgets::lock(connection, organization, budget, month)
        .await?
        .ok_or(Refusal::Missing(BUDGET_MISSING))
}

async fn covered(
    connection: &mut PgConnection,
    organization: Uuid,
    target: &Target,
) -> Result<(), Refusal> {
    if let Some(workspace) = target.workspace
        && !workspaces::lock_owned(connection, organization, workspace).await?
    {
        return Err(Error::ScopeInvalid.into());
    }
    if let Some(member) = target.user
        && organization_members::lock_membership(connection, organization, member)
            .await?
            .is_none()
    {
        return Err(Error::ScopeInvalid.into());
    }
    Ok(())
}

fn audited(row: &BudgetRow) -> Value {
    json!(AuditedBudget::from(row))
}

async fn record(
    state: &AppState,
    auth: &AuthUser,
    user: Uuid,
    row: &BudgetRow,
    action: &str,
    old_values: Option<Value>,
    new_values: Option<Value>,
) {
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(row.organization_id),
            workspace_id: row.target.workspace,
            actor_id: user,
            actor_email: &auth.0.email,
            action,
            resource_type: resources::BUDGET,
            resource_id: Some(row.id),
            old_values,
            new_values,
        },
    )
    .await;
}
