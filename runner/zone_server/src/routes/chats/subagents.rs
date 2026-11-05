//! The subagents a chat's coding agent started, read by anyone who can read
//! the chat.

mod subagents_response;

use axum::{
    Json,
    extract::{Path, State},
};
use uuid::Uuid;

use super::get_chat_with_access;
use crate::auth::AuthUser;
use crate::db::chat_subagents;
use crate::error::ServerError;
use crate::services::chat::subagent::ChatSubagent;
use crate::state::AppState;
use subagents_response::SubagentsResponse;

/// GET /api/chats/{id}/subagents
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<SubagentsResponse>, ServerError> {
    let chat = get_chat_with_access(&state, &auth, id).await?;
    let rows = chat_subagents::list(state.db(), chat.id).await?;
    Ok(Json(SubagentsResponse {
        subagents: rows.into_iter().map(ChatSubagent::from).collect(),
    }))
}
