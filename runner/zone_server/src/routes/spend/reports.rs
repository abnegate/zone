use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use uuid::Uuid;

use super::query::Query;
use super::refusal::Refusal;
use super::report::Report;
use super::report_response::ReportResponse;
use super::{access, caller, organization};
use crate::auth::{AuthUser, Permission};
use crate::db::spend::cursor;
use crate::db::spend::report::{self, Filter};
use crate::services::spend::{Standing, coverage};
use crate::state::AppState;

/// GET /api/organizations/{org_id}/spend-reports
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    match read(&state, &auth, path, query.as_deref()).await {
        Ok(response) => Json(response).into_response(),
        Err(refusal) => refusal.into_response(),
    }
}

async fn read(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    query: Option<&str>,
) -> Result<ReportResponse, Refusal> {
    let user = caller(auth)?;
    let organization = organization(path)?;
    let grant = access::admit(state.db(), organization, user).await?;
    let query = Query::read(query, Utc::now().date_naive())?;
    let standing = Standing::read(state).await;
    let report = if standing.enforced {
        Some(measure(state, query.filter(organization)).await?)
    } else {
        None
    };
    Ok(ReportResponse {
        entitled: standing.entitled,
        enforced: standing.enforced,
        edition: standing.edition,
        status: standing.status,
        editable: standing.editable(grant.allows(Permission::SpendUpdate)),
        report,
    })
}

/// Reads one connection's worth at a time, so a report never takes more of the pool than one
/// connection for its spend and a few for its coverage.
async fn measure(state: &AppState, filter: Filter) -> Result<Report, Refusal> {
    let pool = state.db();
    let ingested_through = cursor::read(pool).await?;
    let mut snapshot = report::snapshot(pool).await?;
    let total = report::totals(&mut snapshot, &filter).await?;
    let daily = report::daily(&mut snapshot, &filter).await?;
    let workspaces = report::by_workspace(&mut snapshot, &filter).await?;
    let users = report::by_user(&mut snapshot, &filter).await?;
    let models = report::by_model(&mut snapshot, &filter).await?;
    let providers = report::by_provider(&mut snapshot, &filter).await?;
    snapshot.commit().await?;
    let uncounted = coverage::uncounted(state, filter.organization).await?;
    Ok(Report {
        from: filter.range.from,
        to: filter.range.to,
        ingested_through,
        total,
        daily,
        workspaces,
        users,
        models,
        providers,
        uncounted,
    })
}
