//! WebSocket handler for chat streaming
//!
//! Streams real-time AI responses for chat conversations.
//!
//! Protocol:
//! 1. Client connects to /ws/chats/:chat_id
//! 2. Client sends JWT token as first message for authentication
//! 3. Client sends "send" messages to trigger AI responses
//! 4. Server streams AI response chunks as they arrive
//! 5. Client can send "cancel" to interrupt generation
//! 6. Connection closes on error or client disconnect

use axum::{
    body::Bytes,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use dashmap::DashMap;
use futures::{SinkExt, Stream, StreamExt};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore, broadcast, mpsc};
use uuid::Uuid;
use zone_core::llm::{
    ChatStreamChunk, LlmClient, LlmConfig, LlmError, Message as LlmMessage, Role as LlmRole,
};

use crate::agent::{self, ActionReceipt, AgentEvent, AgentRun, Citation, ToolCallRecord};
use crate::auth::validate_token;
use crate::db::{ai_settings, chats, knowledge, workspace_members, workspaces};
use crate::services::searxng::{SearchContext, SearxngClient, sanitize_query};
use crate::state::AppState;
use crate::workers::embeddings::spawn_message_embedding_task;

/// WebSocket polling interval in milliseconds
const WS_POLL_INTERVAL_MS: u64 = 50;

/// Authentication timeout in seconds
const WS_AUTH_TIMEOUT_SECS: u64 = 30;

/// Re-check authorization every ~10 seconds (200 poll cycles at 50ms)
const AUTH_RECHECK_INTERVAL: u32 = 200;

/// WebSocket idle timeout in seconds (5 minutes)
const WS_IDLE_TIMEOUT_SECS: u64 = 300;

/// WebSocket ping interval in seconds
const WS_PING_INTERVAL_SECS: u64 = 30;

/// Maximum consecutive errors before closing connection
const MAX_CONSECUTIVE_ERRORS: u32 = 5;

/// Maximum concurrent connections per chat
const MAX_CONNECTIONS_PER_CHAT: usize = 5;

/// Rate limit: max messages per minute
const MAX_MESSAGES_PER_MINUTE: usize = 20;

/// Maximum message content length (100KB)
const MAX_MESSAGE_LENGTH: usize = 100_000;

/// Maximum context messages to include
const MAX_CONTEXT_MESSAGES: i64 = 50;

/// Maximum context search results
const MAX_CONTEXT_RESULTS: usize = 10;

/// Maximum context results to include in prompt
const MAX_CONTEXT_IN_PROMPT: usize = 5;

/// Maximum response length (100KB - same as message)
const MAX_RESPONSE_LENGTH: usize = 100_000;

/// Bound image output independently from text so a provider cannot make a
/// WebSocket frame or message metadata grow without limit.
const MAX_GENERATED_IMAGES: usize = 8;
const MAX_GENERATED_IMAGE_URL_LENGTH: usize = 16 * 1024 * 1024;

/// LLM stream timeout in seconds (5 minutes)
const LLM_STREAM_TIMEOUT_SECS: u64 = 300;

/// Status constants
const STATUS_CONNECTED: &str = "connected";

/// Global connection limiter per chat
static CHAT_CONNECTIONS: Lazy<DashMap<Uuid, Arc<Semaphore>>> = Lazy::new(DashMap::new);

/// Global cancellation broadcaster per (chat_id, message_id)
/// Using composite key prevents race conditions when multiple streams run concurrently
static CHAT_CANCELLATIONS: Lazy<DashMap<(Uuid, Uuid), broadcast::Sender<()>>> =
    Lazy::new(DashMap::new);
/// Serialize full request lifecycles per chat. The protocol's chunk/status
/// frames are intentionally compact and do not all carry correlation IDs.
static CHAT_GENERATIONS: Lazy<DashMap<Uuid, Arc<Semaphore>>> = Lazy::new(DashMap::new);
/// Keep direct image jobs globally bounded for the shared GPU runtime.
static IMAGE_GENERATIONS: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(1));
/// Approval waiters for the active generation on a chat.
static CHAT_APPROVALS: Lazy<DashMap<Uuid, crate::agent::ApprovalGate>> = Lazy::new(DashMap::new);

type SharedSender = Arc<Mutex<futures::stream::SplitSink<WebSocket, Message>>>;

async fn send_server(sender: &SharedSender, message: ServerMessage) -> bool {
    sender
        .lock()
        .await
        .send(message.to_ws_message())
        .await
        .is_ok()
}

/// A request owns its cancellation registration from the moment the socket
/// accepts it, including time spent waiting or preparing context.
struct Generation {
    chat_id: Uuid,
    message_id: Uuid,
    cancel: broadcast::Receiver<()>,
    approvals: crate::agent::ApprovalGate,
}

impl Generation {
    fn new(chat_id: Uuid) -> Self {
        let message_id = Uuid::new_v4();
        let (sender, cancel) = broadcast::channel(1);
        CHAT_CANCELLATIONS.insert((chat_id, message_id), sender);
        let approvals = crate::agent::ApprovalGate::new();
        CHAT_APPROVALS.insert(chat_id, approvals.clone());
        Self {
            chat_id,
            message_id,
            cancel,
            approvals,
        }
    }

    fn is_cancelled(&mut self) -> bool {
        !matches!(
            self.cancel.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        )
    }

    async fn cancelled(&self, sender: &SharedSender) {
        let _ = send_server(
            sender,
            ServerMessage::Cancelled {
                message_id: Some(self.message_id),
            },
        )
        .await;
    }
}

impl Drop for Generation {
    fn drop(&mut self) {
        CHAT_CANCELLATIONS.remove(&(self.chat_id, self.message_id));
        if let Some(entry) = CHAT_APPROVALS.get(&self.chat_id)
            && entry.value().same_as(&self.approvals)
        {
            drop(entry);
            CHAT_APPROVALS.remove(&self.chat_id);
        }
        self.approvals.deny_all();
    }
}

struct ChatPreparation {
    model: String,
    agentic: bool,
    auto_approve: bool,
    tools: agent::ChatTools,
    messages: Vec<LlmMessage>,
}

enum Routing {
    Image(crate::config::ComfyUiConfig),
    Video(crate::config::ComfyUiConfig),
    Chat(chats::ChatRow),
}

/// Client message types
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Authenticate with JWT
    Auth { token: String },
    /// Send a new user message
    Send {
        content: String,
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    },
    /// Cancel current generation
    Cancel,
    /// Confirm or reject a mutating file/shell tool call.
    ApproveTool {
        tool_call_id: String,
        approved: bool,
    },
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ChatImageAttachment {
    name: String,
    mime: String,
    url: String,
}

/// Server message types
#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// The first message's automatic title has been saved.
    TitleUpdated { chat_id: Uuid, title: String },
    /// Initial connection status
    Init { chat_id: Uuid, status: String },
    /// User message saved confirmation
    MessageSaved {
        message_id: Uuid,
        role: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },
    /// Assistant message started
    MessageStart { message_id: Uuid, role: String },
    /// Content chunk streamed
    Chunk { content: String, index: u32 },
    /// The agent started running a tool
    ToolCall {
        message_id: Uuid,
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    /// A mutating file or shell tool is waiting for the user to confirm.
    ToolApprovalRequired {
        message_id: Uuid,
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    /// A tool finished. `detail` is a short outcome for display, not the full
    /// output the model receives.
    ToolResult {
        message_id: Uuid,
        tool_call_id: String,
        name: String,
        success: bool,
        detail: String,
        duration_ms: u64,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        citations: Vec<Citation>,
    },
    /// A workspace write finished. Stored on the message so a reload still
    /// shows the action, target, actor, time, outcome, and item link.
    ActionReceipt {
        message_id: Uuid,
        receipt: ActionReceipt,
    },
    /// An image generated by the assistant.
    Image {
        message_id: Uuid,
        attachment: ChatImageAttachment,
    },
    /// A video generated by the assistant.
    Video {
        message_id: Uuid,
        attachment: ChatImageAttachment,
    },
    /// Assistant message completed
    MessageEnd {
        message_id: Uuid,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Generation cancelled
    Cancelled { message_id: Option<Uuid> },
    /// Error message
    Error { message: String },
    /// Non-fatal progress (e.g. web search in progress)
    Status { message: String },
}

fn saved_action(value: &serde_json::Value) -> Option<ServerMessage> {
    Some(ServerMessage::MessageSaved {
        message_id: serde_json::from_value(value.get("id")?.clone()).ok()?,
        role: value.get("role")?.as_str()?.to_string(),
        content: value.get("content")?.as_str()?.to_string(),
        metadata: value
            .get("metadata")
            .filter(|value| !value.is_null())
            .cloned(),
    })
}

impl ServerMessage {
    /// Convert to WebSocket message with fallback for serialization errors
    fn to_ws_message(&self) -> Message {
        Message::Text(
            serde_json::to_string(self)
                .unwrap_or_else(|e| {
                    tracing::error!("Failed to serialize ServerMessage: {}", e);
                    r#"{"type":"error","message":"Internal serialization error"}"#.to_string()
                })
                .into(),
        )
    }
}

/// WebSocket upgrade handler for chat
pub async fn handle_chat_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(chat_id): Path<Uuid>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, chat_id))
}

/// Guard that cleans up connection resources when dropped
struct ConnectionCleanupGuard {
    chat_id: Uuid,
    semaphore: Arc<Semaphore>,
}

