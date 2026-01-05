//! Chat endpoints

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::chats;
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

impl ErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

/// Chat response
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    id: Uuid,
    workspace_id: Option<Uuid>,
    title: String,
    model_name: String,
    archived: bool,
}

impl From<chats::ChatRow> for ChatResponse {
    fn from(row: chats::ChatRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            title: row.title,
            model_name: row.model_name,
            archived: row.archived.unwrap_or(false),
        }
    }
}

/// Message response
#[derive(Debug, Serialize)]
pub struct MessageResponse {
    id: Uuid,
    chat_id: Uuid,
    role: String,
    content: String,
    metadata: Option<serde_json::Value>,
}

impl From<chats::MessageRow> for MessageResponse {
    fn from(row: chats::MessageRow) -> Self {
        Self {
            id: row.id,
            chat_id: row.chat_id,
            role: row.role,
            content: row.content,
            metadata: row.metadata,
        }
    }
}

/// Query parameters for listing chats
#[derive(Debug, Deserialize)]
pub struct ListChatsQuery {
    workspace_id: Option<Uuid>,
    archived: Option<bool>,
}

/// Create chat request
#[derive(Debug, Deserialize)]
pub struct CreateChatRequest {
    workspace_id: Option<Uuid>,
    title: String,
    model_name: String,
}

/// Update chat request
#[derive(Debug, Deserialize)]
pub struct UpdateChatRequest {
    title: Option<String>,
}

/// Create message request
#[derive(Debug, Deserialize)]
pub struct CreateMessageRequest {
    role: String,
    content: String,
    metadata: Option<serde_json::Value>,
}

/// GET /api/chats
pub async fn list(
    State(state): State<AppState>,
    _auth: AuthUser,
    Query(query): Query<ListChatsQuery>,
) -> impl IntoResponse {
    match chats::list_chats(state.db(), query.workspace_id, query.archived).await {
        Ok(items) => Json(
            items
                .into_iter()
                .map(ChatResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/chats
pub async fn create(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(req): Json<CreateChatRequest>,
) -> impl IntoResponse {
    match chats::create_chat(state.db(), req.workspace_id, &req.title, &req.model_name).await {
        Ok(chat) => (StatusCode::CREATED, Json(ChatResponse::from(chat))).into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// GET /api/chats/:id
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match chats::get_chat(state.db(), id).await {
        Ok(Some(chat)) => Json(ChatResponse::from(chat)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Chat not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// PUT /api/chats/:id
pub async fn update(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateChatRequest>,
) -> impl IntoResponse {
    match chats::update_chat(state.db(), id, req.title.as_deref()).await {
        Ok(Some(chat)) => Json(ChatResponse::from(chat)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Chat not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// DELETE /api/chats/:id
pub async fn delete(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match chats::delete_chat(state.db(), id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Chat not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/chats/:id/archive
pub async fn archive(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match chats::archive_chat(state.db(), id).await {
        Ok(Some(chat)) => Json(ChatResponse::from(chat)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Chat not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/chats/:id/unarchive
pub async fn unarchive(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match chats::unarchive_chat(state.db(), id).await {
        Ok(Some(chat)) => Json(ChatResponse::from(chat)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Chat not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// GET /api/chats/:id/messages
pub async fn list_messages(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match chats::list_messages(state.db(), id).await {
        Ok(msgs) => Json(
            msgs.into_iter()
                .map(MessageResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// POST /api/chats/:id/messages
pub async fn create_message(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateMessageRequest>,
) -> impl IntoResponse {
    match chats::create_message(state.db(), id, &req.role, &req.content, req.metadata).await {
        Ok(msg) => (StatusCode::CREATED, Json(MessageResponse::from(msg))).into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// DELETE /api/chats/:chat_id/messages/:message_id
pub async fn delete_message(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path((chat_id, message_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    match chats::delete_message(state.db(), chat_id, message_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("Message not found")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}
