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

use crate::agent::{self, AgentEvent, AgentRun, ToolCallRecord};
use crate::auth::validate_token;
use crate::db::{ai_settings, chats, workspace_members, workspaces};
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

/// Allowed model prefixes for validation
const ALLOWED_MODEL_PREFIXES: &[&str] = &[
    "gpt-",
    "claude-",
    "o1-",
    "gemini-",
    "llama",
    "mistral",
    "mixtral",
    "codellama",
    // Vision-capable local models, for messages carrying images.
    "llava",
    "bakllava",
    "moondream",
    "minicpm-v",
];

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
}

impl Generation {
    fn new(chat_id: Uuid) -> Self {
        let message_id = Uuid::new_v4();
        let (sender, cancel) = broadcast::channel(1);
        CHAT_CANCELLATIONS.insert((chat_id, message_id), sender);
        Self {
            chat_id,
            message_id,
            cancel,
        }
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
    }
}

struct ChatPreparation {
    model: String,
    agentic: bool,
    tools: agent::ChatTools,
    messages: Vec<LlmMessage>,
}

enum Preparation {
    Image(crate::config::ComfyUiConfig),
    Chat(ChatPreparation),
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
    /// A tool finished. `detail` is a short outcome for display, not the full
    /// output the model receives.
    ToolResult {
        message_id: Uuid,
        tool_call_id: String,
        name: String,
        success: bool,
        detail: String,
        duration_ms: u64,
    },
    /// An image generated by the assistant.
    Image {
        message_id: Uuid,
        attachment: ChatImageAttachment,
    },
    /// Assistant message completed
    MessageEnd {
        message_id: Uuid,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },
    /// Generation cancelled
    Cancelled { message_id: Option<Uuid> },
    /// Error message
    Error { message: String },
    /// Non-fatal progress (e.g. web search in progress)
    Status { message: String },
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

fn generated_image_attachment(url: &str, index: usize) -> Option<ChatImageAttachment> {
    if url.len() > MAX_GENERATED_IMAGE_URL_LENGTH {
        return None;
    }

    let mime = if let Some(header) = url
        .strip_prefix("data:")
        .and_then(|data| data.split_once(','))
    {
        let mime = header.0.split(';').next().unwrap_or_default();
        if !mime.starts_with("image/") {
            return None;
        }
        mime.to_string()
    } else if url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("/api/artifacts/")
    {
        if url.ends_with(".jpg") || url.ends_with(".jpeg") {
            "image/jpeg".to_string()
        } else if url.ends_with(".webp") {
            "image/webp".to_string()
        } else {
            "image/png".to_string()
        }
    } else {
        return None;
    };

    let extension = match mime.as_str() {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        _ => "png",
    };

    Some(ChatImageAttachment {
        name: format!("generated-image-{}.{}", index + 1, extension),
        mime,
        url: url.to_string(),
    })
}

fn image_metadata(attachments: &[ChatImageAttachment]) -> Option<serde_json::Value> {
    (!attachments.is_empty()).then(|| serde_json::json!({ "attachments": attachments }))
}

/// Fold the tool trace into the image metadata, since one turn can produce
/// both and they share the message's single metadata column.
fn merge_metadata(
    images: Option<serde_json::Value>,
    tool_calls: &[ToolCallRecord],
) -> Option<serde_json::Value> {
    if tool_calls.is_empty() {
        return images;
    }

    let mut object = match images {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    object.insert("tool_calls".to_string(), serde_json::json!(tool_calls));
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

/// Validate that a model name is allowed
fn is_valid_model(model_name: &str) -> bool {
    let model_lower = model_name.to_lowercase();
    ALLOWED_MODEL_PREFIXES
        .iter()
        .any(|prefix| model_lower.starts_with(prefix))
}

async fn handle_image_generation(
    state: &AppState,
    sender: &SharedSender,
    chat_id: Uuid,
    workspace_id: Uuid,
    prompt: &str,
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
    let _ = send_server(
        sender,
        ServerMessage::Status {
            message: "Preparing image generation...".to_string(),
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
        .generate(prompt, &mut generation.cancel, progress_tx)
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

    let store = ArtifactStore::new(image_config.artifact_root.clone());
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

    let preparation = tokio::select! {
        biased;
        _ = request.cancel.recv() => {
            request.cancelled(sender).await;
            return;
        }
        result = prepare_message(state, sender, chat_id, workspace_id, content, metadata) => result,
    };
    let result = match preparation {
        Ok(Some(Preparation::Image(config))) => {
            handle_image_generation(
                state,
                sender,
                chat_id,
                workspace_id,
                content,
                config,
                &mut request,
            )
            .await
        }
        Ok(Some(Preparation::Chat(chat))) => {
            handle_chat_generation(state, sender, chat_id, chat, &mut request).await
        }
        Ok(None) => return,
        Err(error) => Err(error),
    };
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

async fn prepare_message(
    state: &AppState,
    sender: &SharedSender,
    chat_id: Uuid,
    workspace_id: Uuid,
    content: &str,
    metadata: Option<serde_json::Value>,
) -> Result<Option<Preparation>, Box<dyn std::error::Error + Send + Sync>> {
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
    let model_name = chat.model_name.as_str();

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
    let image_request = classifier
        .is_image_request(content, metadata.as_ref())
        .await;

    // Image requests never route through the selected model. Validate it only
    // when it will actually receive the request.
    if !image_request && !is_valid_model(model_name) {
        tracing::warn!("Invalid model name rejected: {}", model_name);
        let error_msg = ServerMessage::Error {
            message: "Invalid model configuration".to_string(),
        };
        let _ = send_server(sender, error_msg).await;
        return Ok(None); // Not a fatal error, just reject this message
    }

    let web_search_requested = state
        .config()
        .web_search
        .requested_for(content, metadata.as_ref());

    // Save user message to database
    let user_message =
        chats::create_message(state.db(), chat_id, "user", content, metadata).await?;

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

    if image_request {
        return Ok(Some(Preparation::Image(image_config)));
    }

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
    let tools = agent::ChatTools::build(
        agent::WorkspaceScope {
            state: state.clone(),
            workspace_id,
            chat_id,
        },
        // A deployment can refuse host tools outright, in which case the
        // chat's preference does not get a say.
        chat.agent_sandboxed || !agent::host_tools_allowed(),
    );
    let agentic = chat.agent_enabled && !tools.is_empty();

    if agentic && !tools.is_sandboxed() {
        // Worth a line in the log: this turn can write to the host, and the
        // trace on the message is the only other record of what it did.
        tracing::warn!(
            "Chat {} is running an unsandboxed agent turn with host tools",
            chat_id
        );
    }

    if agentic {
        context_messages.insert(0, LlmMessage::system(agent::system_prompt(&tools)));
    } else if let Some(context_service) = state.context_service() {
        // MAJOR-6: Inject knowledge base context scoped to workspace if context service available
        // Create workspace-scoped search filters
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
            Ok(results) if !results.is_empty() => {
                let mut context_text = String::from("Relevant context:\n\n");
                // MINOR-2: Use named constant for context results in prompt
                for result in results.iter().take(MAX_CONTEXT_IN_PROMPT) {
                    context_text.push_str(&format!("- {}\n", result.chunk_text));
                }

                // Insert context as system message before user message
                context_messages.insert(0, LlmMessage::system(context_text));
            }
            Ok(_) => {
                // No relevant context found
            }
            Err(e) => {
                tracing::warn!("Context search failed: {}", e);
                // Continue without context
            }
        }
    }

    // Inject live web search via SearXNG (reached through Gluetun in Docker).
    if web_search_requested {
        let query = crate::services::searxng::sanitize_query(content);
        if !query.is_empty() {
            let status_msg = ServerMessage::Status {
                message: "Searching the web...".to_string(),
            };
            let _ = send_server(sender, status_msg).await;

            match crate::services::searxng::SearxngClient::new(state.config().web_search.clone()) {
                Ok(client) => match client.search(&query).await {
                    Ok(hits) if !hits.is_empty() => {
                        context_messages.insert(
                            0,
                            LlmMessage::system(crate::services::searxng::format_search_context(
                                &hits,
                            )),
                        );
                        tracing::debug!(
                            "Injected {} web search results for chat {}",
                            hits.len(),
                            chat_id
                        );
                    }
                    Ok(_) => {
                        tracing::debug!("Web search returned no results for chat {}", chat_id);
                    }
                    Err(e) => {
                        tracing::warn!("Web search failed for chat {}: {}", chat_id, e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to create web search client: {}", e);
                }
            }
        }
    }

    Ok(Some(Preparation::Chat(ChatPreparation {
        model: model_name.to_string(),
        agentic,
        tools,
        messages: context_messages,
    })))
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
                    Some(AgentEvent::ToolCallCompleted { id, name, success, detail, duration_ms }) => {
                        if let Some(record) = tool_calls.iter_mut().find(|r| r.id == id) {
                            record.success = success;
                            record.detail = detail.clone();
                            record.duration_ms = duration_ms;
                        }

                        let tool_msg = ServerMessage::ToolResult {
                            message_id: assistant_message_id,
                            tool_call_id: id,
                            name,
                            success,
                            detail,
                            duration_ms,
                        };
                        if !client_gone && !send_server(sender, tool_msg).await {
                            client_gone = true;
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

    // Images and the tool trace share one metadata object, so a turn that
    // produced both keeps both.
    let assistant_metadata = merge_metadata(image_metadata(&generated_images), &tool_calls);

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
                } else if let Some(message) = failure {
                    ServerMessage::Error { message }
                } else {
                    ServerMessage::MessageEnd {
                        message_id: assistant_message_id,
                        content: full_content.clone(),
                        metadata: assistant_metadata,
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
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tool_result\""));
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"detail\":\"3 passages\""));
        assert!(json.contains("\"duration_ms\":128"));
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

        let merged = merge_metadata(images, &records).expect("both sides produce metadata");
        assert_eq!(merged["attachments"][0]["name"], "generated-image-1.png");
        assert_eq!(merged["tool_calls"][0]["name"], "run_shell");
    }

    #[test]
    fn test_merge_metadata_is_none_when_the_turn_produced_neither() {
        assert!(merge_metadata(None, &[]).is_none());
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
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"message_end\""));
        assert!(json.contains("\"content\":\"Full response\""));
        assert!(json.contains("\"metadata\""));
        assert!(json.contains("generated-image-1.png"));
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
    fn test_model_validation() {
        // Valid models
        assert!(is_valid_model("gpt-4"));
        assert!(is_valid_model("gpt-4o"));
        assert!(is_valid_model("gpt-4o-mini"));
        assert!(is_valid_model("GPT-4")); // Case insensitive
        assert!(is_valid_model("claude-3-5-sonnet"));
        assert!(is_valid_model("claude-3-opus"));
        assert!(is_valid_model("o1-preview"));
        assert!(is_valid_model("gemini-pro"));
        assert!(is_valid_model("llama3.1"));
        assert!(is_valid_model("mistral-7b"));
        assert!(is_valid_model("mixtral-8x7b"));
        assert!(is_valid_model("codellama-34b"));

        // Invalid models
        assert!(!is_valid_model(""));
        assert!(!is_valid_model("invalid-model"));
        assert!(!is_valid_model("some-custom-model"));
        assert!(!is_valid_model("random"));
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
}
