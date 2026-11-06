//! The subagents a chat's coding agent started, and the steps each took, read
//! by anyone who can read the chat.

mod response;
mod steps;

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
use response::Response;
use steps::Steps;

/// GET /api/chats/{id}/subagents
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Response>, ServerError> {
    let chat = get_chat_with_access(&state, &auth, id).await?;
    let rows = chat_subagents::list(state.db(), chat.id).await?;
    Ok(Json(Response {
        subagents: rows.into_iter().map(ChatSubagent::from).collect(),
    }))
}

/// GET /api/chats/{id}/subagents/{subagent_id}/steps
pub async fn steps(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, subagent_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Steps>, ServerError> {
    let chat = get_chat_with_access(&state, &auth, id).await?;
    let steps = chat_subagents::steps(state.db(), chat.id, subagent_id)
        .await?
        .ok_or_else(|| ServerError::NotFound("Subagent not found".to_string()))?;
    Ok(Json(Steps { steps }))
}
