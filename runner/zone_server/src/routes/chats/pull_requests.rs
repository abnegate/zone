//! The pull requests a chat opened, read by anyone who can read the chat and
//! changed by those who can write to its workspace.

mod diff_response;
mod pull_request_response;
mod pull_requests_response;
mod update_request;

use axum::{
    Json,
    extract::{Path, State},
};
use uuid::Uuid;

use super::{check_workspace_read_access, check_workspace_write_access, get_chat_with_access};
use crate::auth::AuthUser;
use crate::db::chat_pull_requests::{self, PullRequestRow};
use crate::db::chats::ChatRow;
use crate::db::workspace_members;
use crate::error::ServerError;
use crate::services::chat::pull_request::{self, Error};
use crate::state::AppState;
use diff_response::DiffResponse;
use pull_request_response::PullRequestResponse;
use pull_requests_response::PullRequestsResponse;
use update_request::UpdateRequest;

const CHAT_WITHOUT_WORKSPACE: &str = "Chat has no workspace association";
const PULL_REQUEST_NOT_FOUND: &str = "Pull request not found";
const NOTHING_TO_CHANGE: &str = "Name at least one of dismissed, draft or auto_merge";

/// GET /api/chats/{id}/pull-requests
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<PullRequestsResponse>, ServerError> {
    let (chat, workspace) = chat(&state, &auth, id).await?;
    let writer = writer(&state, &auth, workspace).await?;
    let tracked = pull_request::list(&state, workspace, chat.id).await?;
    Ok(Json(PullRequestsResponse {
        pull_requests: tracked
            .into_iter()
            .map(|tracked| PullRequestResponse::new(tracked, writer))
            .collect(),
    }))
}

/// PATCH /api/chats/{id}/pull-requests/{pull_request_id}
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, pull_request_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateRequest>,
) -> Result<Json<PullRequestResponse>, ServerError> {
    if request.is_empty() {
        return Err(ServerError::BadRequest(NOTHING_TO_CHANGE.to_string()));
    }
    let (chat, workspace) = chat(&state, &auth, id).await?;
    check_workspace_write_access(&state, &auth, workspace).await?;
    let mut row = owned(&state, chat.id, pull_request_id).await?;
    if let Some(draft) = request.draft {
        pull_request::set_draft(&state, workspace, &row, draft)
            .await
            .map_err(refused)?;
    }
    if let Some(enabled) = request.auto_merge {
        pull_request::set_auto_merge(&state, workspace, &row, enabled)
            .await
            .map_err(refused)?;
    }
    if let Some(dismissed) = request.dismissed {
        row = chat_pull_requests::dismiss(state.db(), chat.id, row.id, dismissed)
            .await?
            .ok_or_else(|| ServerError::NotFound(PULL_REQUEST_NOT_FOUND.to_string()))?;
    }
    let tracked = pull_request::track(&state, workspace, row).await?;
    Ok(Json(PullRequestResponse::new(tracked, true)))
}

/// GET /api/chats/{id}/pull-requests/{pull_request_id}/diff
pub async fn diff(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, pull_request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<DiffResponse>, ServerError> {
    let (chat, workspace) = chat(&state, &auth, id).await?;
    let row = owned(&state, chat.id, pull_request_id).await?;
    let excerpt = pull_request::diff(&state, workspace, &row)
        .await
        .map_err(refused)?;
    Ok(Json(DiffResponse {
        diff: excerpt.text,
        truncated: excerpt.truncated,
    }))
}

async fn chat(state: &AppState, auth: &AuthUser, id: Uuid) -> Result<(ChatRow, Uuid), ServerError> {
    let chat = get_chat_with_access(state, auth, id).await?;
    let workspace = chat
        .workspace_id
        .ok_or_else(|| ServerError::Forbidden(CHAT_WITHOUT_WORKSPACE.to_string()))?;
    Ok((chat, workspace))
}

async fn writer(state: &AppState, auth: &AuthUser, workspace: Uuid) -> Result<bool, ServerError> {
    let user = check_workspace_read_access(state, auth, workspace).await?;
    Ok(workspace_members::can_write(state.db(), workspace, user).await?)
}

async fn owned(state: &AppState, chat: Uuid, id: Uuid) -> Result<PullRequestRow, ServerError> {
    chat_pull_requests::get(state.db(), chat, id)
        .await?
        .ok_or_else(|| ServerError::NotFound(PULL_REQUEST_NOT_FOUND.to_string()))
}

fn refused(error: Error) -> ServerError {
    match error {
        Error::Uncredentialed | Error::Refused(_) => ServerError::Conflict(error.to_string()),
        Error::Unreachable | Error::Storage => ServerError::Internal(error.to_string()),
    }
}
