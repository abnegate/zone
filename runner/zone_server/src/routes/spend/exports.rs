use axum::body::{Body, Bytes};
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use futures::{StreamExt, TryStreamExt, stream};
use uuid::Uuid;
use zone_enterprise::Feature;
use zone_enterprise::spend::csv;

use super::query::Query;
use super::refusal::Refusal;
use super::{access, caller, organization};
use crate::auth::AuthUser;
use crate::auth::entitlement::Unentitled;
use crate::db::audit::{actions, resources};
use crate::db::spend::report;
use crate::routes::common::{AuditEvent, audit};
use crate::services::spend::Standing;
use crate::state::AppState;

pub const CONTENT_TYPE: &str = "text/csv; charset=utf-8";

const NO_STORE: &str = "no-store";

/// GET /api/organizations/{org_id}/spend-exports
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    download(&state, &auth, path, query.as_deref())
        .await
        .unwrap_or_else(IntoResponse::into_response)
}

pub fn filename(query: &Query) -> String {
    format!("zone-spend-{}-{}.csv", query.range.from, query.range.to)
}

async fn download(
    state: &AppState,
    auth: &AuthUser,
    path: Result<Path<Uuid>, PathRejection>,
    query: Option<&str>,
) -> Result<Response, Refusal> {
    let user = caller(auth)?;
    let organization = organization(path)?;
    access::admit(state.db(), organization, user).await?;
    let query = Query::read(query, Utc::now().date_naive())?;
    if !Standing::read(state).await.enforced {
        return Err(Unentitled(Feature::Spend).into());
    }
    audit(
        state.db(),
        AuditEvent {
            organization_id: Some(organization),
            workspace_id: None,
            actor_id: user,
            actor_email: &auth.0.email,
            action: actions::SPEND_EXPORT_DOWNLOADED,
            resource_type: resources::SPEND_EXPORT,
            resource_id: Some(organization),
            old_values: None,
            new_values: Some(query.audited()),
        },
    )
    .await;
    let heading =
        stream::once(async { Ok::<_, sqlx::Error>(Bytes::from_static(csv::HEADER.as_bytes())) });
    let rows = report::lines(state.db().clone(), query.filter(organization))
        .map_ok(|line| Bytes::from(csv::row(&line)))
        .inspect_err(|error| tracing::error!(%error, "A spend export stopped part way"));
    let disposition = format!("attachment; filename=\"{}\"", filename(&query));
    Ok((
        [
            (header::CONTENT_TYPE, CONTENT_TYPE.to_owned()),
            (header::CONTENT_DISPOSITION, disposition),
            (header::CACHE_CONTROL, NO_STORE.to_owned()),
        ],
        Body::from_stream(heading.chain(rows)),
    )
        .into_response())
}
