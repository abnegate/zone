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
use zone_core::vpn::CHAT_OFFLINE;

use super::{check_workspace_write_access, get_chat_with_access};
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
const NOTHING_TO_CHANGE: &str = "Name at least one of dismissed, draft, auto_merge or auto_fix";

/// GET /api/chats/{id}/pull-requests
///
/// An offline chat lists none: each one's status is read from GitHub, and an
/// offline chat makes no request beyond its own model.
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<PullRequestsResponse>, ServerError> {
    let (chat, workspace) = chat(&state, &auth, id).await?;
    if chat.offline {
        return Ok(Json(PullRequestsResponse {
            pull_requests: Vec::new(),
        }));
    }
    let writable = writable(&state, &auth, workspace).await?;
    let tracked = pull_request::list(&state, workspace, chat.id).await?;
    Ok(Json(PullRequestsResponse {
        pull_requests: tracked
            .into_iter()
            .map(|tracked| PullRequestResponse::new(tracked, writable))
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
    let (chat, workspace) = online(chat(&state, &auth, id).await?)?;
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
    if let Some(enabled) = request.auto_fix {
        row = pull_request::set_auto_fix(&state, chat.id, &row, enabled, user(&auth)?)
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
    let (chat, workspace) = online(chat(&state, &auth, id).await?)?;
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

/// `chat` when it may reach GitHub, which every change and diff does.
fn online((chat, workspace): (ChatRow, Uuid)) -> Result<(ChatRow, Uuid), ServerError> {
    if chat.offline {
        return Err(ServerError::Conflict(CHAT_OFFLINE.to_string()));
    }
    Ok((chat, workspace))
}

async fn writable(state: &AppState, auth: &AuthUser, workspace: Uuid) -> Result<bool, ServerError> {
    Ok(workspace_members::can_write(state.db(), workspace, user(auth)?).await?)
}

fn user(auth: &AuthUser) -> Result<Uuid, ServerError> {
    auth.0
        .user_id()
        .map_err(|_| ServerError::Unauthorized("Invalid user".to_string()))
}

async fn owned(state: &AppState, chat: Uuid, id: Uuid) -> Result<PullRequestRow, ServerError> {
    chat_pull_requests::get(state.db(), chat, id)
        .await?
        .ok_or_else(|| ServerError::NotFound(PULL_REQUEST_NOT_FOUND.to_string()))
}

fn refused(error: Error) -> ServerError {
    match error {
        Error::Uncredentialed
        | Error::Disconnected
        | Error::Unauthenticated
        | Error::Refused(_) => ServerError::Conflict(error.to_string()),
        Error::Unreachable => ServerError::BadGateway(error.to_string()),
        Error::Storage | Error::Unsaved => ServerError::Internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::jwt::Claims;
    use crate::services::github::testing::connected::Connected;

    const ONLINE: i64 = 11;
    const OFFLINE: i64 = 12;

    fn reader(connected: &Connected) -> AuthUser {
        AuthUser(Claims {
            sub: connected.user.to_string(),
            email: format!("{}@zone.test", connected.user),
            roles: Vec::new(),
            permissions: Vec::new(),
            exp: i64::MAX,
            iat: 0,
            jti: String::new(),
            is_admin: false,
        })
    }

    #[tokio::test]
    async fn an_offline_chat_lists_no_pull_request_and_asks_github_nothing() {
        let connected = Connected::new().await;
        connected.reads_only(Some(ONLINE)).await;
        let online = connected.chat_pull_request(false, ONLINE).await;
        let offline = connected.chat_pull_request(true, OFFLINE).await;

        let Json(listed) = list(
            State(connected.state.clone()),
            reader(&connected),
            Path(offline.chat_id),
        )
        .await
        .expect("an offline chat's pull requests");
        assert!(
            listed.pull_requests.is_empty(),
            "an offline chat's pull request is only shown with what GitHub says of it"
        );

        let Json(listed) = list(
            State(connected.state.clone()),
            reader(&connected),
            Path(online.chat_id),
        )
        .await
        .expect("an online chat's pull requests");
        let ids: Vec<Uuid> = listed.pull_requests.iter().map(|one| one.id).collect();
        assert_eq!(ids, [online.id], "an online chat still lists its own");

        connected.github.verify().await;
    }

    #[tokio::test]
    async fn an_offline_chat_neither_changes_nor_diffs_a_pull_request() {
        let connected = Connected::new().await;
        connected.reads_only(None).await;
        let offline = connected.chat_pull_request(true, OFFLINE).await;

        let changed = update(
            State(connected.state.clone()),
            reader(&connected),
            Path((offline.chat_id, offline.id)),
            Json(UpdateRequest {
                draft: Some(true),
                ..UpdateRequest::default()
            }),
        )
        .await;
        assert!(
            matches!(&changed, Err(ServerError::Conflict(message)) if message == CHAT_OFFLINE),
            "{changed:?}"
        );

        let diffed = diff(
            State(connected.state.clone()),
            reader(&connected),
            Path((offline.chat_id, offline.id)),
        )
        .await;
        assert!(
            matches!(&diffed, Err(ServerError::Conflict(message)) if message == CHAT_OFFLINE),
            "{diffed:?}"
        );

        connected.github.verify().await;
    }

    #[test]
    fn github_out_of_reach_is_a_bad_gateway() {
        assert!(matches!(
            refused(Error::Unreachable),
            ServerError::BadGateway(message) if message == Error::Unreachable.to_string()
        ));
        assert!(matches!(refused(Error::Storage), ServerError::Internal(_)));
        assert!(matches!(refused(Error::Unsaved), ServerError::Internal(_)));
        for error in [
            Error::Uncredentialed,
            Error::Disconnected,
            Error::Unauthenticated,
            Error::Refused("Resource not accessible by integration".to_string()),
        ] {
            assert!(matches!(refused(error), ServerError::Conflict(_)));
        }
    }
}
