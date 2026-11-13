mod commits_response;
mod diff_response;
mod head_query;
mod listing_response;
mod log_response;
mod merge_request;
mod merged_response;
mod refresh_query;
mod refusal_response;

use std::num::NonZeroU64;

use axum::Json;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use serde_json::json;
use uuid::Uuid;

use crate::auth::WorkspaceMember;
use crate::auth::WorkspaceWriter;
use crate::db::audit::actions;
use crate::db::audit::resources;
use crate::error::ServerError;
use crate::routes::common::AuditEvent;
use crate::routes::common::audit;
use crate::services::github::pull_requests;
use crate::services::github::pull_requests::Detail;
use crate::services::github::pull_requests::Error;
use crate::services::github::readiness::CommitSha;
use crate::services::github::snapshots::REFRESH_FLOOR;
use crate::state::AppState;
use commits_response::CommitsResponse;
use diff_response::DiffResponse;
use head_query::HeadQuery;
use head_query::INVALID_HEAD;
use listing_response::ListingResponse;
use log_response::LogResponse;
use merge_request::MergeRequest;
use merged_response::MergedResponse;
use refresh_query::RefreshQuery;
use refusal_response::RefusalResponse;

const INVALID_COMMIT: &str = "sha must be a 40 character commit.";
const INVALID_JOB: &str = "job_id must be positive.";
const INVALID_EXPECTED_HEAD: &str =
    "expected_head must be the pull request's 40 character head commit.";

type Answer<T> = Result<Json<T>, Error>;

/// GET /api/workspaces/{workspace_id}/pull-requests
pub async fn list(
    State(state): State<AppState>,
    member: WorkspaceMember,
    Query(query): Query<RefreshQuery>,
) -> Answer<ListingResponse> {
    let overview = pull_requests::list(&state, member.workspace_id, query.requested()).await?;
    Ok(Json(ListingResponse::new(
        overview,
        state.github_snapshots().pulls.ttl(),
        REFRESH_FLOOR,
    )))
}

/// GET /api/workspaces/{workspace_id}/pull-requests/{source_id}/{number}
pub async fn detail(
    State(state): State<AppState>,
    member: WorkspaceMember,
    Path((_, source_id, number)): Path<(Uuid, Uuid, u64)>,
) -> Answer<Detail> {
    let detail = pull_requests::detail(&state, member.workspace_id, source_id, number).await?;
    Ok(Json(detail))
}

/// GET /api/workspaces/{workspace_id}/pull-requests/{source_id}/{number}/diff
pub async fn diff(
    State(state): State<AppState>,
    member: WorkspaceMember,
    Path((_, source_id, number)): Path<(Uuid, Uuid, u64)>,
    Query(query): Query<HeadQuery>,
) -> Answer<DiffResponse> {
    let head = head(&query)?;
    let diff = pull_requests::diff(&state, member.workspace_id, source_id, number, &head).await?;
    Ok(Json(DiffResponse { diff }))
}

/// GET /api/workspaces/{workspace_id}/pull-requests/{source_id}/{number}/commits
pub async fn commits(
    State(state): State<AppState>,
    member: WorkspaceMember,
    Path((_, source_id, number)): Path<(Uuid, Uuid, u64)>,
    Query(query): Query<HeadQuery>,
) -> Answer<CommitsResponse> {
    let head = head(&query)?;
    let commits =
        pull_requests::commits(&state, member.workspace_id, source_id, number, &head).await?;
    Ok(Json(CommitsResponse { commits }))
}

/// GET /api/workspaces/{workspace_id}/pull-requests/{source_id}/{number}/commits/{sha}
pub async fn commit_diff(
    State(state): State<AppState>,
    member: WorkspaceMember,
    Path((_, source_id, number, sha)): Path<(Uuid, Uuid, u64, String)>,
    Query(query): Query<HeadQuery>,
) -> Answer<DiffResponse> {
    let head = head(&query)?;
    let sha = CommitSha::parse(&sha).ok_or_else(|| invalid(INVALID_COMMIT))?;
    let diff =
        pull_requests::commit_diff(&state, member.workspace_id, source_id, number, &sha, &head)
            .await?;
    Ok(Json(DiffResponse { diff }))
}

/// GET /api/workspaces/{workspace_id}/pull-requests/{source_id}/{number}/jobs/{job_id}/log
pub async fn job_log(
    State(state): State<AppState>,
    member: WorkspaceMember,
    Path((_, source_id, number, job_id)): Path<(Uuid, Uuid, u64, u64)>,
    Query(query): Query<HeadQuery>,
) -> Answer<LogResponse> {
    let head = head(&query)?;
    let job_id = NonZeroU64::new(job_id).ok_or_else(|| invalid(INVALID_JOB))?;
    let log = pull_requests::job_log(
        &state,
        member.workspace_id,
        source_id,
        number,
        job_id,
        &head,
    )
    .await?;
    Ok(Json(LogResponse { log }))
}

/// POST /api/workspaces/{workspace_id}/pull-requests/{source_id}/{number}/merge
pub async fn merge(
    State(state): State<AppState>,
    writer: WorkspaceWriter,
    Path((_, source_id, number)): Path<(Uuid, Uuid, u64)>,
    Json(request): Json<MergeRequest>,
) -> Answer<MergedResponse> {
    let expected =
        CommitSha::parse(&request.expected_head).ok_or_else(|| invalid(INVALID_EXPECTED_HEAD))?;
    let merged = pull_requests::merge(
        &state,
        writer.workspace_id,
        source_id,
        number,
        &expected,
        request.method,
    )
    .await?;
    audit(
        state.db(),
        AuditEvent {
            organization_id: None,
            workspace_id: Some(writer.workspace_id),
            actor_id: writer.user_id,
            actor_email: &writer.email,
            action: actions::PULL_REQUEST_MERGED,
            resource_type: resources::PULL_REQUEST,
            resource_id: Some(source_id),
            old_values: None,
            new_values: Some(json!({
                "source_id": source_id,
                "number": merged.number,
                "head": expected.as_str(),
                "sha": merged.sha,
                "admin": merged.admin,
                "url": merged.url,
            })),
        },
    )
    .await;
    Ok(Json(MergedResponse { merged }))
}

fn head(query: &HeadQuery) -> Result<CommitSha, Error> {
    query.head().ok_or_else(|| invalid(INVALID_HEAD))
}

fn invalid(message: &str) -> Error {
    Error::Invalid(message.to_string())
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        match self {
            Error::Refused(refusal) => {
                (StatusCode::CONFLICT, Json(RefusalResponse::from(refusal))).into_response()
            }
            Error::Missing(message) => ServerError::NotFound(message).into_response(),
            Error::Invalid(message) => ServerError::BadRequest(message).into_response(),
            Error::Disconnected(message) => ServerError::Conflict(message).into_response(),
            Error::Unreadable(message) => ServerError::Internal(message).into_response(),
            Error::Upstream(message) => ServerError::BadGateway(message).into_response(),
        }
    }
}

#[cfg(test)]
mod tests;
