use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use uuid::Uuid;
use zone_enterprise::spend::Uncounted;
use zone_enterprise::{Edition, Status};

use super::query::Query;
use super::refusal::Refusal;
use super::{access, caller, organization};
use crate::auth::AuthUser;
use crate::db::spend::cursor;
use crate::db::spend::report::{self, Day, Filter, NamedUsage, Usage, UserUsage, WorkspaceUsage};
use crate::services::spend::{Standing, coverage};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ReportResponse {
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
    pub editable: bool,
    /// None while the server holds no license, when nothing is attributed.
    pub report: Option<Report>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub ingested_through: Option<DateTime<Utc>>,
    pub total: Usage,
    pub daily: Vec<Day>,
    pub workspaces: Vec<WorkspaceUsage>,
    pub users: Vec<UserUsage>,
    pub models: Vec<NamedUsage>,
    pub providers: Vec<NamedUsage>,
    pub uncounted: Vec<Uncounted>,
}

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
    access::admit(state.db(), organization, user).await?;
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
        editable: standing.editable(true),
        report,
    })
}

async fn measure(state: &AppState, filter: Filter) -> Result<Report, Refusal> {
    let pool = state.db();
    let (ingested_through, total, daily, workspaces, users, models, providers, uncounted) = tokio::try_join!(
        cursor::read(pool),
        report::totals(pool, &filter),
        report::daily(pool, &filter),
        report::by_workspace(pool, &filter),
        report::by_user(pool, &filter),
        report::by_model(pool, &filter),
        report::by_provider(pool, &filter),
        coverage::uncounted(state, filter.organization),
    )?;
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
