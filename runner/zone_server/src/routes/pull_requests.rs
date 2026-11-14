mod commits_response;
mod diff_response;
mod head_query;
mod listing_response;
mod log_response;
mod merge_request;
mod merged_response;
mod merging;
mod refusal_response;

use std::num::NonZeroU64;

use axum::Json;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::extract::rejection::PathRejection;
use axum::extract::rejection::QueryRejection;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use uuid::Uuid;

use crate::auth::PullRequestMerger;
use crate::auth::WorkspaceMember;
use crate::error::ServerError;
use crate::routes::refresh_query::RefreshQuery;
use crate::services::github::pull_requests;
use crate::services::github::pull_requests::Detail;
use crate::services::github::pull_requests::Error;
use crate::services::github::readiness::CommitSha;
use crate::state::AppState;
use commits_response::CommitsResponse;
use diff_response::DiffResponse;
use head_query::HeadQuery;
use head_query::INVALID_HEAD;
use listing_response::ListingResponse;
use log_response::LogResponse;
use merge_request::MergeRequest;
use merged_response::MergedResponse;
use merging::Merging;
use refusal_response::RefusalResponse;

const INVALID_COMMIT: &str = "sha must be a 40 character commit.";
const INVALID_JOB: &str = "job_id must be positive.";
const INVALID_EXPECTED_HEAD: &str =
    "expected_head must be the pull request's 40 character head commit.";
const OUTCOME_UNKNOWN: &str = "The merge stopped before it reported back, so whether it happened is unknown. Reload the pull request before trying again.";

type Answer<T> = Result<Json<T>, Error>;
type PullPath = Result<Path<(Uuid, Uuid, u64)>, PathRejection>;
type Headed = Result<Query<HeadQuery>, QueryRejection>;

pub async fn list(
    State(state): State<AppState>,
    member: WorkspaceMember,
    query: Result<Query<RefreshQuery>, QueryRejection>,
) -> Answer<ListingResponse> {
    let Query(query) = query?;
    let overview = pull_requests::list(&state, member.workspace_id, query.requested()).await?;
    let snapshots = state.github_snapshots();
    Ok(Json(ListingResponse::new(
        overview,
        snapshots.pull_requests.ttl(),
        snapshots.pull_requests.floor(),
    )))
}

pub async fn detail(
    State(state): State<AppState>,
    member: WorkspaceMember,
    path: PullPath,
) -> Answer<Detail> {
    let Path((_, source_id, number)) = path?;
    let detail = pull_requests::detail(&state, member.workspace_id, source_id, number).await?;
    Ok(Json(detail))
}

pub async fn diff(
    State(state): State<AppState>,
    member: WorkspaceMember,
    path: PullPath,
    query: Headed,
) -> Answer<DiffResponse> {
    let Path((_, source_id, number)) = path?;
    let head = head(query)?;
    let diff = pull_requests::diff(&state, member.workspace_id, source_id, number, &head).await?;
    Ok(Json(DiffResponse { diff }))
}

pub async fn commits(
    State(state): State<AppState>,
    member: WorkspaceMember,
    path: PullPath,
    query: Headed,
) -> Answer<CommitsResponse> {
    let Path((_, source_id, number)) = path?;
    let head = head(query)?;
    let commits =
        pull_requests::commits(&state, member.workspace_id, source_id, number, &head).await?;
    Ok(Json(CommitsResponse { commits }))
}

pub async fn commit_diff(
    State(state): State<AppState>,
    member: WorkspaceMember,
    path: Result<Path<(Uuid, Uuid, u64, String)>, PathRejection>,
    query: Headed,
) -> Answer<DiffResponse> {
    let Path((_, source_id, number, sha)) = path?;
    let head = head(query)?;
    let sha = CommitSha::parse(&sha).ok_or_else(|| invalid(INVALID_COMMIT))?;
    let diff =
        pull_requests::commit_diff(&state, member.workspace_id, source_id, number, &sha, &head)
            .await?;
    Ok(Json(DiffResponse { diff }))
}

pub async fn job_log(
    State(state): State<AppState>,
    member: WorkspaceMember,
    path: Result<Path<(Uuid, Uuid, u64, u64)>, PathRejection>,
    query: Headed,
) -> Answer<LogResponse> {
    let Path((_, source_id, number, job_id)) = path?;
    let head = head(query)?;
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

pub async fn merge(
    State(state): State<AppState>,
    PullRequestMerger(writer): PullRequestMerger,
    path: PullPath,
    request: Result<Json<MergeRequest>, JsonRejection>,
) -> Answer<MergedResponse> {
    let Path((_, source_id, number)) = path?;
    let Json(request) = request?;
    let expected =
        CommitSha::parse(&request.expected_head).ok_or_else(|| invalid(INVALID_EXPECTED_HEAD))?;
    let merging = Merging {
        writer,
        source_id,
        number,
        expected,
        method: request.method,
    };
    let merged = tokio::spawn(merging.run(state)).await.map_err(|error| {
        tracing::error!(%error, source = %source_id, number, "A merge did not report back");
        Error::Internal(OUTCOME_UNKNOWN.to_string())
    })??;
    Ok(Json(MergedResponse { merged }))
}

fn head(query: Headed) -> Result<CommitSha, Error> {
    let Query(query) = query?;
    query.head().ok_or_else(|| invalid(INVALID_HEAD))
}

fn invalid(message: &str) -> Error {
    Error::Invalid(message.to_string())
}

impl From<PathRejection> for Error {
    fn from(rejection: PathRejection) -> Self {
        Self::Invalid(rejection.body_text())
    }
}

impl From<QueryRejection> for Error {
    fn from(rejection: QueryRejection) -> Self {
        Self::Invalid(rejection.body_text())
    }
}

impl From<JsonRejection> for Error {
    fn from(rejection: JsonRejection) -> Self {
        Self::Invalid(rejection.body_text())
    }
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
            Error::Internal(message) => ServerError::Internal(message).into_response(),
            Error::Upstream(fault) => ServerError::BadGateway(fault.to_string()).into_response(),
        }
    }
}

#[cfg(test)]
mod tests;
