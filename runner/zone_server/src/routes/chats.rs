//! Chat endpoints

pub(super) mod provider;
pub(super) mod pull_requests;
mod snippet;
pub(super) mod subagents;
pub(super) mod tasks;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::{chat_attached_sources, chats, message_embeddings, projects};
use crate::error::ServerError;
use crate::services::artifacts::ArtifactStore;
use crate::services::character::ChatCharacter;
use crate::services::chat::authorship;
use crate::services::chat::driver::Driver;
use crate::services::chat::indicator::{self, Indicator};
use crate::services::chat::session;
use crate::services::model::Model;
use crate::services::provider::Provider;
use crate::services::route::Route;
use crate::state::AppState;
use crate::workers::embeddings::spawn_message_embedding_task;
use abnegate_agent::context::ContextUsage;

use super::common::{ErrorResponse, Timestamps};

/// Maximum search limit
const MAX_SEARCH_LIMIT: usize = 100;

const MAX_LIST_LIMIT: i64 = 100;

/// Maximum query length
const MAX_QUERY_LENGTH: usize = 10_000;

/// Check if user has read access to workspace
pub(crate) async fn check_workspace_read_access(
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
    agent_enabled: bool,
    /// When true the agent may only call the tools zone hands it. When false it
    /// also keeps its own file and shell tools, which zone never sees.
    agent_sandboxed: bool,
    auto_approve: bool,
    offline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_tokens: Option<i64>,
    reasoning_effort: abnegate_llm::ReasoningEffort,
    #[serde(skip_serializing_if = "Option::is_none")]
    character: Option<ChatCharacter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    needs_character: Option<bool>,
    /// What the chat is for, when it is more than an assistant: a project
    /// planner, or a project's updates.
    #[serde(skip_serializing_if = "Option::is_none")]
    purpose: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<Uuid>,
    /// The provider the chat chose, null while it follows its workspace's. A chat whose choice
    /// couldn't be read leaves it out.
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<Option<Provider>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_model: Option<Option<String>>,
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
            agent_enabled: row.agent_enabled,
            agent_sandboxed: row.agent_sandboxed,
            auto_approve: row.auto_approve,
            offline: row.offline,
            context_tokens: row.context_tokens,
            reasoning_effort: row.reasoning_effort,
            character: row.character,
            tools: None,
            reasoning: None,
            needs_character: None,
            purpose: None,
            project_id: None,
            provider: None,
            provider_model: None,
            timestamps: Timestamps::from_naive(row.created_at, row.updated_at),
        }
    }
}

impl ChatResponse {
    /// Carry the chat's purpose and project into the response.
    fn with_link(mut self, link: Option<chats::ChatLink>) -> Self {
        if let Some(link) = link {
            if link.purpose != chats::ChatPurpose::Assistant {
                self.purpose = Some(link.purpose.as_str().to_string());
            }
            self.project_id = link.project_id;
        }
        self
    }

    /// Carry the provider the chat chose, or that it follows its workspace's.
    /// A choice that couldn't be read is left out rather than reported as
    /// following the workspace.
    fn with_choice(mut self, choice: Option<Option<&chats::provider::Choice>>) -> Self {
        if let Some(choice) = choice {
            self.provider = Some(choice.map(|choice| choice.provider));
            self.provider_model = Some(choice.and_then(|choice| choice.model.clone()));
        }
        self
    }

