use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, TimeDelta, Utc};
use futures::TryStreamExt;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;
use zone_enterprise::Feature;
use zone_enterprise::audit::{Error, Format, Standing};

use super::refusal::Refusal;
use super::{access, parse_end, parse_start};
use crate::auth::entitlement::Unentitled;
use crate::auth::{AuthUser, ServerAdmin};
use crate::db::DbResult;
use crate::db::audit::events::Range;
use crate::db::audit::scope::Scope;
use crate::db::audit::{AuditContext, actions, log_action, resources};
use crate::services::audit::export;
use crate::state::AppState;

const NO_STORE: &str = "no-store";
const LAST_MICROSECOND: TimeDelta = TimeDelta::microseconds(999_999);

#[derive(Debug, Default, Deserialize)]
pub struct ExportQuery {
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Export {
    format: Format,
    range: Range,
}

impl Export {
    fn parse(query: &ExportQuery) -> Result<Self, Error> {
        let format = match query.format.as_deref() {
            None => Format::default(),
            Some(value) => Format::ALL
                .into_iter()
                .find(|format| format.as_str() == value)
                .ok_or(Error::ExportFormatInvalid)?,
        };
        let start = query
            .start_date
            .as_deref()
            .and_then(parse_start)
            .ok_or(Error::ExportRangeInvalid)?;
        let end = query
            .end_date
            .as_deref()
            .and_then(end)
            .ok_or(Error::ExportRangeInvalid)?;
        if start > end {
            return Err(Error::ExportRangeInvalid);
        }
        Ok(Self {
            format,
            range: Range { start, end },
        })
    }
}

/// `parse_end` ends a calendar date at 23:59:59, which would drop rows written during that
/// day's last second; audit rows carry microseconds.
fn end(value: &str) -> Option<DateTime<Utc>> {
    let end = parse_end(value)?;
    if DateTime::parse_from_rfc3339(value).is_ok() {
        return Some(end);
    }
    Some(end + LAST_MICROSECOND)
}

struct Actor {
    id: Uuid,
    email: String,
}

pub async fn organization(
    State(state): State<AppState>,
    Path(organization): Path<Uuid>,
    Query(query): Query<ExportQuery>,
    AuthUser(claims): AuthUser,
) -> Result<Response, Refusal> {
    let user = claims.user_id().map_err(|_| Refusal::Unauthenticated)?;
    let mut connection = state.db().acquire().await?;
    access::organization(&mut connection, organization, user).await?;
    drop(connection);
    let actor = Actor {
        id: user,
        email: claims.email,
    };
    download(&state, Scope::Organization(organization), &actor, &query).await
}

pub async fn server(
    State(state): State<AppState>,
    Query(query): Query<ExportQuery>,
    ServerAdmin(admin): ServerAdmin,
) -> Result<Response, Refusal> {
    let actor = Actor {
        id: admin.id,
        email: admin.email,
    };
    download(&state, Scope::Server, &actor, &query).await
}

async fn download(
    state: &AppState,
    scope: Scope,
    actor: &Actor,
    query: &ExportQuery,
) -> Result<Response, Refusal> {
    entitled(state).await?;
    let export = Export::parse(query)?;
    if let Err(error) = record(state, scope, actor, export).await {
        tracing::error!(%error, "Refused an audit export whose own audit row could not be written");
        return Ok(StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }
    let stream = export::stream(state.db().clone(), scope, export.format, export.range)
        .inspect_err(|error| tracing::error!(%error, "An audit export stopped part way"));
    let disposition = format!(
        "attachment; filename=\"{}\"",
        export::filename(scope, export.range, export.format)
    );
    Ok((
        [
            (
                header::CONTENT_TYPE,
                export.format.content_type().to_owned(),
            ),
            (header::CONTENT_DISPOSITION, disposition),
            (header::CACHE_CONTROL, NO_STORE.to_owned()),
        ],
        Body::from_stream(stream),
    )
        .into_response())
}

async fn entitled(state: &AppState) -> Result<(), Refusal> {
    let stored = state.stored_license().await?;
    let standing = Standing::of(stored.license(), stored.refusal().is_some(), Utc::now());
    if standing.export {
        return Ok(());
    }
    Err(Unentitled(Feature::AuditExport).into())
}

async fn record(state: &AppState, scope: Scope, actor: &Actor, export: Export) -> DbResult<Uuid> {
    let context = AuditContext {
        org_id: scope.organization_id(),
        workspace_id: None,
        actor_id: Some(actor.id),
        actor_email: Some(actor.email.clone()),
        ip_address: None,
        user_agent: None,
    };
    log_action(
        state.db(),
        &context,
        actions::AUDIT_EXPORT_DOWNLOADED,
        resources::AUDIT_EXPORT,
        None,
        None,
        Some(json!({
            "format": export.format.as_str(),
            "start_date": export.range.start,
            "end_date": export.range.end,
        })),
    )
    .await
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use http_body_util::BodyExt;

    use super::*;

    fn query(format: Option<&str>, start: Option<&str>, end: Option<&str>) -> ExportQuery {
        ExportQuery {
            format: format.map(str::to_owned),
            start_date: start.map(str::to_owned),
            end_date: end.map(str::to_owned),
        }
    }

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .unwrap()
    }

    #[test]
    fn the_format_defaults_to_ndjson_and_dates_cover_whole_days() {
        let export = Export::parse(&query(None, Some("2026-11-01"), Some("2026-11-15"))).unwrap();
        assert_eq!(export.format, Format::Ndjson);
        assert_eq!(export.range.start, at(2026, 11, 1, 0, 0, 0));
        assert_eq!(
            export.range.end,
            at(2026, 11, 15, 23, 59, 59) + TimeDelta::microseconds(999_999)
        );
    }

    #[test]
    fn instants_are_kept_as_given() {
        let export = Export::parse(&query(
            Some("csv"),
            Some("2026-11-01T10:00:00+02:00"),
            Some("2026-11-01T10:00:00Z"),
        ))
        .unwrap();
        assert_eq!(export.format, Format::Csv);
        assert_eq!(export.range.start, at(2026, 11, 1, 8, 0, 0));
        assert_eq!(export.range.end, at(2026, 11, 1, 10, 0, 0));
    }

    #[test]
    fn a_single_day_is_a_valid_range() {
        assert!(Export::parse(&query(None, Some("2026-11-01"), Some("2026-11-01"))).is_ok());
    }

    #[test]
    fn an_unknown_format_is_refused_with_the_format_code() {
        for format in ["json", "CSV", ""] {
            let error = Export::parse(&query(Some(format), Some("2026-11-01"), Some("2026-11-02")))
                .unwrap_err();
            assert_eq!(error, Error::ExportFormatInvalid, "{format}");
        }
    }

    #[test]
    fn a_missing_unreadable_or_reversed_range_is_refused_with_the_range_code() {
        let cases = [
            query(None, None, Some("2026-11-02")),
            query(None, Some("2026-11-01"), None),
            query(None, Some("yesterday"), Some("2026-11-02")),
            query(None, Some("2026-11-01"), Some("2026-13-01")),
            query(None, Some("2026-11-02"), Some("2026-11-01")),
        ];
        for case in cases {
            assert_eq!(
                Export::parse(&case).unwrap_err(),
                Error::ExportRangeInvalid,
                "{case:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_refusal_carries_the_enterprise_status_and_code() {
        let response = Refusal::from(Error::ExportRangeInvalid).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["code"], "audit_export_range_invalid");
    }
}
