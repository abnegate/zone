//! The tasks a chat's agent created or started, read by anyone who can read
//! the chat.

mod publication_response;
mod run_response;
mod task_response;
mod tasks_response;

use axum::{
    Json,
    extract::{Path, State},
};
use uuid::Uuid;

use super::get_chat_with_access;
use crate::auth::AuthUser;
use crate::db::chat_tasks;
use crate::error::ServerError;
use crate::state::AppState;
use task_response::TaskResponse;
use tasks_response::TasksResponse;

const CHAT_WITHOUT_WORKSPACE: &str = "Chat has no workspace association";

/// GET /api/chats/{id}/tasks
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<TasksResponse>, ServerError> {
    let chat = get_chat_with_access(&state, &auth, id).await?;
    let workspace = chat
        .workspace_id
        .ok_or_else(|| ServerError::Forbidden(CHAT_WITHOUT_WORKSPACE.to_string()))?;
    let rows = chat_tasks::list(state.db(), workspace, chat.id).await?;
    Ok(Json(TasksResponse {
        tasks: rows.into_iter().map(TaskResponse::from).collect(),
    }))
}