    fn with_profile(mut self, profile: crate::services::model::ModelProfile) -> Self {
        self.tools = profile.tools;
        self.reasoning = profile.reasoning;
        self.needs_character = Some(profile.needs_character);
        self
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
    context: Option<ContextUsage>,
}

#[derive(Debug, Serialize)]
struct SingleChatResponse {
    chat: ChatWithMessagesResponse,
}

/// Load a chat's messages for a single-chat response. A chat whose messages
/// cannot be read still returns the chat, with an empty list.
async fn chat_with_messages(
    state: &AppState,
    auth: &AuthUser,
    chat: chats::ChatRow,
) -> ChatWithMessagesResponse {
    let route = match chat.workspace_id {
        Some(_) => Some(Route::for_chat(state, &chat).await),
        None => None,
    };
    chat_on_route(state, auth, chat, route).await
}

/// [`chat_with_messages`] for a chat whose route is already read. Its profile
/// and context are those of the model the chat asks its provider for.
async fn chat_on_route(
    state: &AppState,
    auth: &AuthUser,
    chat: chats::ChatRow,
    route: Option<Route>,
) -> ChatWithMessagesResponse {
    let choice = match &route {
        Some(route) => route.choice().map(|choice| choice.cloned()),
        None => Some(None),
    };
    let requested = match &route {
        Some(route) => route.apply(chat.clone()),
        None => chat.clone(),
    };
    let driver = route
        .as_ref()
        .and_then(|route| route.agent(state.config()))
        .map(|agent| Driver::new(agent, &requested.model_name));
    let endpoint = route.map(Route::into_endpoint);
    let host = &state.config().ollama_host;
    let profile = match &endpoint {
        None => Model::profile(host, &requested.model_name).await,
        Some(Ok(endpoint)) => {
            Model::profile_on(endpoint.origin(), host, &requested.model_name).await
        }
        Some(Err(_)) => Model::unshown(&requested.model_name),
    };
    let messages = chats::list_messages(state.db(), chat.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(MessageResponse::from)
        .collect();

    let context = match (endpoint, auth.0.user_id()) {
        (Some(Ok(endpoint)), Ok(actor)) => {
            // No draft or inference; reconstruct a fresh observation for reconnect.
            session::build(
                state,
                &requested,
                actor,
                None,
                session::Mode::Preview,
                endpoint,
                driver,
            )
            .await
            .ok()
            .map(|prepared| {
                // The exposed set, not every registered tool: this is what
                // a turn would actually send, which is the number a reader
                // is asking for when they ask what a chat costs.
                let exposed = prepared.agentic.then(|| prepared.tools.definitions());
                prepared.context.usage(&prepared.model, exposed.as_deref())
            })
        }
        _ => None,
    };
    let link = chats::link(state.db(), chat.id).await.ok().flatten();
    ChatWithMessagesResponse {
        context,
        chat: ChatResponse::from(chat)
            .with_profile(profile)
            .with_link(link)
            .with_choice(choice.as_ref().map(Option::as_ref)),
        messages,
    }
}

async fn apply_character(
    state: &AppState,
    chat_id: Uuid,
    character: Option<&ChatCharacter>,
    clear: bool,
) -> Result<Option<chats::ChatRow>, (StatusCode, Json<ErrorResponse>)> {
    if !clear && character.is_none() {
        return Ok(None);
    }
    if let Some(card) = character
        && let Err(message) = card.validate()
    {
        return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse::new(message))));
    }
    let stored = if clear { None } else { character };
    let chat = chats::set_chat_character(state.db(), chat_id, stored)
        .await
        .map_err(|error| {
            tracing::error!("Database error saving character: {}", error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
        })?;
    if let Some(card) = stored
        && let Some(greeting) = card
            .first_mes
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    {
        let empty = chats::list_messages(state.db(), chat_id)
            .await
            .map(|messages| messages.is_empty())
            .unwrap_or(true);
        if empty {
            let _ = chats::create_message(state.db(), chat_id, "assistant", greeting, None).await;
        }
    }
    Ok(chat)
}

/// One page of a workspace's chats. `total` counts every chat the filter
/// matches; `limit` is null when the page holds all of them from `offset` on.
#[derive(Debug, Serialize)]
pub struct ChatsListResponse {
    chats: Vec<ChatResponse>,
    total: i64,
    limit: Option<i64>,
    offset: i64,
}