impl Drop for ConnectionCleanupGuard {
    fn drop(&mut self) {
        // Schedule async cleanup - check after permit is released
        let chat_id = self.chat_id;
        let semaphore = self.semaphore.clone();
        tokio::spawn(async move {
            // Small delay to ensure permit is released
            tokio::time::sleep(Duration::from_millis(50)).await;
            // Only remove if all permits are now available (no active connections)
            if semaphore.available_permits() == MAX_CONNECTIONS_PER_CHAT {
                CHAT_CONNECTIONS.remove(&chat_id);
                tracing::debug!("Cleaned up connection entry for chat {}", chat_id);
            }
        });
    }
}

/// Pull provider-safe image URLs out of a message's stored metadata.
///
/// Images ride in `metadata.attachments[]` rather than in `content`, so the
/// text of a message stays readable and the images survive a reload. Protected
/// relative artifact URLs require Zone authentication, which LiteLLM does not
/// receive, so they must never be forwarded on later turns.
fn image_urls_from_metadata(metadata: Option<&serde_json::Value>) -> Vec<String> {
    metadata
        .and_then(|m| m.get("attachments"))
        .and_then(|a| a.as_array())
        .map(|attachments| {
            attachments
                .iter()
                .filter(|a| {
                    a.get("mime")
                        .and_then(|m| m.as_str())
                        .is_some_and(|m| m.starts_with("image/"))
                })
                .filter_map(|a| a.get("url").and_then(|u| u.as_str()))
                .filter(|url| {
                    url.starts_with("data:")
                        || url.starts_with("https://")
                        || url.starts_with("http://")
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn generated_media_attachment(url: &str, mime: &str, index: usize) -> Option<ChatImageAttachment> {
    if url.len() > MAX_GENERATED_IMAGE_URL_LENGTH {
        return None;
    }

    let mime = if let Some(header) = url
        .strip_prefix("data:")
        .and_then(|data| data.split_once(','))
    {
        let mime = header.0.split(';').next().unwrap_or_default();
        if !mime.starts_with("image/") && !mime.starts_with("video/") {
            return None;
        }
        mime.to_string()
    } else if url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("/api/artifacts/")
    {
        if !mime.is_empty() {
            mime.to_string()
        } else if url.ends_with(".webm") {
            "video/webm".to_string()
        } else if url.ends_with(".mp4") {
            "video/mp4".to_string()
        } else if url.ends_with(".jpg") || url.ends_with(".jpeg") {
            "image/jpeg".to_string()
        } else if url.ends_with(".webp") {
            "image/webp".to_string()
        } else {
            "image/png".to_string()
        }
    } else {
        return None;
    };

    let (prefix, extension) = match mime.as_str() {
        "video/webm" => ("generated-video", "webm"),
        "video/mp4" => ("generated-video", "mp4"),
        "image/jpeg" => ("generated-image", "jpg"),
        "image/webp" => ("generated-image", "webp"),
        "image/gif" => ("generated-image", "gif"),
        "image/avif" => ("generated-image", "avif"),
        _ if mime.starts_with("video/") => ("generated-video", "webm"),
        _ => ("generated-image", "png"),
    };

    Some(ChatImageAttachment {
        name: format!("{prefix}-{}.{}", index + 1, extension),
        mime,
        url: url.to_string(),
    })
}

fn generated_image_attachment(url: &str, index: usize) -> Option<ChatImageAttachment> {
    generated_media_attachment(url, "", index)
}

fn image_metadata(attachments: &[ChatImageAttachment]) -> Option<serde_json::Value> {
    (!attachments.is_empty()).then(|| serde_json::json!({ "attachments": attachments }))
}

/// Fold the tool trace, citations, and write receipts into the image
/// metadata, since one turn can produce all of them and they share the
/// message's single metadata column.
fn merge_metadata(
    images: Option<serde_json::Value>,
    tool_calls: &[ToolCallRecord],
    citations: &[Citation],
    receipts: &[ActionReceipt],
) -> Option<serde_json::Value> {
    if tool_calls.is_empty() && citations.is_empty() && receipts.is_empty() {
        return images;
    }

    let mut object = match images {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    if !tool_calls.is_empty() {
        object.insert("tool_calls".to_string(), serde_json::json!(tool_calls));
    }
    if !citations.is_empty() {
        object.insert("citations".to_string(), serde_json::json!(citations));
    }
    if !receipts.is_empty() {
        object.insert("action_receipts".to_string(), serde_json::json!(receipts));
    }
    Some(serde_json::Value::Object(object))
}

/// Handle the WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState, chat_id: Uuid) {
    let (mut sender, mut receiver) = socket.split();

    // Rate limiting - enforce max connections per chat
    let semaphore = CHAT_CONNECTIONS
        .entry(chat_id)
        .or_insert_with(|| Arc::new(Semaphore::new(MAX_CONNECTIONS_PER_CHAT)))
        .clone();

    let _permit = match semaphore.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            tracing::warn!("Too many connections for chat {}, rejecting", chat_id);
            let error_msg = ServerMessage::Error {
                message: "Too many connections".to_string(),
            };
            let _ = sender.send(error_msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
    };

    // Create cleanup guard - will clean up CHAT_CONNECTIONS on drop
    let _cleanup_guard = ConnectionCleanupGuard {
        chat_id,
        semaphore: semaphore.clone(),
    };

    // Wait for auth message
    let claims = match tokio::time::timeout(
        Duration::from_secs(WS_AUTH_TIMEOUT_SECS),
        receiver.next(),
    )
    .await
    {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::Auth { token }) => {
                match validate_token(&token, state.config().jwt_secret()) {
                    Ok(claims) => claims,
                    Err(e) => {
                        tracing::warn!("Authentication failed for chat {}: {}", chat_id, e);
                        let error_msg = ServerMessage::Error {
                            message: "Authentication failed".to_string(),
                        };
                        let _ = sender.send(error_msg.to_ws_message()).await;
                        let _ = sender.close().await;
                        return;
                    }
                }
            }
            _ => {
                let error_msg = ServerMessage::Error {
                    message: "Invalid message format".to_string(),
                };
                let _ = sender.send(error_msg.to_ws_message()).await;
                let _ = sender.close().await;
                return;
            }
        },
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => return,
        _ => {
            let error_msg = ServerMessage::Error {
                message: "Authentication timeout or error".to_string(),
            };
            let _ = sender.send(error_msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
    };

    // Get user ID from claims
    let user_id = match claims.user_id() {
        Ok(id) => id,
        Err(e) => {
            tracing::error!("Invalid user ID in JWT: {}", e);
            let error_msg = ServerMessage::Error {
                message: "Invalid user ID".to_string(),
            };
            let _ = sender.send(error_msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
    };

    // Verify chat exists and get workspace
    let chat = match chats::get_chat(state.db(), chat_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            let error_msg = ServerMessage::Error {
                message: "Chat not found".to_string(),
            };
            let _ = sender.send(error_msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
        Err(e) => {
            tracing::error!("Database error fetching chat: {}", e);
            let error_msg = ServerMessage::Error {
                message: "Internal server error".to_string(),
            };
            let _ = sender.send(error_msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
    };

    // Extract workspace_id and verify user has access
    let workspace_id = match chat.workspace_id {
        Some(ws_id) => ws_id,
        None => {
            tracing::warn!("Chat {} has no workspace_id", chat_id);
            let error_msg = ServerMessage::Error {
                message: "Invalid chat configuration".to_string(),
            };
            let _ = sender.send(error_msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
    };

    // Verify user has write access to the workspace
    match workspace_members::can_write(state.db(), workspace_id, user_id).await {
        Ok(true) => {
            tracing::info!(
                "User {} connected to chat {} in workspace {}",
                user_id,
                chat_id,
                workspace_id
            );
        }
        Ok(false) => {
            tracing::warn!(
                "User {} attempted to access chat {} without permission",
                user_id,
                chat_id
            );
            let error_msg = ServerMessage::Error {
                message: "Access denied".to_string(),
            };
            let _ = sender.send(error_msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
        Err(e) => {
            tracing::error!("Database error checking workspace access: {}", e);
            let error_msg = ServerMessage::Error {
                message: "Internal server error".to_string(),
            };
            let _ = sender.send(error_msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
    }

    // Send initial status
    let init_msg = ServerMessage::Init {
        chat_id,
        status: STATUS_CONNECTED.to_string(),
    };

    if sender.send(init_msg.to_ws_message()).await.is_err() {
        return;
    }
    let sender = Arc::new(Mutex::new(sender));
    let mut titles = crate::workers::titles::subscribe();
    let mut actions = crate::db::actions::subscribe();

    // Setup state for message loop
    let mut auth_check_counter = 0;
    let mut consecutive_errors = 0;
    let mut last_client_activity = Instant::now();
    let mut ping_interval = tokio::time::interval(Duration::from_secs(WS_PING_INTERVAL_SECS));
    let mut message_count = 0;
    let mut rate_limit_window_start = Instant::now();

    // Main message loop
    loop {
        tokio::select! {
            update = actions.recv() => {
                if let Ok((destination, message)) = update
                    && destination == chat_id
                {
                    if !workspace_members::is_member(state.db(), user_id, workspace_id).await.unwrap_or(false) {
                        let _ = sender.lock().await.close().await;
                        return;
                    }
                    if let Some(message) = saved_action(&message)
                        && !send_server(&sender, message).await
                    {
                        break;
                    }
                }
            }
            update = titles.recv() => {
                if let Ok((updated_chat_id, title)) = update
                    && updated_chat_id == chat_id
                    && !send_server(&sender, ServerMessage::TitleUpdated { chat_id, title }).await
                {
                    break;
                }
            }
            // Handle client messages
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        last_client_activity = Instant::now();

                        // Parse client message
                        match serde_json::from_str::<ClientMessage>(&text) {
                            Ok(ClientMessage::Send { content, metadata }) => {
                                // Rate limiting check
                                if rate_limit_window_start.elapsed() > Duration::from_secs(60) {
                                    message_count = 0;
                                    rate_limit_window_start = Instant::now();
                                }

                                message_count += 1;
                                if message_count > MAX_MESSAGES_PER_MINUTE {
                                    let error_msg = ServerMessage::Error {
                                        message: "Rate limit exceeded".to_string(),
                                    };
                                    let _ = send_server(&sender, error_msg).await;
                                    continue;
                                }

                                // Validate message length
                                if content.len() > MAX_MESSAGE_LENGTH {
                                    let error_msg = ServerMessage::Error {
                                        message: "Message too long".to_string(),
                                    };
                                    let _ = send_server(&sender, error_msg).await;
                                    continue;
                                }

                                // Handle the send message
                                let task_state = state.clone();
                                let task_sender = sender.clone();
                                let task_content = content;
                                let generation = Generation::new(chat_id);
                                tokio::spawn(async move {
                                    handle_send_message(
                                        &task_state,
                                        &task_sender,
                                        chat_id,
                                        workspace_id,
                                        user_id,
                                        &task_content,
                                        metadata,
                                        generation,
                                    ).await;
                                });
                            }
                            Ok(ClientMessage::Cancel) => {
                                // Broadcast cancellation to all active streams for this chat
                                // Iterate and send to all matching (chat_id, *) keys
                                let keys_to_cancel: Vec<_> = CHAT_CANCELLATIONS
                                    .iter()
                                    .filter(|entry| entry.key().0 == chat_id)
                                    .map(|entry| *entry.key())
                                    .collect();

                                for key in keys_to_cancel {
                                    if let Some(tx) = CHAT_CANCELLATIONS.get(&key) {
                                        let _ = tx.send(());
                                    }
                                }
                                if let Some(gate) = CHAT_APPROVALS.get(&chat_id) {
                                    gate.deny_all();
                                }
                            }
                            Ok(ClientMessage::ApproveTool {
                                tool_call_id,
                                approved,
                            }) => {
                                let decided = CHAT_APPROVALS
                                    .get(&chat_id)
                                    .is_some_and(|gate| gate.decide(&tool_call_id, approved));
                                if !decided {
                                    let _ = send_server(
                                        &sender,
                                        ServerMessage::Error {
                                            message: "That tool call is not waiting for approval."
                                                .to_string(),
                                        },
                                    )
                                    .await;
                                }
                            }
                            Ok(ClientMessage::Auth { .. }) => {
                                // Ignore duplicate auth messages
                            }
                            Err(e) => {
                                tracing::warn!("Invalid client message: {}", e);
                                let error_msg = ServerMessage::Error {
                                    message: "Invalid message format".to_string(),
                                };
                                let _ = send_server(&sender, error_msg).await;
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if sender.lock().await.send(Message::Pong(data)).await.is_err() {
                            return;
                        }
                        last_client_activity = Instant::now();
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_client_activity = Instant::now();
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    _ => {}
                }
            }

            // Periodic ping and idle timeout check
            _ = ping_interval.tick() => {
                // Check for idle timeout
                if last_client_activity.elapsed() > Duration::from_secs(WS_IDLE_TIMEOUT_SECS) {
                    tracing::info!("Closing idle WebSocket connection for chat {}", chat_id);
                    let _ = sender.lock().await.close().await;
                    return;
                }

                // Periodic authorization re-check
                auth_check_counter += 1;
                if auth_check_counter >= AUTH_RECHECK_INTERVAL {
                    auth_check_counter = 0;
                    match workspace_members::can_write(state.db(), workspace_id, user_id).await {
                        Ok(false) => {
                            tracing::warn!(
                                "User {} lost access to workspace {} during chat {}",
                                user_id,
                                workspace_id,
                                chat_id
                            );
                            let error_msg = ServerMessage::Error {
                                message: "Access revoked".to_string(),
                            };
                            let _ = send_server(&sender, error_msg).await;
                            let _ = sender.lock().await.close().await;
                            return;
                        }
                        Err(e) => {
                            tracing::error!("Error re-checking workspace access: {}", e);
                            consecutive_errors += 1;
                            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                let error_msg = ServerMessage::Error {
                                    message: "Connection unstable, please reconnect".to_string(),
                                };
                                let _ = send_server(&sender, error_msg).await;
                                let _ = sender.lock().await.close().await;
                                return;
                            }
                        }
                        Ok(true) => {
                            consecutive_errors = 0;
                        }
                    }
                }

                // Send ping
                if sender.lock().await.send(Message::Ping(Bytes::new())).await.is_err() {
                    return;
                }
            }
        }
    }
}

async fn resolve_generation_source(
    state: &AppState,
    chat_id: Uuid,
    workspace_id: Uuid,
    prompt: &str,
    metadata: Option<&serde_json::Value>,
    store: &crate::services::artifacts::ArtifactStore,
) -> Result<
    Option<crate::services::comfyui::SourceImage>,
    crate::services::image_source::SourceImageError,
> {
    use crate::services::image_source::{
        has_image_attachment, resolve_source_image, resolve_source_image_from,
    };

    if has_image_attachment(metadata) {
        return resolve_source_image(metadata, workspace_id, chat_id, store).await;
    }
    if !crate::services::image_intent::should_reuse_thread_image(prompt) {
        return Ok(None);
    }
    let history = match chats::list_messages(state.db(), chat_id).await {
        Ok(messages) => messages,
        Err(error) => {
            tracing::warn!("Failed to load chat images for image-to-image: {error}");
            return Ok(None);
        }
    };
    resolve_source_image_from(
        history
            .iter()
            .rev()
            .map(|message| message.metadata.as_ref()),
        workspace_id,
        chat_id,
        store,
    )
    .await
}

async fn handle_image_generation(
    state: &AppState,
    sender: &SharedSender,
    chat_id: Uuid,
    workspace_id: Uuid,
    prompt: &str,
    metadata: Option<&serde_json::Value>,
    image_config: crate::config::ComfyUiConfig,
    generation: &mut Generation,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::services::{
        artifacts::ArtifactStore,
        comfyui::{ComfyUiClient, ComfyUiError},
    };

    const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
    let assistant_message_id = generation.message_id;

    let client = match ComfyUiClient::new(image_config.clone()) {
        Ok(client) => client,
        Err(error) => {
            let _ = send_server(
                sender,
                ServerMessage::Error {
                    message: format!("Image generation is not configured: {error}"),
                },
            )
            .await;
            return Ok(());
        }
    };
    let store = ArtifactStore::new(image_config.artifact_root.clone());
    let source =
        match resolve_generation_source(state, chat_id, workspace_id, prompt, metadata, &store)
            .await
        {
            Ok(source) => source,
            Err(error) => {
                let _ = send_server(
                    sender,
                    ServerMessage::Error {
                        message: format!("Image generation failed: {error}"),
                    },
                )
                .await;
                return Ok(());
            }
        };
    let _ = send_server(
        sender,
        ServerMessage::Status {
            message: if source.is_some() {
                "Preparing image-to-image...".to_string()
            } else {
                "Preparing image generation...".to_string()
            },
        },
    )
    .await;
    let generation_prompt = if source.is_some() {
        crate::services::image_intent::ImageIntentClassifier::new(
            image_config.clone(),
            state.config().litellm_host.clone(),
            state.config().litellm_key.clone(),
        )
        .edit_prompt(prompt)
        .await
    } else {
        prompt.to_string()
    };
    let _generation_permit = tokio::select! {
        biased;
        _ = generation.cancel.recv() => {
            let _ = send_server(
                sender,
                ServerMessage::Cancelled { message_id: Some(assistant_message_id) },
            ).await;
            return Ok(());
        }
        permit = IMAGE_GENERATIONS.acquire() => permit.expect("image semaphore is never closed"),
    };

    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    let progress_sender = sender.clone();
    let progress_task = tokio::spawn(async move {
        while let Some(message) = progress_rx.recv().await {
            if !send_server(&progress_sender, ServerMessage::Status { message }).await {
                break;
            }
        }
    });

    let result = client
        .generate(
            &generation_prompt,
            source.as_ref(),
            &mut generation.cancel,
            progress_tx,
        )
        .await;
    progress_task.abort();
    let _ = progress_task.await;
    if result.is_ok() && generation.cancel.try_recv().is_ok() {
        generation.cancelled(sender).await;
        return Ok(());
    }

    let images = match result {
        Ok(images) => images,
        Err(ComfyUiError::Cancelled) => {
            let _ = send_server(
                sender,
                ServerMessage::Cancelled {
                    message_id: Some(assistant_message_id),
                },
            )
            .await;
            return Ok(());
        }
        Err(error) => {
            let message = match &error {
                ComfyUiError::Http(error) if error.is_connect() =>
                    "Image generation failed: cannot reach ComfyUI. Start the image service and try again.".to_string(),
                ComfyUiError::Http(error) if error.is_timeout() =>
                    "Image generation failed: ComfyUI did not respond in time. Check the image service and try again.".to_string(),
                _ => format!("Image generation failed: {error}"),
            };
            let _ = send_server(sender, ServerMessage::Error { message }).await;
            return Ok(());
        }
    };

    let mut attachments = Vec::new();
    for image in images.into_iter().take(MAX_GENERATED_IMAGES) {
        if image.bytes.len() > MAX_ARTIFACT_BYTES {
            tracing::warn!("ComfyUI output exceeded artifact size limit");
            continue;
        }
        let extension = match image.mime.as_str() {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        };
        let url = match store
            .persist(
                workspace_id,
                chat_id,
                assistant_message_id,
                extension,
                &image.bytes,
            )
            .await
        {
            Ok(url) => url,
            Err(error) => {
                tracing::error!("Failed to persist generated image: {error}");
                store
                    .cleanup_owner(workspace_id, chat_id, assistant_message_id)
                    .await;
                let _ = send_server(
                    sender,
                    ServerMessage::Error {
                        message: "Image generation failed: could not store the image".to_string(),
                    },
                )
                .await;
                return Ok(());
            }
        };
        if let Some(attachment) = generated_image_attachment(&url, attachments.len()) {
            attachments.push(attachment);
        }
    }
    if attachments.is_empty() {
        let _ = send_server(
            sender,
            ServerMessage::Error {
                message: "Image generation completed without a usable image".to_string(),
            },
        )
        .await;
        return Ok(());
    }

    let content = "Generated image.";
    let metadata = image_metadata(&attachments);
    if let Err(error) = chats::create_message_with_id(
        state.db(),
        assistant_message_id,
        chat_id,
        "assistant",
        content,
        metadata.clone(),
    )
    .await
    {
        tracing::error!("Failed to persist generated image message: {error}");
        store
            .cleanup_owner(workspace_id, chat_id, assistant_message_id)
            .await;
        let _ = send_server(
            sender,
            ServerMessage::Error {
                message: "Image generation failed: could not save the message".to_string(),
            },
        )
        .await;
        return Ok(());
    }
    let _ = send_server(
        sender,
        ServerMessage::MessageStart {
            message_id: assistant_message_id,
            role: "assistant".to_string(),
        },
    )
    .await;
    for attachment in &attachments {
        let _ = send_server(
            sender,
            ServerMessage::Image {
                message_id: assistant_message_id,
                attachment: attachment.clone(),
            },
        )
        .await;
    }
    let _ = send_server(
        sender,
        ServerMessage::MessageEnd {
            message_id: assistant_message_id,
            content: content.to_string(),
            metadata,
            error: None,
        },
    )
    .await;
    Ok(())
}

async fn handle_video_generation(
    state: &AppState,
    sender: &SharedSender,
    chat_id: Uuid,
    workspace_id: Uuid,
    prompt: &str,
    metadata: Option<&serde_json::Value>,
    video_config: crate::config::ComfyUiConfig,
    generation: &mut Generation,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::services::{
        artifacts::ArtifactStore,
        comfyui::{ComfyUiClient, ComfyUiError},
    };

    const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
    let assistant_message_id = generation.message_id;

    let client = match ComfyUiClient::new(video_config.clone()) {
        Ok(client) => client,
        Err(error) => {
            let _ = send_server(
                sender,
                ServerMessage::Error {
                    message: format!("Video generation is not configured: {error}"),
                },
            )
            .await;
            return Ok(());
        }
    };
    let store = ArtifactStore::new(video_config.artifact_root.clone());
    let source =
        match resolve_generation_source(state, chat_id, workspace_id, prompt, metadata, &store)
            .await
        {
            Ok(source) => source,
            Err(error) => {
                let _ = send_server(
                    sender,
                    ServerMessage::Error {
                        message: format!("Video generation failed: {error}"),
                    },
                )
                .await;
                return Ok(());
            }
        };
    let _ = send_server(
        sender,
        ServerMessage::Status {
            message: if source.is_some() {
                "Preparing image-to-video...".to_string()
            } else {
                "Preparing video generation...".to_string()
            },
        },
    )
    .await;
    let _generation_permit = tokio::select! {
        biased;
        _ = generation.cancel.recv() => {
            let _ = send_server(
                sender,
                ServerMessage::Cancelled { message_id: Some(assistant_message_id) },
            ).await;
            return Ok(());
        }
        permit = IMAGE_GENERATIONS.acquire() => permit.expect("image semaphore is never closed"),
    };
    let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
    let progress_sender = sender.clone();
    let progress_task = tokio::spawn(async move {
        while let Some(message) = progress_rx.recv().await {
            if !send_server(&progress_sender, ServerMessage::Status { message }).await {
                break;
            }
        }
    });

    let result = client
        .generate_video(prompt, source.as_ref(), &mut generation.cancel, progress_tx)
        .await;
    progress_task.abort();
    let _ = progress_task.await;
    if result.is_ok() && generation.cancel.try_recv().is_ok() {
        generation.cancelled(sender).await;
        return Ok(());
    }

    let videos = match result {
        Ok(videos) => videos,
        Err(ComfyUiError::Cancelled) => {
            let _ = send_server(
                sender,
                ServerMessage::Cancelled {
                    message_id: Some(assistant_message_id),
                },
            )
            .await;
            return Ok(());
        }
        Err(error) => {
            let message = match &error {
                ComfyUiError::Http(error) if error.is_connect() =>
                    "Video generation failed: cannot reach ComfyUI. Start the image service and try again.".to_string(),
                ComfyUiError::Http(error) if error.is_timeout() =>
                    "Video generation failed: ComfyUI did not respond in time. Check the image service and try again.".to_string(),
                _ => format!("Video generation failed: {error}"),
            };
            let _ = send_server(sender, ServerMessage::Error { message }).await;
            return Ok(());
        }
    };

    let mut attachments = Vec::new();
    for video in videos.into_iter().take(MAX_GENERATED_IMAGES) {
        if video.bytes.len() > MAX_ARTIFACT_BYTES {
            tracing::warn!("ComfyUI video output exceeded artifact size limit");
            continue;
        }
        let extension = match video.mime.as_str() {
            "video/mp4" => "mp4",
            _ => "webm",
        };
        let url = match store
            .persist(
                workspace_id,
                chat_id,
                assistant_message_id,
                extension,
                &video.bytes,
            )
            .await
        {
            Ok(url) => url,
            Err(error) => {
                tracing::error!("Failed to persist generated video: {error}");
                store
                    .cleanup_owner(workspace_id, chat_id, assistant_message_id)
                    .await;
                let _ = send_server(
                    sender,
                    ServerMessage::Error {
                        message: "Video generation failed: could not store the video".to_string(),
                    },
                )
                .await;
                return Ok(());
            }
        };
        if let Some(attachment) = generated_media_attachment(&url, &video.mime, attachments.len()) {
            attachments.push(attachment);
        }
    }
    if attachments.is_empty() {
        let _ = send_server(
            sender,
            ServerMessage::Error {
                message: "Video generation completed without a usable video".to_string(),
            },
        )
        .await;
        return Ok(());
    }

    let content = "Generated video.";
    let metadata = image_metadata(&attachments);
    if let Err(error) = chats::create_message_with_id(
        state.db(),
        assistant_message_id,
        chat_id,
        "assistant",
        content,
        metadata.clone(),
    )
    .await
    {
        tracing::error!("Failed to persist generated video message: {error}");
        store
            .cleanup_owner(workspace_id, chat_id, assistant_message_id)
            .await;
        let _ = send_server(
            sender,
            ServerMessage::Error {
                message: "Video generation failed: could not save the message".to_string(),
            },
        )
        .await;
        return Ok(());
    }
    let _ = send_server(
        sender,
        ServerMessage::MessageStart {
            message_id: assistant_message_id,
            role: "assistant".to_string(),
        },
    )
    .await;
    for attachment in &attachments {
        let _ = send_server(
            sender,
            ServerMessage::Video {
                message_id: assistant_message_id,
                attachment: attachment.clone(),
            },
        )
        .await;
    }
    let _ = send_server(
        sender,
        ServerMessage::MessageEnd {
            message_id: assistant_message_id,
            content: content.to_string(),
            metadata,
            error: None,
        },
    )
    .await;
    Ok(())
}

/// Adapt a plain completion stream to the events the agent emits, so one loop
/// can consume either shape.
fn plain_events(
    stream: impl Stream<Item = Result<ChatStreamChunk, LlmError>>,
) -> impl Stream<Item = AgentEvent> {
    async_stream::stream! {
        futures::pin_mut!(stream);

        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(e) => {
                    // Keep whatever was generated: returning here discarded a
                    // complete reply whenever the provider sent one chunk the
                    // envelope could not parse.
                    tracing::error!("LLM stream error, keeping partial reply: {}", e);
                    yield AgentEvent::Failed("Stream error".to_string());
                    return;
                }
            };

            let Some(choice) = chunk.choices.first() else {
                continue;
            };

            if let Some(content) = &choice.delta.content
                && !content.is_empty()
            {
                yield AgentEvent::Chunk(content.clone());
            }

            for image in &choice.delta.generated_images {
                yield AgentEvent::Image(image.image_url.url.clone());
            }

            if choice.finish_reason.is_some() {
                return;
            }
        }
    }
}

/// Handle a send message request
///
/// Chat requires write access (Member role or higher) since it creates messages.
/// This is intentionally stricter than context.rs which only requires membership.
async fn handle_send_message(
    state: &AppState,
    sender: &SharedSender,
    chat_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    content: &str,
    metadata: Option<serde_json::Value>,
    mut request: Generation,
) {
    let generation = CHAT_GENERATIONS
        .entry(chat_id)
        .or_insert_with(|| Arc::new(Semaphore::new(1)))
        .clone();
    let _permit = tokio::select! {
        biased;
        _ = request.cancel.recv() => {
            request.cancelled(sender).await;
            return;
        }
        permit = generation.acquire() => permit.expect("chat semaphore is never closed"),
    };

    if !workspace_members::can_write(state.db(), workspace_id, user_id)
        .await
        .unwrap_or(false)
    {
        let _ = send_server(
            sender,
            ServerMessage::Error {
                message: "Workspace access denied".to_string(),
            },
        )
        .await;
        return;
    }
    let preparation = tokio::select! {
        biased;
        _ = request.cancel.recv() => {
            request.cancelled(sender).await;
            return;
        }
        result = prepare_message(state, sender, chat_id, workspace_id, content, metadata.as_ref()) => result,
    };
    let result = async {
        let Some(routing) = preparation? else {
            return Ok(());
        };
        if request.is_cancelled() {
            request.cancelled(sender).await;
            return Ok(());
        }
        let web_search_requested = state.config().web_search.requested_for(content, metadata.as_ref());

        // Once persistence begins, finish the commit and acknowledgement
        // before honouring Stop. Dropping an INSERT future cannot roll it back.
        save_message(state, sender, chat_id, content, metadata.clone()).await?;

        if request.is_cancelled() {
            request.cancelled(sender).await;
            return Ok(());
        }
        match routing {
            Routing::Image(config) => {
                handle_image_generation(
                    state,
                    sender,
                    chat_id,
                    workspace_id,
                    content,
                    metadata.as_ref(),
                    config,
                    &mut request,
                )
                .await
            }
            Routing::Video(config) => {
                handle_video_generation(
                    state,
                    sender,
                    chat_id,
                    workspace_id,
                    content,
                    metadata.as_ref(),
                    config,
                    &mut request,
                )
                .await
            }
            Routing::Chat(chat) => {
                let preparation = tokio::select! {
                    biased;
                    _ = request.cancel.recv() => {
                        request.cancelled(sender).await;
                        return Ok(());
                    }
                    result = prepare_chat(state, sender, chat_id, workspace_id, user_id, content, chat, web_search_requested) => result,
                };
                handle_chat_generation(state, sender, chat_id, preparation, &mut request).await
            }
        }
    }.await;
    if let Err(error) = result {
        tracing::error!("Error handling send message: {error}");
        let _ = send_server(
            sender,
            ServerMessage::Error {
                message: "Failed to process message".to_string(),
            },
        )
        .await;
    }
}

async fn save_message(
    state: &AppState,
    sender: &SharedSender,
    chat_id: Uuid,
    content: &str,
    metadata: Option<serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Save user message to database
    let user_message =
        chats::create_message(state.db(), chat_id, "user", content, metadata).await?;
    crate::workers::titles::spawn(state.clone(), &user_message);

    // Confirm message saved
    let saved_msg = ServerMessage::MessageSaved {
        message_id: user_message.id,
        role: "user".to_string(),
        content: content.to_string(),
        metadata: user_message.metadata.clone(),
    };
    if !send_server(sender, saved_msg).await {
        tracing::debug!("Client disconnected after saving user message");
    }

    // Spawn background task to generate user message embedding
    spawn_message_embedding_task(state.clone(), user_message.id, chat_id, content.to_string());
    Ok(())
}

async fn prepare_message(
    state: &AppState,
    sender: &SharedSender,
    chat_id: Uuid,
    workspace_id: Uuid,
    content: &str,
    metadata: Option<&serde_json::Value>,
) -> Result<Option<Routing>, Box<dyn std::error::Error + Send + Sync>> {
    // Read the chat fresh rather than trusting the row captured at connect
    // time, so switching model or toggling agent mode takes effect on the next
    // message instead of the next reconnect.
    let chat = match chats::get_chat(state.db(), chat_id).await? {
        Some(chat) => chat,
        None => {
            let error_msg = ServerMessage::Error {
                message: "Chat not found".to_string(),
            };
            let _ = send_server(sender, error_msg).await;
            return Ok(None);
        }
    };
    if chat.workspace_id != Some(workspace_id) {
        return Err("Chat does not belong to the authenticated workspace".into());
    }
    let mut image_config = state.config().comfyui.clone();
    if let Ok(Some(workspace)) = workspaces::get_workspace(state.db(), workspace_id).await
        && let Ok(settings) = ai_settings::get_effective_ai_settings(
            state.db(),
            workspace.organization_id,
            workspace_id,
        )
        .await
    {
        settings.apply_to_comfyui(&mut image_config);
    }
    let classifier = crate::services::image_intent::ImageIntentClassifier::new(
        image_config.clone(),
        state.config().litellm_host.clone(),
        state.config().litellm_key.clone(),
    );
    let intent = classifier.classify(content, metadata).await;

    if intent == crate::services::image_intent::GenerationIntent::Chat
        && crate::services::model::Model::completion(&state.config().ollama_host, &chat.model_name)
            .await
            == Some(false)
    {
        let _ = send_server(
            sender,
            ServerMessage::Error {
                message: crate::services::model::UNSUPPORTED.to_string(),
            },
        )
        .await;
        return Ok(None);
    }

    Ok(Some(if chat.agent_enabled {
        Routing::Chat(chat)
    } else {
        match intent {
            crate::services::image_intent::GenerationIntent::Video => Routing::Video(image_config),
            crate::services::image_intent::GenerationIntent::Image => Routing::Image(image_config),
            crate::services::image_intent::GenerationIntent::Chat => Routing::Chat(chat),
        }
    }))
}

async fn prepare_chat(
    state: &AppState,
    sender: &SharedSender,
    chat_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    content: &str,
    chat: chats::ChatRow,
    web_search_requested: bool,
) -> ChatPreparation {
    let model_name = chat.model_name.as_str();
    // Build context for AI
    let mut context_messages = Vec::new();

    // Add recent conversation history
    match chats::list_messages(state.db(), chat_id).await {
        Ok(messages) => {
            // Take last N messages for context
            let recent_messages: Vec<_> = messages
                .iter()
                .rev()
                .take(MAX_CONTEXT_MESSAGES as usize)
                .rev()
                .collect();

            for msg in recent_messages {
                context_messages.push(LlmMessage {
                    role: match msg.role.as_str() {
                        "user" => LlmRole::User,
                        "assistant" => LlmRole::Assistant,
                        "system" => LlmRole::System,
                        _ => LlmRole::User,
                    },
                    content: Some(msg.content.clone()),
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                    images: image_urls_from_metadata(msg.metadata.as_ref()),
                    generated_images: Vec::new(),
                });
            }
        }
        Err(e) => {
            tracing::warn!("Failed to fetch message history: {}", e);
            // Continue without history
        }
    }

    // Agent mode replaces blind context injection with a `search_knowledge`
    // tool. Doing both would spend the context window on passages the model
    // never asked for and then offer to fetch them again.
    let tools = agent::ChatTools::build(agent::WorkspaceScope {
        state: state.clone(),
        workspace_id,
        chat_id,
        user_id,
    })
    .await;
    let agentic = chat.agent_enabled && !tools.is_empty();

    let mut prompt = if agentic {
        agent::system_prompt(&tools, chat.auto_approve)
    } else {
        "You are Zone's assistant, answering inside one of the user's workspaces.".to_string()
    };
    if !agentic {
        let mut context_lines: Vec<String> = Vec::new();

        if let Some(embedding_service) = state.embedding_service() {
            match embedding_service.embed(content).await {
                Ok(query_embedding) => {
                    match knowledge::search_knowledge_entries(
                        state.db(),
                        &query_embedding,
                        workspace_id,
                        MAX_CONTEXT_IN_PROMPT as i64,
                        0.5,
                    )
                    .await
                    {
                        Ok(hits) => {
                            for hit in hits {
                                let note =
                                    hit.content.split_whitespace().collect::<Vec<_>>().join(" ");
                                let note = if note.chars().count() > 500 {
                                    format!("{}…", note.chars().take(500).collect::<String>())
                                } else {
                                    note
                                };
                                context_lines
                                    .push(format!("- [knowledge] {}: {}", hit.title, note));
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Knowledge search failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Knowledge query embed failed: {}", e);
                }
            }
        }

        if let Some(context_service) = state.context_service() {
            let filters = zone_context::embeddings::SearchFilters {
                workspace_id: Some(workspace_id),
                source_ids: None,
                categories: None,
                min_quality: None,
                since: None,
            };
            match context_service
                .search(content, MAX_CONTEXT_RESULTS, Some(filters))
                .await
            {
                Ok(results) => {
                    for result in results.iter().take(MAX_CONTEXT_IN_PROMPT) {
                        context_lines.push(format!("- {}", result.chunk_text));
                    }
                }
                Err(e) => {
                    tracing::warn!("Context search failed: {}", e);
                }
            }
        }

        if !context_lines.is_empty() {
            prompt.push_str("\n\nRelevant context:\n\n");
            prompt.push_str(&context_lines.join("\n"));
            prompt.push('\n');
        }
    }

    // Inject live web search via SearXNG (reached through Gluetun in Docker).
    let mut search = SearchContext::new(&state.config().web_search);
    if web_search_requested {
        let query = sanitize_query(content);
        if !query.is_empty() {
            let status_msg = ServerMessage::Status {
                message: "Searching the web...".to_string(),
            };
            let _ = send_server(sender, status_msg).await;

            match SearxngClient::new(state.config().web_search.clone()) {
                Ok(client) => match client.search(&query).await {
                    Ok(hits) if !hits.is_empty() => {
                        tracing::debug!(
                            "Injected {} web search results for chat {}",
                            hits.len(),
                            chat_id
                        );
                        search = SearchContext::Results(hits);
                    }
                    Ok(_) => {
                        tracing::debug!("Web search returned no results for chat {}", chat_id);
                        search = SearchContext::Empty;
                    }
                    Err(e) => {
                        tracing::warn!("Web search failed for chat {}: {}", chat_id, e);
                        search = SearchContext::Failed;
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to create web search client: {}", e);
                    search = SearchContext::Failed;
                }
            }
        }
    }
    prompt.push_str("\n\n");
    prompt.push_str(&search.capability());
    context_messages.insert(0, LlmMessage::system(prompt));
    // Ollama's Qwen3.8 renderer hoists system messages ahead of history. A
    // supplemental user-role context message keeps current evidence nearby.
    // This message is sent only to the model, never stored as a user message.
    context_messages.push(LlmMessage::user(search.prompt()));

    ChatPreparation {
        model: model_name.to_string(),
        agentic,
        auto_approve: chat.auto_approve,
        tools,
        messages: context_messages,
    }
}

async fn handle_chat_generation(
    state: &AppState,
    sender: &SharedSender,
    chat_id: Uuid,
    preparation: ChatPreparation,
    generation: &mut Generation,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ChatPreparation {
        model,
        agentic,
        auto_approve,
        tools,
        messages: context_messages,
    } = preparation;
    let model_name = model.as_str();
    // Create LLM client
    let llm_config = LlmConfig {
        base_url: state.config().litellm_host.clone(),
        api_key: state.config().litellm_key.clone(),
        default_model: model_name.to_string(),
        temperature: 0.7,
        max_tokens: 4096,
    };
    let llm_client = LlmClient::new(llm_config);

    let assistant_message_id = generation.message_id;
    if generation.cancel.try_recv().is_ok() {
        generation.cancelled(sender).await;
        return Ok(());
    }

    // Send message start with the ID we'll use throughout
    let start_msg = ServerMessage::MessageStart {
        message_id: assistant_message_id,
        role: "assistant".to_string(),
    };
    let _ = send_server(sender, start_msg).await;

    // Both modes produce the same event stream, so the loop below - and its
    // cancellation, timeout and truncation handling - is shared.
    let mut events: Pin<Box<dyn Stream<Item = AgentEvent> + Send>> = if agentic {
        Box::pin(agent::run(AgentRun {
            llm: llm_client,
            model: model_name.to_string(),
            tools,
            messages: context_messages,
            budget: agent::LoopBudget::chat(),
            approval: if auto_approve {
                agent::ApprovalPolicy::Auto
            } else {
                agent::ApprovalPolicy::Required(generation.approvals.clone())
            },
        }))
    } else {
        let stream = tokio::select! {
            biased;
            _ = generation.cancel.recv() => {
                generation.cancelled(sender).await;
                return Ok(());
            }
            stream = llm_client.chat_stream_with_model(model_name, context_messages, None) => stream,
        };
        match stream {
            Ok(stream) => Box::pin(plain_events(stream)),
            Err(e) => {
                tracing::error!("Failed to create LLM stream: {}", e);
                let error_msg = ServerMessage::Error {
                    message: "Failed to generate response".to_string(),
                };
                let _ = send_server(sender, error_msg).await;
                return Ok(());
            }
        }
    };

    let mut full_content = String::new();
    let mut generated_images = Vec::new();
    let mut chunk_index = 0;
    let mut cancelled = false;
    let mut client_gone = false;
    let mut failure = None;
    let mut response_truncated = false;
    let mut tool_calls: Vec<ToolCallRecord> = Vec::new();
    let mut citations: Vec<Citation> = Vec::new();
    let mut action_receipts: Vec<ActionReceipt> = Vec::new();

    // MAJOR-4: Add overall stream timeout
    let stream_deadline =
        tokio::time::Instant::now() + Duration::from_secs(LLM_STREAM_TIMEOUT_SECS);

    loop {
        tokio::select! {
            biased;
            // Check for cancellation before polling another event.
            _ = generation.cancel.recv() => {
                cancelled = true;
                tracing::debug!("Stream cancelled for message {}", assistant_message_id);
                break;
            }

            // MAJOR-4: Timeout for entire stream
            _ = tokio::time::sleep_until(stream_deadline) => {
                tracing::warn!("LLM stream timeout for chat {}, message {}", chat_id, assistant_message_id);
                failure = Some("Response generation timed out".to_string());
                break;
            }

            // Process agent events
            event = events.next() => {
                match event {
                    Some(AgentEvent::Chunk(content)) => {
                        // MAJOR-3: Check response length limit
                        if full_content.len() + content.len() > MAX_RESPONSE_LENGTH {
                            tracing::warn!(
                                "LLM response exceeded maximum length for message {}",
                                assistant_message_id
                            );
                            response_truncated = true;
                            break;
                        }

                        full_content.push_str(&content);

                        let chunk_msg = ServerMessage::Chunk {
                            content,
                            index: chunk_index,
                        };
                        if !client_gone && !send_server(sender, chunk_msg).await {
                            // The reader navigated away. Keep generating:
                            // the reply still has to be saved, because the
                            // console reloads history from the database.
                            client_gone = true;
                        }
                        chunk_index += 1;
                    }
                    Some(AgentEvent::ToolApprovalRequired { id, name, arguments }) => {
                        if let Some(record) = tool_calls.iter_mut().find(|r| r.id == id) {
                            record.detail = "Waiting for approval…".to_string();
                        }
                        let tool_msg = ServerMessage::ToolApprovalRequired {
                            message_id: assistant_message_id,
                            tool_call_id: id,
                            name,
                            arguments,
                        };
                        if !client_gone && !send_server(sender, tool_msg).await {
                            client_gone = true;
                        }
                    }
                    Some(AgentEvent::ToolCallStarted { id, name, arguments }) => {
                        // Recorded before the tool runs so a turn cancelled
                        // mid-call still shows what it was doing.
                        tool_calls.push(ToolCallRecord {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: arguments.clone(),
                            success: false,
                            detail: "Did not finish".to_string(),
                            duration_ms: 0,
                        });

                        let tool_msg = ServerMessage::ToolCall {
                            message_id: assistant_message_id,
                            tool_call_id: id,
                            name,
                            arguments,
                        };
                        if !client_gone && !send_server(sender, tool_msg).await {
                            client_gone = true;
                        }
                    }
                    Some(AgentEvent::Image(url)) => {
                        // Images arrive as deltas and repeat, so the cap and
                        // the duplicate check both have to live here rather
                        // than in whichever stream produced the event.
                        if generated_images.len() >= MAX_GENERATED_IMAGES
                            || generated_images
                                .iter()
                                .any(|existing: &ChatImageAttachment| existing.url == url)
                        {
                            continue;
                        }

                        let Some(attachment) =
                            generated_image_attachment(&url, generated_images.len())
                        else {
                            tracing::warn!(
                                "Ignored invalid generated image for message {}",
                                assistant_message_id
                            );
                            continue;
                        };

                        generated_images.push(attachment.clone());
                        if !client_gone {
                            let image_msg = ServerMessage::Image {
                                message_id: assistant_message_id,
                                attachment,
                            };
                            if !send_server(sender, image_msg).await {
                                client_gone = true;
                            }
                        }
                    }
                    Some(AgentEvent::ToolCallCompleted { id, name, success, detail, duration_ms, citations: observed, receipt }) => {
                        if let Some(record) = tool_calls.iter_mut().find(|r| r.id == id) {
                            record.success = success;
                            record.detail = detail.clone();
                            record.duration_ms = duration_ms;
                        }
                        crate::agent::citations::merge(&mut citations, observed.clone());

                        let tool_msg = ServerMessage::ToolResult {
                            message_id: assistant_message_id,
                            tool_call_id: id,
                            name,
                            success,
                            detail,
                            duration_ms,
                            citations: observed,
                        };
                        if !client_gone && !send_server(sender, tool_msg).await {
                            client_gone = true;
                        }
                        if let Some(receipt) = receipt {
                            let receipt_msg = ServerMessage::ActionReceipt {
                                message_id: assistant_message_id,
                                receipt: receipt.clone(),
                            };
                            action_receipts.push(receipt);
                            if !client_gone && !send_server(sender, receipt_msg).await {
                                client_gone = true;
                            }
                        }
                    }
                    Some(AgentEvent::Failed(message)) => {
                        failure = Some(message);
                        break;
                    }
                    None => {
                        // Stream ended
                        break;
                    }
                }
            }
        }
    }

    // Drop the producer before acknowledging cancellation so it cannot emit
    // more events after the terminal frame.
    drop(events);

    // Nothing generated yet: there is no reply worth keeping. A turn that ran
    // tools or produced an image before it broke is worth keeping even with no
    // prose, because both show the reader what was attempted.
    if (cancelled || failure.is_some())
        && full_content.is_empty()
        && tool_calls.is_empty()
        && generated_images.is_empty()
    {
        if !client_gone {
            let terminal = match failure {
                Some(message) => ServerMessage::Error { message },
                None => ServerMessage::Cancelled {
                    message_id: Some(assistant_message_id),
                },
            };
            let _ = send_server(sender, terminal).await;
        }
        return Ok(());
    }

    // Save assistant message to database
    // Note: We save even truncated responses so users see partial results
    if response_truncated {
        full_content.push_str("\n\n[Response truncated due to length limit]");
    } else if failure.is_some() {
        full_content.push_str("\n\n[Response interrupted]");
    }

    // Tools can run without the model ever producing prose. The turn is still
    // worth keeping for its trace, but an assistant message with no content
    // reads as a bug, and providers reject one when it comes back as history.
    // An image is its own answer, so it does not need the placeholder.
    if full_content.trim().is_empty() && generated_images.is_empty() {
        full_content = "[Stopped before answering]".to_string();
    }

    // Images, the tool trace, citations, and write receipts share one
    // metadata object, so a turn that produced more than one keeps all of them.
    let assistant_metadata = merge_metadata(
        image_metadata(&generated_images),
        &tool_calls,
        &citations,
        &action_receipts,
    );

    match chats::create_message_with_id(
        state.db(),
        assistant_message_id,
        chat_id,
        "assistant",
        &full_content,
        assistant_metadata.clone(),
    )
    .await
    {
        Ok(msg) => {
            // Spawn background task to generate assistant message embedding
            if !full_content.trim().is_empty() {
                spawn_message_embedding_task(state.clone(), msg.id, chat_id, full_content.clone());
            }

            // CRITICAL-4: Send message end with the SAME ID we sent in MessageStart
            // The database generates msg.id, but we use assistant_message_id for protocol consistency
            if !client_gone {
                let end_msg = if cancelled {
                    ServerMessage::Cancelled {
                        message_id: Some(assistant_message_id),
                    }
                } else {
                    ServerMessage::MessageEnd {
                        message_id: assistant_message_id,
                        content: full_content.clone(),
                        metadata: assistant_metadata,
                        error: failure,
                    }
                };
                let _ = send_server(sender, end_msg).await;
            }

            tracing::debug!(
                "Assistant message completed: chat_id={}, message_id={}, db_id={}, length={}",
                chat_id,
                assistant_message_id,
                msg.id,
                msg.content.len()
            );
        }
        Err(e) => {
            tracing::error!("Failed to save assistant message: {}", e);
            let error_msg = ServerMessage::Error {
                message: "Failed to save response".to_string(),
            };
            let _ = send_server(sender, error_msg).await;
            return Ok(());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_auth_deserialize() {
        let json = r#"{"type": "auth", "token": "test-token-123"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Auth { token } => assert_eq!(token, "test-token-123"),
            _ => panic!("Expected Auth message"),
        }
    }

    #[test]
    fn test_client_message_send_deserialize() {
        let json = r#"{"type": "send", "content": "Hello AI"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Send { content, metadata } => {
                assert_eq!(content, "Hello AI");
                assert!(metadata.is_none());
            }
            _ => panic!("Expected Send message"),
        }
    }

    #[test]
    fn test_client_message_send_with_metadata() {
        let json = r#"{"type": "send", "content": "Hello", "metadata": {"key": "value"}}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Send { content, metadata } => {
                assert_eq!(content, "Hello");
                assert!(metadata.is_some());
                let meta = metadata.unwrap();
                assert_eq!(meta["key"], "value");
            }
            _ => panic!("Expected Send message"),
        }
    }

    #[test]
    fn test_client_message_cancel_deserialize() {
        let json = r#"{"type": "cancel"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Cancel => {}
            _ => panic!("Expected Cancel message"),
        }
    }

    #[test]
    fn test_server_message_init_serialize() {
        let chat_id = Uuid::new_v4();
        let msg = ServerMessage::Init {
            chat_id,
            status: STATUS_CONNECTED.to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"init\""));
        assert!(json.contains("\"status\":\"connected\""));
        assert!(json.contains(&chat_id.to_string()));
    }

    #[test]
    fn test_server_message_message_saved_serialize() {
        let msg = ServerMessage::MessageSaved {
            message_id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "Test message".to_string(),
            metadata: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"message_saved\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Test message\""));
        assert!(!json.contains("\"metadata\""));
    }

    #[test]
    fn test_server_message_message_saved_includes_image_metadata() {
        let metadata = serde_json::json!({
            "attachments": [{
                "name": "shot.png",
                "mime": "image/png",
                "url": "data:image/png;base64,xx"
            }]
        });
        let msg = ServerMessage::MessageSaved {
            message_id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "see this".to_string(),
            metadata: Some(metadata.clone()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"metadata\""));
        assert!(json.contains("shot.png"));
        assert!(json.contains("image/png"));
    }

    #[test]
    fn test_image_urls_from_metadata() {
        let metadata = serde_json::json!({
            "attachments": [
                {
                    "name": "shot.png",
                    "mime": "image/png",
                    "url": "data:image/png;base64,xx"
                },
                {
                    "name": "notes.md",
                    "mime": "text/markdown",
                    "url": "https://example.test/notes.md"
                },
                {
                    "name": "protected.png",
                    "mime": "image/png",
                    "url": "/api/artifacts/00000000-0000-0000-0000-000000000001/00000000-0000-0000-0000-000000000002/00000000-0000-0000-0000-000000000003/image.png"
                }
            ]
        });
        assert_eq!(
            image_urls_from_metadata(Some(&metadata)),
            vec!["data:image/png;base64,xx".to_string()]
        );
        let public = serde_json::json!({
            "attachments": [{
                "name": "remote.png",
                "mime": "image/png",
                "url": "https://example.test/remote.png"
            }]
        });
        assert_eq!(
            image_urls_from_metadata(Some(&public)),
            vec!["https://example.test/remote.png".to_string()]
        );
        assert!(image_urls_from_metadata(None).is_empty());
    }

    #[test]
    fn test_generated_image_attachment_builds_persistable_metadata() {
        let attachment =
            generated_image_attachment("data:image/webp;base64,abc", 0).expect("valid image");
        assert_eq!(
            attachment,
            ChatImageAttachment {
                name: "generated-image-1.webp".to_string(),
                mime: "image/webp".to_string(),
                url: "data:image/webp;base64,abc".to_string(),
            }
        );

        let metadata = image_metadata(&[attachment]).expect("image metadata");
        assert_eq!(metadata["attachments"][0]["name"], "generated-image-1.webp");
    }

    #[test]
    fn test_generated_image_attachment_rejects_non_image_data() {
        assert!(generated_image_attachment("data:text/html;base64,abc", 0).is_none());
        assert!(generated_image_attachment("javascript:alert(1)", 0).is_none());
    }

    #[test]
    fn test_generated_video_attachment_from_artifact_url() {
        let attachment =
            generated_media_attachment("/api/artifacts/ws/chat/msg/clip.webm", "video/webm", 0)
                .expect("valid video");
        assert_eq!(attachment.name, "generated-video-1.webm");
        assert_eq!(attachment.mime, "video/webm");
    }

    #[test]
    fn test_server_message_message_start_serialize() {
        let msg = ServerMessage::MessageStart {
            message_id: Uuid::new_v4(),
            role: "assistant".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"message_start\""));
        assert!(json.contains("\"role\":\"assistant\""));
    }

    #[test]
    fn test_server_message_chunk_serialize() {
        let msg = ServerMessage::Chunk {
            content: "Hello".to_string(),
            index: 5,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"chunk\""));
        assert!(json.contains("\"content\":\"Hello\""));
        assert!(json.contains("\"index\":5"));
    }

    #[test]
    fn test_server_message_tool_call_serialize() {
        let msg = ServerMessage::ToolCall {
            message_id: Uuid::new_v4(),
            tool_call_id: "call_abc".to_string(),
            name: "search_knowledge".to_string(),
            arguments: r#"{"query":"deploys"}"#.to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tool_call\""));
        assert!(json.contains("\"tool_call_id\":\"call_abc\""));
        assert!(json.contains("\"name\":\"search_knowledge\""));
        assert!(json.contains("deploys"));
    }

    #[test]
    fn test_server_message_tool_result_serialize() {
        let msg = ServerMessage::ToolResult {
            message_id: Uuid::new_v4(),
            tool_call_id: "call_abc".to_string(),
            name: "search_knowledge".to_string(),
            success: true,
            detail: "3 passages".to_string(),
            duration_ms: 128,
            citations: Vec::new(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"detail\":\"3 passages\""));
        assert!(json.contains("\"duration_ms\":128"));
        assert!(!json.contains("citations"));
    }

    #[test]
    fn test_tool_call_records_serialize_as_message_metadata() {
        // The console reads this shape back out of `messages.metadata` when a
        // conversation is reopened, so the key and field names are load bearing.
        let records = vec![ToolCallRecord {
            id: "call_abc".to_string(),
            name: "list_tasks".to_string(),
            arguments: "{}".to_string(),
            success: true,
            detail: "2 tasks".to_string(),
            duration_ms: 7,
        }];
        let metadata = serde_json::json!({ "tool_calls": records });

        assert_eq!(metadata["tool_calls"][0]["name"], "list_tasks");
        assert_eq!(metadata["tool_calls"][0]["success"], true);
        assert_eq!(metadata["tool_calls"][0]["duration_ms"], 7);
    }

    #[test]
    fn test_merge_metadata_keeps_images_and_tool_calls() {
        let images = image_metadata(&[ChatImageAttachment {
            name: "generated-image-1.png".to_string(),
            mime: "image/png".to_string(),
            url: "https://example.test/one.png".to_string(),
        }]);
        let records = vec![ToolCallRecord {
            id: "call_1".to_string(),
            name: "run_shell".to_string(),
            arguments: "{}".to_string(),
            success: true,
            detail: "ok".to_string(),
            duration_ms: 3,
        }];

        let merged =
            merge_metadata(images, &records, &[], &[]).expect("both sides produce metadata");
        assert_eq!(merged["attachments"][0]["name"], "generated-image-1.png");
        assert_eq!(merged["tool_calls"][0]["name"], "run_shell");
    }

    #[test]
    fn test_merge_metadata_keeps_citations_with_source_and_revision() {
        let citations = vec![crate::agent::Citation {
            kind: crate::agent::CitationKind::GithubBuild,
            title: "repository main@aaaaaaa".into(),
            url: "https://github.com/owner/repository/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            revision: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
            observed_at: "2026-09-05T00:00:00+00:00".into(),
            complete: false,
            outcome: crate::agent::CitationOutcome::Incomplete,
            note: Some("Observed CI only".into()),
        }];
        let merged =
            merge_metadata(None, &[], &citations, &[]).expect("citations produce metadata");
        assert_eq!(merged["citations"][0]["url"], citations[0].url);
        assert_eq!(
            merged["citations"][0]["revision"],
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(merged["citations"][0]["complete"], false);
        assert_eq!(merged["citations"][0]["outcome"], "incomplete");
        assert!(merged.get("tool_calls").is_none());
    }

    #[test]
    fn test_merge_metadata_keeps_action_receipts() {
        let receipts = vec![ActionReceipt {
            id: "call_1".to_string(),
            action: "create_task".to_string(),
            target_type: crate::agent::ActionTarget::Task,
            target_id: "task-1".to_string(),
            target_label: "Ship".to_string(),
            actor_id: "user-1".to_string(),
            actor_name: "Alice".to_string(),
            occurred_at: "2026-09-05T10:47:00.000Z".to_string(),
            success: true,
            outcome: "Task created".to_string(),
            href: "/tasks?id=task-1".to_string(),
        }];
        let merged = merge_metadata(None, &[], &[], &receipts).expect("receipts produce metadata");
        assert_eq!(merged["action_receipts"][0]["action"], "create_task");
        assert_eq!(merged["action_receipts"][0]["href"], "/tasks?id=task-1");
    }

    #[test]
    fn test_merge_metadata_is_none_when_the_turn_produced_neither() {
        assert!(merge_metadata(None, &[], &[], &[]).is_none());
    }

    #[test]
    fn test_server_message_action_receipt_serialize() {
        let msg = ServerMessage::ActionReceipt {
            message_id: Uuid::new_v4(),
            receipt: ActionReceipt {
                id: "call_1".to_string(),
                action: "create_task".to_string(),
                target_type: crate::agent::ActionTarget::Task,
                target_id: "task-1".to_string(),
                target_label: "Ship".to_string(),
                actor_id: "user-1".to_string(),
                actor_name: "Alice".to_string(),
                occurred_at: "2026-09-05T10:47:00.000Z".to_string(),
                success: true,
                outcome: "Task created".to_string(),
                href: "/tasks?id=task-1".to_string(),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"action_receipt\""));
        assert!(json.contains("\"action\":\"create_task\""));
        assert!(json.contains("\"href\":\"/tasks?id=task-1\""));
    }

    #[test]
    fn test_server_message_message_end_serialize() {
        let metadata = serde_json::json!({
            "attachments": [{
                "name": "generated-image-1.png",
                "mime": "image/png",
                "url": "data:image/png;base64,abc"
            }]
        });
        let msg = ServerMessage::MessageEnd {
            message_id: Uuid::new_v4(),
            content: "Full response".to_string(),
            metadata: Some(metadata),
            error: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"message_end\""));
        assert!(json.contains("\"content\":\"Full response\""));
        assert!(json.contains("\"metadata\""));
        assert!(json.contains("generated-image-1.png"));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_server_message_cancelled_serialize() {
        let msg = ServerMessage::Cancelled {
            message_id: Some(Uuid::new_v4()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"cancelled\""));

        let msg_none = ServerMessage::Cancelled { message_id: None };
        let json_none = serde_json::to_string(&msg_none).unwrap();
        assert!(json_none.contains("\"type\":\"cancelled\""));
        assert!(json_none.contains("null"));
    }

    #[test]
    fn test_server_message_error_serialize() {
        let msg = ServerMessage::Error {
            message: "Something went wrong".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"message\":\"Something went wrong\""));
    }

    #[test]
    fn test_server_message_status_serialize() {
        let msg = ServerMessage::Status {
            message: "Searching the web...".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"status\""));
        assert!(json.contains("Searching the web..."));
    }

    #[test]
    fn test_server_message_to_ws_message() {
        let msg = ServerMessage::Error {
            message: "test".to_string(),
        };
        let ws_msg = msg.to_ws_message();
        match ws_msg {
            Message::Text(text) => {
                assert!(text.contains("\"type\":\"error\""));
                assert!(text.contains("\"message\":\"test\""));
            }
            _ => panic!("Expected Text message"),
        }
    }

    #[test]
    fn test_constants() {
        assert_eq!(WS_AUTH_TIMEOUT_SECS, 30);
        assert_eq!(AUTH_RECHECK_INTERVAL, 200);
        assert_eq!(WS_IDLE_TIMEOUT_SECS, 300);
        assert_eq!(WS_PING_INTERVAL_SECS, 30);
        assert_eq!(MAX_CONSECUTIVE_ERRORS, 5);
        assert_eq!(MAX_CONNECTIONS_PER_CHAT, 5);
        assert_eq!(MAX_MESSAGES_PER_MINUTE, 20);
        assert_eq!(MAX_MESSAGE_LENGTH, 100_000);
        assert_eq!(MAX_CONTEXT_MESSAGES, 50);
        assert_eq!(MAX_CONTEXT_RESULTS, 10);
        assert_eq!(MAX_CONTEXT_IN_PROMPT, 5);
        assert_eq!(MAX_RESPONSE_LENGTH, 100_000);
        assert_eq!(MAX_GENERATED_IMAGES, 8);
        assert_eq!(LLM_STREAM_TIMEOUT_SECS, 300);
        assert_eq!(STATUS_CONNECTED, "connected");
    }

    #[test]
    fn test_message_serialization_roundtrip() {
        // Test that all message types can be serialized and deserialized
        let messages = vec![
            ServerMessage::Init {
                chat_id: Uuid::new_v4(),
                status: "connected".to_string(),
            },
            ServerMessage::Chunk {
                content: "test".to_string(),
                index: 0,
            },
            ServerMessage::Error {
                message: "error".to_string(),
            },
        ];

        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            // Just verify it can be serialized without panicking
            assert!(!json.is_empty());
        }
    }

    #[test]
    fn workspace_saved_messages_preserve_role_and_metadata() {
        let id = Uuid::new_v4();
        let metadata = serde_json::json!({"source":"reminder","actor_id":Uuid::new_v4()});
        let saved = saved_action(&serde_json::json!({"id":id,"role":"assistant","content":"Follow up","metadata":metadata})).unwrap();
        let value = serde_json::to_value(saved).unwrap();
        assert_eq!(value["type"], "message_saved");
        assert_eq!(value["message_id"], id.to_string());
        assert_eq!(value["role"], "assistant");
        assert_eq!(value["metadata"], metadata);
        assert!(saved_action(&serde_json::json!({"id":"invalid"})).is_none());
    }

    #[test]
    fn approve_tool_client_message_round_trips() {
        let parsed: ClientMessage = serde_json::from_value(serde_json::json!({
            "type": "approve_tool",
            "tool_call_id": "call_1",
            "approved": false
        }))
        .unwrap();
        match parsed {
            ClientMessage::ApproveTool {
                tool_call_id,
                approved,
            } => {
                assert_eq!(tool_call_id, "call_1");
                assert!(!approved);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn tool_approval_required_serializes_for_the_console() {
        let json = serde_json::to_value(ServerMessage::ToolApprovalRequired {
            message_id: Uuid::nil(),
            tool_call_id: "call_1".into(),
            name: "write_file".into(),
            arguments: r#"{"path":"x"}"#.into(),
        })
        .unwrap();
        assert_eq!(json["type"], "tool_approval_required");
        assert_eq!(json["tool_call_id"], "call_1");
        assert_eq!(json["name"], "write_file");
    }
}
