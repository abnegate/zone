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
use crate::db::{chats, message_embeddings};
use crate::error::ServerError;
use crate::state::AppState;
use crate::workers::embeddings::spawn_message_embedding_task;

use super::common::{ErrorResponse, Timestamps};

/// Maximum search limit
const MAX_SEARCH_LIMIT: usize = 100;

/// Maximum query length
const MAX_QUERY_LENGTH: usize = 10_000;

/// Check if user has read access to workspace
async fn check_workspace_read_access(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: Uuid,
) -> Result<Uuid, ServerError> {
    let user_id = auth.0.user_id().map_err(|e| {
        tracing::error!("Failed to get user ID: {}", e);
        ServerError::Unauthorized("Invalid user".to_string())
    })?;

    match crate::db::workspace_members::can_read(state.db(), workspace_id, user_id).await {
        Ok(true) => Ok(user_id),
        Ok(false) => Err(ServerError::Forbidden(
            "Access denied to workspace".to_string(),
        )),
        Err(e) => {
            tracing::error!("Database error checking workspace access: {}", e);
            Err(ServerError::Internal("Internal server error".to_string()))
        }
    }
}

/// Check if user has write access to workspace
async fn check_workspace_write_access(
    state: &AppState,
    auth: &AuthUser,
    workspace_id: Uuid,
) -> Result<Uuid, ServerError> {
    let user_id = auth.0.user_id().map_err(|e| {
        tracing::error!("Failed to get user ID: {}", e);
        ServerError::Unauthorized("Invalid user".to_string())
    })?;

    match crate::db::workspace_members::can_write(state.db(), workspace_id, user_id).await {
        Ok(true) => Ok(user_id),
        Ok(false) => Err(ServerError::Forbidden(
            "Access denied to workspace".to_string(),
        )),
        Err(e) => {
            tracing::error!("Database error checking workspace access: {}", e);
            Err(ServerError::Internal("Internal server error".to_string()))
        }
    }
}

/// Get chat and verify workspace access
async fn get_chat_with_access(
    state: &AppState,
    auth: &AuthUser,
    chat_id: Uuid,
) -> Result<chats::ChatRow, ServerError> {
    // Get the chat
    let chat = chats::get_chat(state.db(), chat_id)
        .await
        .map_err(|e| {
            tracing::error!("Database error getting chat: {}", e);
            ServerError::Internal("Internal server error".to_string())
        })?
        .ok_or_else(|| ServerError::NotFound("Chat not found".to_string()))?;

    // Require workspace_id - deny access to orphan chats
    let workspace_id = match chat.workspace_id {
        Some(id) => id,
        None => {
            tracing::warn!(
                "Attempted access to chat {} without workspace association",
                chat_id
            );
            return Err(ServerError::Forbidden(
                "Chat has no workspace association".to_string(),
            ));
        }
    };

    // Verify workspace access
    check_workspace_read_access(state, auth, workspace_id).await?;

    Ok(chat)
}