/// What each chat in the workspace is doing, for the list's indicators.
#[derive(Debug, Serialize)]
pub struct ChatIndicatorsResponse {
    chats: Vec<Indicator>,
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

/// Query parameters for listing chats. Without `limit` the list holds every
/// chat from `offset` on.
#[derive(Debug, Deserialize)]
pub struct ListChatsQuery {
    workspace_id: Uuid,
    archived: Option<bool>,
    #[serde(default)]
    sort: chats::Sort,
    limit: Option<i64>,
    #[serde(default)]
    offset: i64,
}

fn parse_page(limit: Option<i64>, offset: i64) -> Result<chats::Page, String> {
    if limit.is_some_and(|limit| !(1..=MAX_LIST_LIMIT).contains(&limit)) {
        return Err(format!("Limit must be between 1 and {MAX_LIST_LIMIT}"));
    }
    if offset < 0 {
        return Err("Offset must not be negative".to_string());
    }
    Ok(chats::Page { limit, offset })
}

/// Create chat request
#[derive(Debug, Deserialize)]
pub struct CreateChatRequest {
    #[serde(default)]
    automatic_title: bool,
    workspace_id: Uuid,
    title: String,
    model_name: String,
    #[serde(default)]
    agent_enabled: bool,
    #[serde(default = "sandboxed_by_default")]
    agent_sandboxed: bool,
    #[serde(default)]
    auto_approve: bool,
    #[serde(default)]
    offline: bool,
    #[serde(default)]
    context_tokens: Option<i64>,
    #[serde(default)]
    reasoning_effort: Option<abnegate_llm::ReasoningEffort>,
    #[serde(default)]
    character: Option<ChatCharacter>,
    #[serde(default)]
    project_id: Option<Uuid>,
    /// The provider the chat runs on in place of its workspace's.
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    provider_model: Option<String>,
}

const fn sandboxed_by_default() -> bool {
    true
}

fn parse_context_tokens(value: Option<i64>) -> Result<Option<i64>, &'static str> {
    match value {
        None => Ok(None),
        Some(tokens) if tokens > 0 => Ok(Some(tokens)),
        Some(_) => Err("Context size must be a positive token count"),
    }
}

/// Update chat request
#[derive(Debug, Deserialize)]
pub struct UpdateChatRequest {
    title: Option<String>,
    model_name: Option<String>,
    agent_enabled: Option<bool>,
    agent_sandboxed: Option<bool>,
    auto_approve: Option<bool>,
    reasoning_effort: Option<abnegate_llm::ReasoningEffort>,
    context_tokens: Option<i64>,
    #[serde(default)]
    character: Option<ChatCharacter>,
    #[serde(default)]
    clear_character: bool,
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
) -> Result<Json<ChatsListResponse>, ServerError> {
    check_workspace_read_access(&state, &auth, query.workspace_id).await?;
    let page = parse_page(query.limit, query.offset).map_err(ServerError::BadRequest)?;

    let (items, total) = tokio::try_join!(
        chats::list_chats(
            state.db(),
            query.workspace_id,
            query.archived,
            query.sort,
            page
        ),
        chats::count_chats(state.db(), query.workspace_id, query.archived),
    )?;
    let ids: Vec<_> = items.iter().map(|chat| chat.id).collect();
    let choices = match chats::provider::of(state.db(), &ids).await {
        Ok(choices) => Some(choices),
        Err(error) => {
            tracing::warn!(%error, "Could not read the listed chats' providers");
            None
        }
    };
    let links = chats::links_for(state.db(), &ids).await?;

    Ok(Json(ChatsListResponse {
        chats: items
            .into_iter()
            .map(|chat| {
                let id = chat.id;
                let choice = choices.as_ref().map(|choices| choices.get(&id));
                ChatResponse::from(chat)
                    .with_link(links.get(&id).cloned())
                    .with_choice(choice)
            })
            .collect(),
        total,
        limit: page.limit,
        offset: page.offset,
    }))
}

/// GET /api/chats/indicators
///
/// Separate from the list itself because it is read on a timer and can involve
/// GitHub, while a list read has to stay quick and local.
pub async fn indicators(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ListChatsQuery>,
) -> Result<Json<ChatIndicatorsResponse>, ServerError> {
    check_workspace_read_access(&state, &auth, query.workspace_id).await?;
    let page = parse_page(query.limit, query.offset).map_err(ServerError::BadRequest)?;
    Ok(Json(ChatIndicatorsResponse {
        chats: indicator::list(&state, query.workspace_id, query.archived, query.sort, page)
            .await?,
    }))
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

    let choice = match provider::chosen(
        &state,
        req.workspace_id,
        req.provider.as_deref(),
        req.provider_model.as_deref(),
    )
    .await
    {
        Ok(choice) => choice,
        Err(refusal) => return provider::refused(refusal),
    };
    let route = Route::for_workspace(&state, req.workspace_id).await;
    if let Some(refusal) =
        workspace_refusal(&state, &route, &req.model_name, choice.is_some()).await
    {
        return refusal;
    }
    let route = choice.is_none().then_some(route);

    if let Some(project_id) = req.project_id {
        match projects::get_project_in_workspace(state.db(), project_id, req.workspace_id).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new("Project not found in this workspace")),
                )
                    .into_response();
            }
            Err(e) => {
                tracing::error!("Database error: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        }
    }

    let context_tokens = match parse_context_tokens(req.context_tokens) {
        Ok(tokens) => tokens,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(message))).into_response();
        }
    };

    match chats::create_chat_with_title(
        state.db(),
        Some(req.workspace_id),
        &req.title,
        &req.model_name,
        (req.agent_enabled, req.agent_sandboxed),
        req.automatic_title,
        req.auto_approve,
        req.reasoning_effort.unwrap_or_default(),
        req.project_id,
        req.offline,
        context_tokens,
        choice.as_ref(),
    )
    .await
    {
        Ok(chat) => {
            let chat = match apply_character(&state, chat.id, req.character.as_ref(), false).await {
                Ok(Some(chat)) => chat,
                Ok(None) => chat,
                Err(response) => return response.into_response(),
            };
            let chat = match route {
                Some(route) => chat_on_route(&state, &auth, chat, Some(route)).await,
                None => chat_with_messages(&state, &auth, chat).await,
            };
            (StatusCode::CREATED, Json(SingleChatResponse { chat })).into_response()
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

/// GET /api/chats/:id
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    // Get chat and verify access
    match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => Json(SingleChatResponse {
            chat: chat_with_messages(&state, &auth, chat).await,
        })
        .into_response(),
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

    let title = req.title.as_deref().map(str::trim);
    if title.is_some_and(str::is_empty) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Title cannot be blank")),
        )
            .into_response();
    }
    let model = req.model_name.as_deref().map(str::trim);
    if model.is_some_and(str::is_empty) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new("Model cannot be blank")),
        )
            .into_response();
    }
    let context_tokens = match parse_context_tokens(req.context_tokens) {
        Ok(tokens) => tokens,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(message))).into_response();
        }
    };
    if let Some(model) = model
        && let Some(refusal) = model_refusal(&state, &chat, model).await
    {
        return refusal;
    }
    match chats::update_chat(
        state.db(),
        id,
        title,
        req.agent_enabled,
        req.agent_sandboxed,
        req.auto_approve,
        req.reasoning_effort,
        context_tokens,
        model,
    )
    .await
    {
        Ok(Some(chat)) => {
            if let Some(auto_approve) = req.auto_approve {
                crate::agent::ApprovalPolicy::set_chat_auto(id, auto_approve);
            }
            let chat = if req.clear_character {
                match apply_character(&state, chat.id, None, true).await {
                    Ok(Some(chat)) => chat,
                    Ok(None) => chat,
                    Err(response) => return response.into_response(),
                }
            } else {
                match apply_character(&state, chat.id, req.character.as_ref(), false).await {
                    Ok(Some(chat)) => chat,
                    Ok(None) => chat,
                    Err(response) => return response.into_response(),
                }
            };
            Json(SingleChatResponse {
                chat: chat_with_messages(&state, &auth, chat).await,
            })
            .into_response()
        }
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

/// Why `chat` can't keep `model` as the model it asks its workspace's
/// provider for, as [`create`] refuses it.
async fn model_refusal(
    state: &AppState,
    chat: &chats::ChatRow,
    model: &str,
) -> Option<axum::response::Response> {
    let Some(workspace) = chat.workspace_id else {
        let profile = Model::profile(&state.config().ollama_host, model).await;
        return (profile.completion == Some(false)).then(unsupported);
    };
    let chosen = match chats::provider::get(state.db(), chat.id).await {
        Ok(choice) => choice.is_some(),
        Err(error) => {
            tracing::error!(chat = %chat.id, %error, "Could not read the chat's provider");
            return Some(
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response(),
            );
        }
    };
    let route = Route::for_workspace(state, workspace).await;
    workspace_refusal(state, &route, model, chosen).await
}

/// Why `model` is refused as the model a chat keeps for its workspace's
/// `route`: the route knows it can't chat, or can't be used at all. A name the
/// route doesn't know is allowed. A `chosen` chat runs on its own provider
/// until it returns to the workspace's, so an unusable route refuses it nothing.
async fn workspace_refusal(
    state: &AppState,
    route: &Route,
    model: &str,
    chosen: bool,
) -> Option<axum::response::Response> {
    let endpoint = match route.endpoint() {
        Ok(endpoint) => endpoint,
        Err(_) if chosen => return None,
        Err(unusable) => return Some(ServerError::Conflict(unusable.to_string()).into_response()),
    };
    let profile = Model::profile_on(endpoint.origin(), &state.config().ollama_host, model).await;
    (profile.completion == Some(false)).then(unsupported)
}

fn unsupported() -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse::new(crate::services::model::UNSUPPORTED)),
    )
        .into_response()
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
        Ok(true) => {
            if let Some(workspace_id) = chat.workspace_id {
                ArtifactStore::new(state.config().comfyui.artifact_root.clone())
                    .cleanup_chat(workspace_id, id)
                    .await;
            }
            StatusCode::NO_CONTENT.into_response()
        }
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
        Ok(Some(chat)) => Json(SingleChatResponse {
            chat: chat_with_messages(&state, &auth, chat).await,
        })
        .into_response(),
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
        Ok(Some(chat)) => Json(SingleChatResponse {
            chat: chat_with_messages(&state, &auth, chat).await,
        })
        .into_response(),
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

    if let Some(key) = authorship::claimed(req.metadata.as_ref()) {
        return ServerError::BadRequest(authorship::refusal(key)).into_response();
    }

    let mut session = match session::Session::acquire(
        &state,
        id,
        chat.workspace_id.expect("authorized workspace"),
        Uuid::new_v4(),
    )
    .await
    {
        Ok(session) => session,
        Err(error) => return ServerError::Conflict(error.to_string()).into_response(),
    };
    let result = session
        .store
        .create_message(&session.lease, &req.role, &req.content, req.metadata)
        .await;
    if let Err(error) = session.close().await {
        return ServerError::Conflict(error.to_string()).into_response();
    }
    match result {
        Ok(msg) => {
            crate::workers::titles::spawn(state.clone(), &msg);
            // Spawn background task to generate and store embedding
            // This is non-blocking and allows the message to be returned immediately
            spawn_message_embedding_task(state.clone(), msg.id, msg.chat_id, msg.content.clone());

            (
                StatusCode::CREATED,
                Json(SingleMessageResponse {
                    message: MessageResponse::from(msg),
                }),
            )
                .into_response()
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

    let mut session = match session::Session::acquire(
        &state,
        chat_id,
        chat.workspace_id.expect("authorized workspace"),
        Uuid::new_v4(),
    )
    .await
    {
        Ok(session) => session,
        Err(error) => return ServerError::Conflict(error.to_string()).into_response(),
    };
    let result = session
        .store
        .delete_message(&session.lease, message_id)
        .await;
    if let Ok(Some(subagents)) = &result {
        crate::ws::chat::announce_removed_subagents(chat_id, subagents.clone()).await;
    }
    if let Err(error) = session.close().await {
        return ServerError::Conflict(error.to_string()).into_response();
    }
    match result {
        Ok(Some(_)) => {
            if let Err(e) =
                message_embeddings::delete_message_embedding(state.db(), message_id).await
            {
                tracing::warn!(
                    "Failed to delete embedding for message {}: {}",
                    message_id,
                    e
                );
            }
            if let Some(workspace_id) = chat.workspace_id {
                // Generated artifacts are stored under their trusted database
                // message ID. Never derive an owner from user-controlled
                // attachment metadata.
                ArtifactStore::new(state.config().comfyui.artifact_root.clone())
                    .cleanup_owner(workspace_id, chat_id, message_id)
                    .await;
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(None) => (
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

/// How much of a matched message a search row carries as its snippet.
const SEARCH_SNIPPET_CHARS: usize = 200;

/// Message search result response
#[derive(Debug, Serialize)]
pub struct MessageSearchResponse {
    message_id: Uuid,
    chat_id: Uuid,
    chat_title: String,
    similarity: f32,
    relevance_score: f32,
    role: String,
    content: String,
    snippet: String,
    #[serde(flatten)]
    timestamps: Timestamps,
}

impl MessageSearchResponse {
    fn from_hit(result: message_embeddings::MessageSearchResult, chat_title: String) -> Self {
        Self {
            message_id: result.message_id,
            chat_id: result.chat_id,
            chat_title,
            similarity: result.similarity,
            relevance_score: result.similarity.clamp(0.0, 1.0),
            snippet: snippet::excerpt(&result.content, SEARCH_SNIPPET_CHARS),
            role: result.role,
            content: result.content,
            timestamps: Timestamps::from_naive(Some(result.created_at), Some(result.created_at)),
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

    let mut semantic = Vec::new();
    match state.embedding_service() {
        Some(embedding_service) => {
            match embedding_service
                .embed(&abnegate_index::embed_query_text(
                    embedding_service.model(),
                    &params.query,
                ))
                .await
            {
                Ok(query_embedding) => {
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
                        Ok(results) => semantic = results,
                        Err(e) => {
                            tracing::error!("Database error during message search: {}", e);
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                Json(ErrorResponse::new("Internal server error")),
                            )
                                .into_response();
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to generate query embedding, falling back to keyword: {}",
                        e
                    );
                }
            }
        }
        None => {
            tracing::warn!("Embedding service not available; using keyword message search");
        }
    }

    let keyword = match message_embeddings::search_messages_keyword(
        state.db(),
        &params.query,
        params.workspace_id,
        params.chat_id,
        limit,
    )
    .await
    {
        Ok(results) => results,
        Err(e) => {
            tracing::error!("Database error during keyword message search: {}", e);
            if semantic.is_empty() {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
            Vec::new()
        }
    };

    let results = message_embeddings::fuse_message_hits(semantic, keyword, &params.query, limit);
    let mut chat_ids: Vec<Uuid> = results.iter().map(|result| result.chat_id).collect();
    chat_ids.sort_unstable();
    chat_ids.dedup();
    let titles: std::collections::HashMap<Uuid, String> =
        match chats::titles(state.db(), &chat_ids).await {
            Ok(titles) => titles.into_iter().collect(),
            Err(e) => {
                tracing::error!("Database error reading chat titles: {}", e);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new("Internal server error")),
                )
                    .into_response();
            }
        };
    let response: Vec<MessageSearchResponse> = results
        .into_iter()
        .map(|result| {
            let title = titles.get(&result.chat_id).cloned().unwrap_or_default();
            MessageSearchResponse::from_hit(result, title)
        })
        .collect();
    let total = response.len();

    Json(MessageSearchListResponse {
        results: response,
        total,
    })
    .into_response()
}

#[derive(Debug, Serialize)]
pub struct ChatSourcesResponse {
    sources: Vec<chat_attached_sources::Attached>,
}

#[derive(Debug, Deserialize)]
pub struct SetChatSourcesRequest {
    source_ids: Vec<Uuid>,
}

/// GET /api/chats/{id}/sources
///
/// The sources this chat's retrieval is confined to. Empty means the whole
/// workspace.
pub async fn list_sources(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    if let Err(e) = get_chat_with_access(&state, &auth, id).await {
        return e.into_response();
    }
    match chat_attached_sources::list(state.db(), id).await {
        Ok(sources) => Json(ChatSourcesResponse { sources }).into_response(),
        Err(e) => {
            tracing::error!("Database error listing attached sources: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// PUT /api/chats/{id}/sources
///
/// Replace the chat's attachment with `source_ids`, every one of which has to
/// be an active source of the chat's workspace.
pub async fn set_sources(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(request): Json<SetChatSourcesRequest>,
) -> impl IntoResponse {
    let chat = match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => chat,
        Err(e) => return e.into_response(),
    };
    let Some(workspace_id) = chat.workspace_id else {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new("Chat has no workspace association")),
        )
            .into_response();
    };
    if let Err(e) = check_workspace_write_access(&state, &auth, workspace_id).await {
        return e.into_response();
    }
    match chat_attached_sources::replace(state.db(), id, workspace_id, &request.source_ids).await {
        Ok(sources) => Json(ChatSourcesResponse { sources }).into_response(),
        Err(chat_attached_sources::ReplaceError::Foreign(foreign)) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(format!(
                "Not sources of this chat's workspace: {}",
                foreign
                    .iter()
                    .map(Uuid::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        )
            .into_response(),
        Err(chat_attached_sources::ReplaceError::Database(e)) => {
            tracing::error!("Database error attaching sources: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new("Internal server error")),
            )
                .into_response()
        }
    }
}

/// Read-only draft estimation. Shares preparation with send; never saves or summarizes.
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

pub async fn context(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(request): Json<SendMessageRequest>,
) -> impl IntoResponse {
    let chat = match get_chat_with_access(&state, &auth, id).await {
        Ok(chat) => chat,
        Err(error) => return error.into_response(),
    };
    let workspace = match chat.workspace_id {
        Some(workspace) => workspace,
        None => {
            return ServerError::Forbidden("Chat has no workspace association".into())
                .into_response();
        }
    };
    let actor = match check_workspace_write_access(&state, &auth, workspace).await {
        Ok(actor) => actor,
        Err(error) => return error.into_response(),
    };
    let route = Route::for_chat(&state, &chat).await;
    let chat = route.apply(chat);
    let driver = route
        .agent(state.config())
        .map(|agent| Driver::new(agent, &chat.model_name));
    let endpoint = match route.into_endpoint() {
        Ok(endpoint) => endpoint,
        Err(unusable) => return ServerError::Conflict(unusable.to_string()).into_response(),
    };
    match session::build(
        &state,
        &chat,
        actor,
        Some((&request.content, request.metadata.as_ref())),
        session::Mode::Preview,
        endpoint,
        driver,
    )
    .await
    {
        Ok(prepared) => {
            let exposed = prepared.agentic.then(|| prepared.tools.definitions());
            Json(serde_json::json!({"context":prepared.context.usage(&prepared.model,exposed.as_deref())})).into_response()
        }
        Err(error) => ServerError::Conflict(error).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::db::chats::provider::Choice;
    use crate::services::chat::session::chat_row;

    fn serialised(choice: Option<Option<&Choice>>) -> Value {
        let response = ChatResponse::from(chat_row(None, false, false)).with_choice(choice);
        serde_json::to_value(response).expect("a chat serialises")
    }

    #[test]
    fn a_chat_carries_its_choice_and_leaves_out_one_it_could_not_read() {
        let choice = Choice {
            provider: Provider::Codex,
            model: Some("gpt-6-sol".to_string()),
        };

        let chosen = serialised(Some(Some(&choice)));
        let following = serialised(Some(None));
        let unread = serialised(None);

        assert_eq!(chosen["provider"], json!("codex"), "{chosen}");
        assert_eq!(chosen["provider_model"], json!("gpt-6-sol"), "{chosen}");
        assert!(
            following.get("provider") == Some(&Value::Null)
                && following.get("provider_model") == Some(&Value::Null),
            "a chat following its workspace says so: {following}"
        );
        assert!(
            unread.get("provider").is_none() && unread.get("provider_model").is_none(),
            "an unread choice is not reported as following the workspace: {unread}"
        );
    }

    #[test]
    fn a_page_takes_limits_from_one_to_a_hundred_or_none() {
        for limit in [None, Some(1), Some(MAX_LIST_LIMIT)] {
            assert_eq!(
                parse_page(limit, 7),
                Ok(chats::Page { limit, offset: 7 }),
                "limit {limit:?}"
            );
        }
        for limit in [0, -1, MAX_LIST_LIMIT + 1] {
            assert!(parse_page(Some(limit), 0).is_err(), "limit {limit}");
        }
    }

    #[test]
    fn a_page_refuses_a_negative_offset() {
        assert!(parse_page(None, -1).is_err());
        assert!(parse_page(Some(10), -1).is_err());
    }
}