/// Chat response
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    id: Uuid,
    workspace_id: Option<Uuid>,
    title: String,
    model_name: String,
    archived: bool,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl From<chats::ChatRow> for ChatResponse {
    fn from(row: chats::ChatRow) -> Self {
        Self {
            id: row.id,
            workspace_id: row.workspace_id,
            title: row.title,
            model_name: row.model_name,
            archived: row.archived.unwrap_or(false),
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
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
    created_at: String,
}

impl From<chats::MessageRow> for MessageResponse {
    fn from(row: chats::MessageRow) -> Self {
        Self {
            id: row.id,
            chat_id: row.chat_id,
            role: row.role,
            content: row.content,
            metadata: row.metadata,
            created_at: row
                .created_at
                .map(|dt| dt.and_utc().to_rfc3339())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Serialize)]
struct ChatWithMessagesResponse {
    #[serde(flatten)]
    chat: ChatResponse,
    messages: Vec<MessageResponse>,
}

#[derive(Debug, Serialize)]
struct SingleChatResponse {
    chat: ChatWithMessagesResponse,
}

/// Load a chat's messages for a single-chat response. A chat whose messages
/// cannot be read still returns the chat, with an empty list.
async fn chat_with_messages(state: &AppState, chat: chats::ChatRow) -> ChatWithMessagesResponse {
    let messages = chats::list_messages(state.db(), chat.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(MessageResponse::from)
        .collect();

    ChatWithMessagesResponse {
        chat: ChatResponse::from(chat),
        messages,
    }
}

/// Chats list response
#[derive(Debug, Serialize)]
pub struct ChatsListResponse {
    chats: Vec<ChatResponse>,
}

#[derive(Debug, Serialize)]
struct SingleMessageResponse {
    message: MessageResponse,
}

/// Messages list response
#[derive(Debug, Serialize)]
pub struct MessagesListResponse {
    messages: Vec<MessageResponse>,
}

/// Query parameters for listing chats
#[derive(Debug, Deserialize)]
pub struct ListChatsQuery {
    workspace_id: Uuid,
    archived: Option<bool>,
}

/// Create chat request
#[derive(Debug, Deserialize)]
pub struct CreateChatRequest {
    workspace_id: Uuid,
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
    auth: AuthUser,
    Query(query): Query<ListChatsQuery>,
) -> impl IntoResponse {
    // Check workspace access
    if let Err(e) = check_workspace_read_access(&state, &auth, query.workspace_id).await {
        return e.into_response();
    }

    match chats::list_chats(state.db(), Some(query.workspace_id), query.archived).await {
        Ok(items) => Json(ChatsListResponse {
            chats: items.into_iter().map(ChatResponse::from).collect(),
        })
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
    auth: AuthUser,
    Json(req): Json<CreateChatRequest>,
) -> impl IntoResponse {
    // Check workspace write access
    if let Err(e) = check_workspace_write_access(&state, &auth, req.workspace_id).await {
        return e.into_response();
    }

    match chats::create_chat(
        state.db(),
        Some(req.workspace_id),
        &req.title,
        &req.model_name,
    )
    .await
    {
        Ok(chat) => (StatusCode::CREATED, Json(SingleChatResponse { chat: chat_with_messages(&state, chat).await })).into_response(),
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
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Get chat and verify access
    match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => Json(SingleChatResponse { chat: chat_with_messages(&state, chat).await }).into_response(),
        Err(e) => e.into_response(),
    }
}

/// PUT /api/chats/:id
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateChatRequest>,
) -> impl IntoResponse {
    // Get chat and verify access (write access required)
    let chat = match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => chat,
        Err(e) => return e.into_response(),
    };

    // Verify write access to workspace
    if let Some(workspace_id) = chat.workspace_id
        && let Err(e) = check_workspace_write_access(&state, &auth, workspace_id).await
    {
        return e.into_response();
    }

    match chats::update_chat(state.db(), id, req.title.as_deref()).await {
        Ok(Some(chat)) => Json(SingleChatResponse { chat: chat_with_messages(&state, chat).await }).into_response(),
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
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Get chat and verify access (write access required)
    let chat = match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => chat,
        Err(e) => return e.into_response(),
    };

    // Verify write access to workspace
    if let Some(workspace_id) = chat.workspace_id
        && let Err(e) = check_workspace_write_access(&state, &auth, workspace_id).await
    {
        return e.into_response();
    }

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
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Get chat and verify access (write access required)
    let chat = match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => chat,
        Err(e) => return e.into_response(),
    };

    // Verify write access to workspace
    if let Some(workspace_id) = chat.workspace_id
        && let Err(e) = check_workspace_write_access(&state, &auth, workspace_id).await
    {
        return e.into_response();
    }

    match chats::archive_chat(state.db(), id).await {
        Ok(Some(chat)) => Json(SingleChatResponse { chat: chat_with_messages(&state, chat).await }).into_response(),
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
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Get chat and verify access (write access required)
    let chat = match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => chat,
        Err(e) => return e.into_response(),
    };

    // Verify write access to workspace
    if let Some(workspace_id) = chat.workspace_id
        && let Err(e) = check_workspace_write_access(&state, &auth, workspace_id).await
    {
        return e.into_response();
    }

    match chats::unarchive_chat(state.db(), id).await {
        Ok(Some(chat)) => Json(SingleChatResponse { chat: chat_with_messages(&state, chat).await }).into_response(),
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
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Get chat and verify access
    if let Err(e) = get_chat_with_access(&state, &auth, id).await {
        return e.into_response();
    }

    match chats::list_messages(state.db(), id).await {
        Ok(msgs) => Json(MessagesListResponse {
            messages: msgs.into_iter().map(MessageResponse::from).collect(),
        })
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
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateMessageRequest>,
) -> impl IntoResponse {
    // Get chat and verify write access
    let chat = match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => chat,
        Err(e) => return e.into_response(),
    };

    // Verify write access to workspace
    if let Some(workspace_id) = chat.workspace_id
        && let Err(e) = check_workspace_write_access(&state, &auth, workspace_id).await
    {
        return e.into_response();
    }

    match chats::create_message(state.db(), id, &req.role, &req.content, req.metadata).await {
        Ok(msg) => {
            // Spawn background task to generate and store embedding
            // This is non-blocking and allows the message to be returned immediately
            spawn_message_embedding_task(state.clone(), msg.id, msg.chat_id, msg.content.clone());

            (StatusCode::CREATED, Json(SingleMessageResponse { message: MessageResponse::from(msg) })).into_response()
        }
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
    auth: AuthUser,
    Path((chat_id, message_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    // Get chat and verify write access
    let chat = match get_chat_with_access(&state, &auth, chat_id).await {
        Ok(chat) => chat,
        Err(e) => return e.into_response(),
    };

    // Verify write access to workspace
    if let Some(workspace_id) = chat.workspace_id
        && let Err(e) = check_workspace_write_access(&state, &auth, workspace_id).await
    {
        return e.into_response();
    }

    match chats::delete_message(state.db(), chat_id, message_id).await {
        Ok(true) => {
            // Clean up embedding
            if let Err(e) =
                message_embeddings::delete_message_embedding(state.db(), message_id).await
            {
                tracing::warn!(
                    "Failed to delete embedding for message {}: {}",
                    message_id,
                    e
                );
            }
            StatusCode::NO_CONTENT.into_response()
        }
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

/// Query parameters for message search
#[derive(Debug, Deserialize)]
pub struct SearchMessagesQuery {
    query: String,
    workspace_id: Uuid,
    chat_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default = "default_threshold")]
    threshold: f32,
}

fn default_limit() -> usize {
    10
}

fn default_threshold() -> f32 {
    0.7
}

/// Message search result response
#[derive(Debug, Serialize)]
pub struct MessageSearchResponse {
    message_id: Uuid,
    chat_id: Uuid,
    similarity: f32,
    role: String,
    content: String,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl From<message_embeddings::MessageSearchResult> for MessageSearchResponse {
    fn from(result: message_embeddings::MessageSearchResult) -> Self {
        Self {
            message_id: result.message_id,
            chat_id: result.chat_id,
            similarity: result.similarity,
            role: result.role,
            content: result.content,
            timestamps: Timestamps::from_utc(result.created_at, result.created_at),
        }
    }
}

/// Message search results list response
#[derive(Debug, Serialize)]
pub struct MessageSearchListResponse {
    results: Vec<MessageSearchResponse>,
    total: usize,
}

/// GET /api/chats/search
///
/// Search messages by semantic similarity across chat history.
///
/// Query parameters:
/// - `query` (required): The search query text
/// - `workspace_id` (required): Workspace to search within
/// - `chat_id` (optional): Filter results to a specific chat
/// - `limit` (optional): Maximum number of results (default: 10, max: 100)
/// - `threshold` (optional): Minimum similarity score 0.0-1.0 (default: 0.7)
///
/// Returns search results ordered by similarity (highest first).
pub async fn search_messages(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(params): Query<SearchMessagesQuery>,
) -> impl IntoResponse {
    // Check workspace read access
    if let Err(e) = check_workspace_read_access(&state, &auth, params.workspace_id).await {
        return e.into_response();
    }

    // Validate query is not empty
    if params.query.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Query parameter cannot be empty")),
        )
            .into_response();
    }

    // Validate query length
    if params.query.len() > MAX_QUERY_LENGTH {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Query too long")),
        )
            .into_response();
    }

    // Validate threshold range
    if !(0.0..=1.0).contains(&params.threshold) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Threshold must be 0.0-1.0")),
        )
            .into_response();
    }

    // Apply max limit
    let limit = params.limit.min(MAX_SEARCH_LIMIT);

    // If chat_id is specified, verify it belongs to the workspace
    if let Some(chat_id) = params.chat_id {
        match get_chat_with_access(&state, &auth, chat_id).await {
            Ok(chat) => {
                if chat.workspace_id != Some(params.workspace_id) {
                    return (
                        StatusCode::FORBIDDEN,
                        Json(ErrorResponse::new(
                            "Chat does not belong to specified workspace",
                        )),
                    )
                        .into_response();
                }
            }
            Err(e) => return e.into_response(),
        }
    }

    // Get embedding service
    let embedding_service = match state.embedding_service() {
        Some(svc) => svc,
        None => {
            tracing::warn!("Embedding service not available for search");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse::new("Embedding service not available")),
            )
                .into_response();
        }
    };

    // Generate query embedding
    let query_embedding = match embedding_service.embed(&params.query).await {
        Ok(emb) => emb,
        Err(e) => {
            tracing::error!("Failed to generate query embedding: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Failed to generate query embedding")),
            )
                .into_response();
        }
    };

    // Search messages within workspace
    match message_embeddings::search_messages(
        state.db(),
        &query_embedding,
        params.workspace_id,
        params.chat_id,
        limit,
        params.threshold,
    )
    .await
    {
        Ok(results) => {
            // Convert to response format
            let response: Vec<MessageSearchResponse> = results
                .into_iter()
                .map(MessageSearchResponse::from)
                .collect();
            let total = response.len();

            Json(MessageSearchListResponse {
                results: response,
                total,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!("Database error during message search: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}
