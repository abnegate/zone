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
//!    A "send" during a turn is queued; "interrupt", "edit_queued" and
//!    "remove_queued" act on the queue, and every change is sent as "queue"
//! 6. Connection closes on error or client disconnect

use abnegate_agent::chat::{Capacity, ReplayMessage};
use abnegate_agent::context::ContextUsage;
use abnegate_agent::tool::Session as ToolSession;
use abnegate_agent::tool::job::{JobStarted, JobStatus, Jobs, parse_receipt};
use abnegate_agent_cli::SubagentStep;
use abnegate_comfy::MediaType;
use abnegate_llm::{Message as LlmMessage, Role as LlmRole, Usage};
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
use sqlx::PgPool;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore, broadcast, mpsc};
use uuid::Uuid;
use zone_core::llm::{BuiltinTools, LlmBackend, LlmClient, Observer};

use crate::agent::wait::{self, Outcome, WaitSettled, Waited, Waiting};
use crate::agent::{
    self, ActionReceipt, AgentEvent, AgentRun, ChatTools, Citation, Question, Refusal, Spend,
    ToolCallRecord, WorkspaceScope,
};
use crate::auth::validate_access_token;
use crate::db::{
    self, chat_attached_sources, chat_sources, chats, knowledge, sessions, workspace_members,
};
use crate::mcp::AgentTools;
#[cfg(test)]
use crate::services::character::ChatCharacter;
use crate::services::chat::authorship;
use crate::services::chat::driver::Driver;
use crate::services::chat::handover::{self, Cause, Handover, Notice};
use crate::services::chat::observers::Observers;
use crate::services::chat::pull_request::Recorder;
use crate::services::chat::session::{self, Session};
use crate::services::chat::subagent::{Announcer, ChatSubagent, Tracker};
use crate::services::completion_tokens::{FilterStep, TokenFilter};
use crate::services::endpoint::{Endpoint, Origin};
use crate::services::login::router;
use crate::services::project_directory::{self, Start};
use crate::services::route::Route;
use crate::state::AppState;
use crate::workers::embeddings::spawn_message_embedding_task;
use abnegate_search::{SearchContext, SearchHit, SearxngClient, sanitize_query};

mod ending;
pub mod job;
mod queue;

use ending::Ending;
use queue::{Finished, Interrupted, Queued, Started};

/// WebSocket polling interval in milliseconds
const WS_POLL_INTERVAL_MS: u64 = 50;

/// Authentication timeout in seconds
const WS_AUTH_TIMEOUT_SECS: u64 = 30;

/// Re-check authorization every 10 seconds.
const AUTH_RECHECK_INTERVAL: Duration = Duration::from_secs(10);

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

/// Maximum context search results
const MAX_CONTEXT_RESULTS: usize = 10;

/// Maximum context results to include in prompt
const MAX_CONTEXT_IN_PROMPT: usize = 5;

/// Maximum response length (100KB - same as message)
const MAX_RESPONSE_LENGTH: usize = 100_000;

const TRUNCATED_NOTE: &str = "\n\n[Response truncated due to length limit]";

const INTERRUPTED_NOTE: &str = "\n\n[Response interrupted]";

/// The answer a coding agent gave before it was stopped stands, so the turn is no failure, but
/// the reader is told it did not end on its own.
const CUT_SHORT_NOTE: &str = "\n\n[Cut short: the agent was stopped before it finished]";

/// Bound image output independently from text so a provider cannot make a
/// WebSocket frame or message metadata grow without limit.
const MAX_GENERATED_IMAGES: usize = 8;
const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
const MAX_GENERATED_IMAGE_URL_LENGTH: usize = 16 * 1024 * 1024;

/// Status constants
const STATUS_CONNECTED: &str = "connected";

/// Global connection limiter per chat
static CHAT_CONNECTIONS: Lazy<DashMap<Uuid, Arc<Semaphore>>> = Lazy::new(DashMap::new);

async fn can_access_chat(
    state: &AppState,
    user_id: Uuid,
    session_id: Uuid,
    workspace_id: Uuid,
) -> db::DbResult<bool> {
    let (session_active, can_write) = tokio::try_join!(
        sessions::is_active_user_session(state.db(), session_id, user_id),
        workspace_members::can_write(state.db(), workspace_id, user_id),
    )?;

    Ok(session_active && can_write)
}

/// Live frames per chat, kept while a connection or a generation holds one.
static CHAT_STREAMS: Lazy<DashMap<Uuid, Weak<ChatStream>>> = Lazy::new(DashMap::new);

/// Frames a connection may fall behind by before it re-joins the stream.
const CHAT_STREAM_CAPACITY: usize = 256;

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

fn snippet_line(text: &str, max_chars: usize) -> String {
    let note = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if note.chars().count() > max_chars {
        format!("{}…", note.chars().take(max_chars).collect::<String>())
    } else {
        note
    }
}

fn format_retrieved_line(kind: &str, title: &str, uri: &str, text: &str) -> String {
    format!("- [{kind}] {title} ({uri}): {}", snippet_line(text, 500))
}

async fn emit_chunk(
    stream: &ChatStream,
    full_content: &mut String,
    chunk_index: &mut u32,
    content: String,
    response_truncated: &mut bool,
) -> bool {
    if content.is_empty() {
        return true;
    }
    if full_content.len() + content.len() > MAX_RESPONSE_LENGTH {
        *response_truncated = true;
        return false;
    }
    full_content.push_str(&content);
    let chunk_msg = ServerMessage::Chunk {
        content,
        index: *chunk_index,
    };
    publish(stream, chunk_msg).await;
    *chunk_index += 1;
    true
}

fn interleave_context_lines(
    knowledge: Vec<String>,
    sources: Vec<String>,
    limit: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut knowledge = knowledge.into_iter();
    let mut sources = sources.into_iter();
    loop {
        if out.len() >= limit {
            break;
        }
        let mut progressed = false;
        if let Some(line) = knowledge.next() {
            out.push(line);
            progressed = true;
        }
        if out.len() >= limit {
            break;
        }
        if let Some(line) = sources.next() {
            out.push(line);
            progressed = true;
        }
        if !progressed {
            break;
        }
    }
    out
}

async fn send_server(sender: &SharedSender, message: ServerMessage) -> bool {
    sender
        .lock()
        .await
        .send(message.to_ws_message())
        .await
        .is_ok()
}

/// Frames already published for the turn in flight.
///
/// A turn belongs to the chat rather than to the socket that started it, so a
/// connection that arrives while one is running replays this log and picks the
/// reply up where it left off. Before its message starts, a turn has only its
/// latest status to show, such as a media job queued behind another.
#[derive(Default)]
struct LiveTurn {
    frames: Vec<ServerMessage>,
    status: Option<ServerMessage>,
}

impl LiveTurn {
    fn record(&mut self, message: &ServerMessage) {
        match message {
            ServerMessage::Subagent { .. } | ServerMessage::SubagentsRemoved { .. } => {}
            ServerMessage::MessageStart { .. } => {
                self.status = None;
                self.frames.clear();
                self.frames.push(message.clone());
            }
            ServerMessage::MessageEnd { .. }
            | ServerMessage::Cancelled { .. }
            | ServerMessage::Error { .. } => {
                self.status = None;
                self.frames.clear();
            }
            ServerMessage::Status { .. } if self.frames.is_empty() => {
                self.status = Some(message.clone());
            }
            // Anything else outside a turn is already in the chat a joining client loads.
            _ if self.frames.is_empty() => {}
            // Text arrives a token at a time, so keeping a frame each would
            // make the log as long as the reply.
            ServerMessage::Chunk { content, .. } => match self.frames.last_mut() {
                Some(ServerMessage::Chunk {
                    content: earlier, ..
                }) => earlier.push_str(content),
                _ => self.frames.push(message.clone()),
            },
            ServerMessage::Reasoning { content } => match self.frames.last_mut() {
                Some(ServerMessage::Reasoning { content: earlier }) => earlier.push_str(content),
                _ => self.frames.push(message.clone()),
            },
            _ => self.frames.push(message.clone()),
        }
    }

    fn replay(&self) -> Vec<ServerMessage> {
        if self.frames.is_empty() {
            return self.status.iter().cloned().collect();
        }
        self.frames
            .iter()
            .map(|frame| match frame {
                ServerMessage::MessageStart {
                    message_id, role, ..
                } => ServerMessage::MessageStart {
                    message_id: *message_id,
                    role: role.clone(),
                    resumed: true,
                },
                frame => frame.clone(),
            })
            .collect()
    }
}

/// The frames of one chat, shared by every connection to it.
struct ChatStream {
    chat_id: Uuid,
    events: broadcast::Sender<ServerMessage>,
    live: Mutex<LiveTurn>,
}

impl ChatStream {
    fn of(chat_id: Uuid) -> Arc<Self> {
        let mut entry = CHAT_STREAMS.entry(chat_id).or_default();
        if let Some(stream) = entry.upgrade() {
            return stream;
        }
        let stream = Arc::new(Self {
            chat_id,
            events: broadcast::channel(CHAT_STREAM_CAPACITY).0,
            live: Mutex::new(LiveTurn::default()),
        });
        *entry = Arc::downgrade(&stream);
        stream
    }

    /// Replay the turn in flight and subscribe under one lock, so a joining
    /// connection can neither miss a frame published between the two nor see
    /// one twice.
    async fn join(&self) -> (Vec<ServerMessage>, broadcast::Receiver<ServerMessage>) {
        let live = self.live.lock().await;
        let mut frames = live.replay();
        if let Some((messages, held)) = queue::snapshot(self.chat_id) {
            frames.push(ServerMessage::Queue {
                messages,
                held,
                started: None,
            });
        }
        (frames, self.events.subscribe())
    }
}

impl Drop for ChatStream {
    fn drop(&mut self) {
        CHAT_STREAMS.remove_if(&self.chat_id, |_, stream| stream.strong_count() == 0);
    }
}

/// Record and broadcast under one lock, so the log and the subscribers stay in
/// the same order.
async fn publish(stream: &ChatStream, message: ServerMessage) {
    let mut live = stream.live.lock().await;
    live.record(&message);
    let _ = stream.events.send(message);
}

/// Publishes each change a turn's tracker stores to a chat's subagents.
struct SubagentFrames {
    stream: Arc<ChatStream>,
}

#[async_trait::async_trait]
impl Announcer for SubagentFrames {
    async fn announce(&self, subagent: ChatSubagent, steps: Vec<SubagentStep>) {
        let chat_id = self.stream.chat_id;
        publish(
            &self.stream,
            ServerMessage::Subagent {
                chat_id,
                subagent,
                steps,
            },
        )
        .await;
    }
}

/// Tells every connection to each subagent's chat that a sweep stopped it.
pub(crate) async fn announce_stopped_subagents(stopped: Vec<db::chat_subagents::Stopped>) {
    for db::chat_subagents::Stopped { subagent, failed } in stopped {
        let frames = SubagentFrames {
            stream: ChatStream::of(subagent.chat_id),
        };
        frames
            .announce(ChatSubagent::from(subagent), failed.0)
            .await;
    }
}

/// Tells every connection to `chat_id` that deleting a message removed these
/// subagents.
pub(crate) async fn announce_removed_subagents(chat_id: Uuid, ids: Vec<Uuid>) {
    if ids.is_empty() {
        return;
    }
    publish(
        &ChatStream::of(chat_id),
        ServerMessage::SubagentsRemoved { chat_id, ids },
    )
    .await;
}

/// Broadcast the queue as it is now. Read under the lock a joining connection
/// reads it under, so the last queue frame every connection sees is current.
async fn announce(stream: &ChatStream, started: Option<Queued>) {
    let _live = stream.live.lock().await;
    let (messages, held) = queue::snapshot(stream.chat_id).unwrap_or_default();
    let _ = stream.events.send(ServerMessage::Queue {
        messages,
        held,
        started,
    });
}

/// A turn holds the chat's generation permit, including one the queue did not start.
fn generating(chat_id: Uuid) -> bool {
    CHAT_GENERATIONS
        .get(&chat_id)
        .is_some_and(|generation| generation.available_permits() == 0)
}

fn cancel_turns(chat_id: Uuid) {
    for entry in CHAT_CANCELLATIONS
        .iter()
        .filter(|entry| entry.key().0 == chat_id)
    {
        let _ = entry.value().send(());
    }
    crate::agent::ApprovalPolicy::deny_chat(chat_id);
}

/// Register the turn a message is starting under the queue's lock, so a Stop
/// or Interrupt that takes the lock next cancels this turn and not one before it.
fn begin(chat_id: Uuid, started: Started) -> (Queued, Generation) {
    let generation = Generation::registered(chat_id, started.turn);
    (started.message, generation)
}

/// Run a message and then whatever was queued behind it, with nobody needing
/// to stay connected.
fn drive(
    state: AppState,
    stream: Arc<ChatStream>,
    chat_id: Uuid,
    workspace_id: Uuid,
    first: (Queued, Generation),
    told: bool,
) {
    let announcer = stream.clone();
    let started = told.then(|| first.0.clone());
    let turn = move |(message, generation): (Queued, Generation)| {
        let state = state.clone();
        let stream = stream.clone();
        async move {
            handle_send_message(
                &state,
                &stream,
                chat_id,
                workspace_id,
                message.author,
                &message.content,
                message.metadata,
                generation,
                false,
            )
            .await
        }
    };
    let running = turn(first);
    let teller = announcer.clone();
    tokio::spawn(async move {
        if started.is_some() {
            announce(&announcer, started).await;
        }
        let draining = queue::drain(
            chat_id,
            running,
            move |started| turn(begin(chat_id, started)),
            move |started| {
                let stream = teller.clone();
                async move { announce(&stream, started).await }
            },
        );
        let drained =
            futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(draining)).await;
        if drained.is_err() {
            announce(&announcer, None).await;
        }
    });
}

/// Send frames the caller already holds to one connection.
async fn forward(sender: &SharedSender, frames: Vec<ServerMessage>) -> bool {
    for frame in frames {
        if !send_server(sender, frame).await {
            return false;
        }
    }
    true
}

/// A request owns its cancellation registration from the moment the socket
/// accepts it, including time spent waiting or preparing context.
struct Generation {
    chat_id: Uuid,
    message_id: Uuid,
    cancel: broadcast::Receiver<()>,
    approvals: crate::agent::ApprovalPolicy,
    started: bool,
}

impl Generation {
    fn new(chat_id: Uuid) -> Self {
        Self::registered(chat_id, Uuid::new_v4())
    }

    fn registered(chat_id: Uuid, message_id: Uuid) -> Self {
        let (sender, cancel) = broadcast::channel(1);
        CHAT_CANCELLATIONS.insert((chat_id, message_id), sender);
        Self {
            chat_id,
            message_id,
            cancel,
            approvals: crate::agent::ApprovalPolicy::required(crate::agent::ApprovalGate::new()),
            started: false,
        }
    }

    fn is_cancelled(&mut self) -> bool {
        !matches!(
            self.cancel.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        )
    }

    fn cancellation(&self) -> broadcast::Sender<()> {
        CHAT_CANCELLATIONS
            .get(&(self.chat_id, self.message_id))
            .map(|sender| sender.value().clone())
            .expect("active generation cancellation remains registered")
    }

    async fn cancelled(&self, stream: &ChatStream) {
        publish(
            stream,
            ServerMessage::Cancelled {
                message_id: self.started.then_some(self.message_id),
            },
        )
        .await;
    }
}

impl Drop for Generation {
    fn drop(&mut self) {
        CHAT_CANCELLATIONS.remove(&(self.chat_id, self.message_id));
        crate::agent::ApprovalPolicy::unregister(self.chat_id, &self.approvals);
        self.approvals.deny_all();
    }
}

type ChatPreparation = session::Preparation;

enum Routing {
    Image(crate::config::ComfyUiConfig, LlmBackend, Endpoint),
    Video(crate::config::ComfyUiConfig),
    Audio(crate::config::ComfyUiConfig),
    Upscale(crate::config::ComfyUiConfig),
    Chat(chats::ChatRow, Box<Route>),
}

/// What a provider reported when a turn failed, remedied for a coding agent's
/// sign-in and without the endpoint's key.
fn failure(endpoint: &Endpoint, backend: &LlmBackend, message: String) -> String {
    endpoint.scrub(&crate::services::backend::remedied(backend, message).message)
}

/// Client message types
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Authenticate with JWT
    Auth {
        token: String,
    },
    /// Send a new user message, queued behind a turn that is still running.
    Send {
        #[serde(default = "Uuid::new_v4")]
        id: Uuid,
        content: String,
        #[serde(default)]
        metadata: Option<serde_json::Value>,
    },
    /// Cancel current generation
    Cancel,
    /// Stop the running turn and send this queued message next.
    Interrupt {
        id: Uuid,
    },
    EditQueued {
        id: Uuid,
        content: String,
    },
    RemoveQueued {
        id: Uuid,
    },
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
    /// Assistant message started. `resumed` marks a replay to a connection
    /// that joined a turn already in flight, so it revives that message
    /// instead of starting a second one.
    MessageStart {
        message_id: Uuid,
        role: String,
        #[serde(skip_serializing_if = "is_not_resumed")]
        resumed: bool,
    },
    Context {
        chat_id: Uuid,
        message_id: Option<Uuid>,
        usage: ContextUsage,
    },
    /// Content chunk streamed
    Chunk { content: String, index: u32 },
    /// Thinking tokens from a model that advertised reasoning.
    Reasoning { content: String },
    /// The agent started running a tool
    ToolCall {
        message_id: Uuid,
        tool_call_id: String,
        name: String,
        arguments: String,
        at: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_at: Option<usize>,
        /// The model's stated reason, for side-effecting tools that carry one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A call is waiting for the user to allow it: a host write or command, or
    /// anything that leaves the workspace.
    ToolApprovalRequired {
        message_id: Uuid,
        tool_call_id: String,
        name: String,
        arguments: String,
        /// Why the model says it needs this. Model-authored, so the console
        /// shows it as a claim the reader is being asked to weigh.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// What the call will do, rendered by the server from the arguments
        /// themselves. Observed, not claimed, so it is what settles a
        /// disagreement between the two.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preview: Option<String>,
    },
    /// A decision arrived for a call that is no longer waiting: another
    /// connection answered the card first, or the turn moved past it. Nothing
    /// about the turn has gone wrong, so this is its own frame rather than an
    /// `Error`, which the console reads as the reply having died. It goes only
    /// to the connection that decided.
    ToolApprovalClosed { tool_call_id: String },
    /// The model put a structured question to the reader, and the turn ends
    /// here. There is no answering frame: what the reader chooses arrives as
    /// an ordinary `send`, so this is the last decision point of the turn.
    QuestionRequired {
        message_id: Uuid,
        tool_call_id: String,
        questions: Vec<Question>,
    },
    /// A background job started. `tool_call_id` is the call that spawned it, so
    /// the console can hang the job card off that call.
    JobStarted {
        message_id: Uuid,
        tool_call_id: String,
        job: JobStarted,
    },
    /// A background job is no longer running. Matched to its card by `job.id`
    /// rather than by a call, because the call that observes an exit is rarely
    /// the call that started the job.
    JobExited { message_id: Uuid, job: job::JobExit },
    /// The model opened a wait, and the turn is suspended on it. Unlike a
    /// question this is not the last frame of the turn: a second run resumes
    /// the same message once the wait settles.
    WaitStarted {
        message_id: Uuid,
        tool_call_id: String,
        waiting: Waiting,
    },
    /// How a wait ended. The model is told the same thing through the outcome
    /// entry injected into its context, never through this frame.
    WaitSettled {
        message_id: Uuid,
        settled: WaitSettled,
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
        /// Always sent, as it is always stored on the record.
        refusal: Option<Refusal>,
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
    /// An audio clip generated by the assistant.
    Audio {
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
    /// The whole queue, sent on every change. `started` is the message that
    /// just left it to begin a turn.
    Queue {
        messages: Vec<Queued>,
        held: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        started: Option<Queued>,
    },
    /// Error message
    Error { message: String },
    /// Non-fatal progress (e.g. web search in progress)
    Status { message: String },
    /// The turn moved to another login and carries on in the same answer.
    Handover {
        message_id: Uuid,
        #[serde(flatten)]
        notice: Notice,
    },
    /// One of the chat's subagents changed. `steps` holds only the steps this
    /// change touched. Never replayed: a joining client reads them over REST.
    Subagent {
        chat_id: Uuid,
        subagent: ChatSubagent,
        steps: Vec<SubagentStep>,
    },
    /// Deleting a message removed the subagents it started. Never replayed.
    SubagentsRemoved { chat_id: Uuid, ids: Vec<Uuid> },
}

fn is_not_resumed(resumed: &bool) -> bool {
    !*resumed
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
        if !mime.starts_with("image/") && !mime.starts_with("video/") && !mime.starts_with("audio/")
        {
            return None;
        }
        mime.to_string()
    } else if url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("/api/artifacts/")
    {
        if mime.starts_with("image/") || mime.starts_with("video/") || mime.starts_with("audio/") {
            // Canonicalise what the generator reported so the metadata agrees
            // with the Content-Type the artifact route will serve.
            MediaType::for_mime(mime)
                .map(|media| media.mime.to_string())
                .unwrap_or_else(|| mime.to_string())
        } else {
            MediaType::for_filename(url)
                .unwrap_or(MediaType::PNG)
                .mime
                .to_string()
        }
    } else {
        return None;
    };

    let (prefix, fallback) = if mime.starts_with("video/") {
        ("generated-video", MediaType::WEBM)
    } else if mime.starts_with("audio/") {
        ("generated-audio", MediaType::FLAC)
    } else {
        ("generated-image", MediaType::PNG)
    };
    let extension = MediaType::for_mime(&mime).unwrap_or(fallback).extension;

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

/// The key the console reads to show that a reply was written with stored
/// memory in front of the model. Set from the turn's own tool calls, never
/// from anything the model claims.
const MEMORY_USED_KEY: &str = "memory_used";

/// The record and the frame a started call leaves in the trace.
///
/// Both are workspace-readable — the record rides the assistant message's
/// metadata and the frame goes to every reader with the chat open — so a
/// memory tool's arguments and stated reason are reduced here, before either
/// exists. Lifted out of the loop so the trace a call leaves can be tested
/// with the arguments a real call carries.
fn trace_started(
    message_id: Uuid,
    id: String,
    name: String,
    arguments: &str,
    at: usize,
    thought: Option<Thought>,
) -> (ToolCallRecord, ServerMessage) {
    let (reasoning, reasoning_at) = thought.map_or((None, None), |thought| {
        (Some(thought.text), Some(thought.at))
    });
    let reason = crate::agent::memory::trace_reason(&name, crate::agent::reason(arguments));
    let shown = crate::agent::memory::trace_arguments(&name, arguments)
        .unwrap_or_else(|| arguments.to_string());
    let record = ToolCallRecord {
        id: id.clone(),
        name: name.clone(),
        arguments: shown.clone(),
        success: false,
        detail: UNFINISHED_DETAIL.to_string(),
        duration_ms: 0,
        at: Some(at),
        reasoning: reasoning.clone(),
        reasoning_at,
        reason: reason.clone(),
        preview: None,
        questions: Vec::new(),
        job: None,
        waiting: None,
        refusal: None,
    };
    let frame = ServerMessage::ToolCall {
        message_id,
        tool_call_id: id,
        name,
        arguments: shown,
        at,
        reasoning,
        reasoning_at,
        reason,
    };
    (record, frame)
}

/// Fold the tool trace, citations, write receipts and the logins the answer
/// moved across into the image metadata, since one turn can produce all of
/// them and they share the message's single metadata column. `usage`, the
/// tokens every login the turn ran on spent together, rides only with a move.
fn merge_metadata(
    images: Option<serde_json::Value>,
    tool_calls: &[ToolCallRecord],
    citations: &[Citation],
    receipts: &[ActionReceipt],
    reasoning: Option<&Thinking>,
    handovers: &[Notice],
    usage: Option<Usage>,
) -> Option<serde_json::Value> {
    let reasoning = reasoning.filter(|thinking| !thinking.text.is_empty());
    if tool_calls.is_empty()
        && citations.is_empty()
        && receipts.is_empty()
        && reasoning.is_none()
        && handovers.is_empty()
    {
        return images;
    }

    let mut object = match images {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    if !tool_calls.is_empty() {
        object.insert("tool_calls".to_string(), serde_json::json!(tool_calls));
    }
    if tool_calls
        .iter()
        .any(|call| call.success && call.name == crate::agent::memory::MEMORY_READ)
    {
        object.insert(MEMORY_USED_KEY.to_string(), serde_json::json!(true));
    }
    if !citations.is_empty() {
        object.insert("citations".to_string(), serde_json::json!(citations));
    }
    if !receipts.is_empty() {
        object.insert("action_receipts".to_string(), serde_json::json!(receipts));
    }
    if let Some(reasoning) = reasoning {
        object.insert("reasoning".to_string(), serde_json::json!(reasoning.text));
        object.insert("reasoning_at".to_string(), serde_json::json!(reasoning.at));
    }
    if !handovers.is_empty() {
        object.insert(HANDOVERS_KEY.to_string(), serde_json::json!(handovers));
        if let Some(usage) = usage {
            object.insert(USAGE_KEY.to_string(), serde_json::json!(usage));
        }
    }
    Some(serde_json::Value::Object(object))
}

/// Where an answer that moved across logins keeps each move, as its `handover` frame said it.
const HANDOVERS_KEY: &str = "handovers";

/// Where an answer that moved across logins keeps the tokens they spent on it together.
const USAGE_KEY: &str = "usage";

const LIVE_SNAPSHOT_INTERVAL: Duration = Duration::from_millis(300);

const MEDIA_HEARTBEAT: Duration = Duration::from_secs(30);

const MEDIA_WORKING: &str = "Working...";

const MEDIA_QUEUED: &str = "Waiting for another generation...";

/// `placeholder` in place of whatever follows the `told` bytes Zone itself opened an answer with,
/// when the model wrote nothing there.
fn fill_silence(content: &mut String, told: usize, placeholder: &str) -> bool {
    let silent = content[told..].trim().is_empty();
    if silent {
        content.truncate(told);
        content.push_str(placeholder);
    }
    silent
}

/// Keeps every offset inside the `kept` characters a placeholder was written after.
fn clamp_offsets(
    kept: usize,
    tool_calls: &mut [ToolCallRecord],
    handovers: &mut [Notice],
    thinking: &mut Thinking,
) {
    for call in tool_calls {
        call.at = call.at.map(|at| at.min(kept));
        call.reasoning_at = call.reasoning_at.map(|at| at.min(kept));
    }
    for notice in handovers {
        notice.at = notice.at.min(kept);
    }
    thinking.at = thinking.at.min(kept);
}

/// Model thinking not yet attached to a call, and where in the answer it began.
#[derive(Debug, Default)]
struct Thinking {
    text: String,
    at: usize,
}

/// Thinking handed to the call it preceded.
struct Thought {
    text: String,
    at: usize,
}

impl Thinking {
    fn push(&mut self, content: &str, at: impl FnOnce() -> usize) {
        if self.text.is_empty() {
            self.at = at();
        }
        self.text.push_str(content);
    }

    fn take(&mut self) -> Option<Thought> {
        let text = std::mem::take(&mut self.text);
        (!text.is_empty()).then_some(Thought { text, at: self.at })
    }
}

/// What a turn that produced neither prose nor an image is stored as.
const STOPPED_BEFORE_ANSWERING: &str = "[Stopped before answering]";

/// The same, for a turn that ended because the model asked something: it is
/// waiting on the reader rather than stopped.
const AWAITING_ANSWER: &str = "[Waiting for your answer]";

/// The same again, for a turn whose wait was cut short by cancellation, a lost
/// lease or the turn's own deadline. It stopped waiting rather than gave up,
/// and the two read very differently to whoever comes back to the chat.
const STOPPED_AFTER_WAITING: &str = "[Stopped while waiting]";

/// The outcome shown against the question call in the tool trace. Matches the
/// string the console writes on the live frame, so a reload does not relabel
/// the same call.
const AWAITING_ANSWER_DETAIL: &str = "Waiting for your answer…";

/// The outcome a call is stored with until its result replaces it, so it is
/// what a call the turn never finished is left showing. The console writes the
/// same string onto a call a stopped turn left open.
const UNFINISHED_DETAIL: &str = "Did not finish";

/// The outcome a call is stored with while it waits for the reader to allow it.
const AWAITING_APPROVAL_DETAIL: &str = "Waiting for approval…";

/// Leaves every call that was put to the reader and has no result when its turn ends as one the
/// turn never finished, whether it was still waiting or had been allowed and was running.
fn leave_unfinished(tool_calls: &mut [ToolCallRecord]) {
    for call in tool_calls
        .iter_mut()
        .filter(|call| call.detail == AWAITING_APPROVAL_DETAIL)
    {
        call.detail = UNFINISHED_DETAIL.to_string();
    }
}

/// Another writer took the chat's lease, so this turn may no longer persist.
const OWNERSHIP_LOST: &str = "Chat generation ownership was lost";

/// The turn's own wall clock ran out, wherever it was spent.
const GENERATION_TIMED_OUT: &str = "Response generation timed out";

/// What a turn that produced neither prose nor an image is stored as.
///
/// A question outranks a wait: a turn that waited and then asked is waiting on
/// the reader, which is the one of the two they can do something about.
const fn silent_turn(asked: bool, waited: bool) -> &'static str {
    if asked {
        AWAITING_ANSWER
    } else if waited {
        STOPPED_AFTER_WAITING
    } else {
        STOPPED_BEFORE_ANSWERING
    }
}

/// The job a canonical tool result announced, and the call that started it.
///
/// A tool reaches a frame through its result text and nothing else: the
/// registry holds no pid, `ToolResult` has no detail slot, and the completion
/// event's `detail` is a truncated first line. Reading the line back is
/// `abnegate_agent`'s, which owns the format and checks what it read by rebuilding
/// the receipt from it; what is left here is which messages may carry one.
fn spawned_job(message: &ReplayMessage) -> Option<(String, JobStarted)> {
    if message.role != LlmRole::Tool {
        return None;
    }
    let call = message.tool_call_id.clone()?;
    let job = parse_receipt(message.content.as_deref()?)?;
    Some((call, job))
}

/// Where a job has got to, or nothing once the registry has let it go.
async fn job_state(chat_id: Uuid, id: &str) -> Option<JobStatus> {
    Jobs::read(ToolSession::Chat(chat_id), id, 0, 0)
        .await
        .ok()
        .map(|tail| tail.state)
}

/// How a job ended, as the exit field the console reads. No code means killed.
fn job_exit(id: &str, state: Option<JobStatus>) -> job::JobExit {
    job::JobExit {
        id: id.to_string(),
        exit_code: match state {
            Some(JobStatus::Exited(code)) => Some(code),
            _ => None,
        },
    }
}

/// The exits the turn reports for the jobs it still tracks at teardown.
///
/// A job still running here is about to be killed with the turn, so the state
/// is read before anything kills it: that is what tells a job that finished
/// apart from one the turn took with it.
async fn job_exits(chat_id: Uuid, tracked: &[String]) -> Vec<job::JobExit> {
    let mut exits = Vec::with_capacity(tracked.len());
    for id in tracked {
        exits.push(job_exit(id, job_state(chat_id, id).await));
    }
    exits
}

/// A job wait's end, reported only once the job itself reached one.
///
/// A settled wait is not a settled job: [`wait::await_outcome`]'s deadline arm
/// hands its timeout back as an ordinary outcome, and a lost wait reads the
/// same way. Announcing an exit for either would draw a kill that never
/// happened, and taking the id out of `tracked` would silence the real end.
async fn job_wait_exit(chat_id: Uuid, id: &str, tracked: &mut Vec<String>) -> Option<job::JobExit> {
    let state = job_state(chat_id, id).await?;
    if !state.settled() {
        return None;
    }
    tracked.retain(|held| held != id);
    Some(job_exit(id, Some(state)))
}

/// How a wait the model opened came to an end.
#[derive(Debug, PartialEq)]
enum WaitEnd {
    Settled(Outcome),
    Cancelled,
    Lost,
    Expired,
}

/// The first of the four things that can end a wait.
///
/// Cancellation and the lease guard are passed in rather than awaited inside
/// the turn's own event arm, where neither would ever be reached: those arms
/// are biased and only re-read when the select is re-entered. It matters most
/// to the turn a disconnected socket leaves running, where a reconnected
/// client's cancel is the only thing that can stop the wait before the turn's
/// own clock does.
///
/// Polled in the order written: an outcome landing in the same tick as the
/// deadline has happened, and reporting it as a timeout would tell the model
/// nothing had.
async fn settle_wait(
    outcome: impl Future<Output = Outcome>,
    lost: impl Future<Output = ()>,
    cancel: &mut broadcast::Receiver<()>,
    stream_deadline: tokio::time::Instant,
) -> WaitEnd {
    tokio::select! {
        biased;
        outcome = outcome => WaitEnd::Settled(outcome),
        () = lost => WaitEnd::Lost,
        _ = cancel.recv() => WaitEnd::Cancelled,
        _ = tokio::time::sleep_until(stream_deadline) => WaitEnd::Expired,
    }
}

async fn publish_live_assistant(
    session: &session::Session,
    content: &str,
    tool_calls: &[ToolCallRecord],
    citations: &[Citation],
    receipts: &[ActionReceipt],
    images: &[ChatImageAttachment],
    reasoning: &Thinking,
    handovers: &[Notice],
    usage: Option<Usage>,
) -> Result<(), String> {
    if content.is_empty()
        && tool_calls.is_empty()
        && citations.is_empty()
        && receipts.is_empty()
        && images.is_empty()
        && reasoning.text.is_empty()
        && handovers.is_empty()
    {
        return Ok(());
    }
    session
        .store
        .publish(
            &session.lease,
            session.turn,
            content,
            merge_metadata(
                image_metadata(images),
                tool_calls,
                citations,
                receipts,
                Some(reasoning),
                handovers,
                usage,
            ),
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
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
            crate::metrics::record_ws_chat("rejected", "too_many");
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
    let access = match tokio::time::timeout(
        Duration::from_secs(WS_AUTH_TIMEOUT_SECS),
        receiver.next(),
    )
    .await
    {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::Auth { token }) => {
                match validate_access_token(&token, state.config().jwt_secret()) {
                    Ok(access) => access,
                    Err(e) => {
                        crate::metrics::record_ws_chat("rejected", "auth_failed");
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
                crate::metrics::record_ws_chat("rejected", "bad_format");
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
            crate::metrics::record_ws_chat("rejected", "timeout");
            let error_msg = ServerMessage::Error {
                message: "Authentication timeout or error".to_string(),
            };
            let _ = sender.send(error_msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
    };

    // Get user ID from claims
    let user_id = match access.claims.user_id() {
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
    let Some(session_id) = access.session_id else {
        let error_msg = ServerMessage::Error {
            message: "Authentication failed".to_string(),
        };
        let _ = sender.send(error_msg.to_ws_message()).await;
        let _ = sender.close().await;
        return;
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
    match can_access_chat(&state, user_id, session_id, workspace_id).await {
        Ok(true) => {
            tracing::info!(
                "User {} connected to chat {} in workspace {}",
                user_id,
                chat_id,
                workspace_id
            );
        }
        Ok(false) => {
            crate::metrics::record_ws_chat("rejected", "access_denied");
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

    // Subscribe before announcing the connection, so that `init` tells the
    // client every later frame reaches it, and catch it up on the turn in
    // flight so a reload or a dropped socket rejoins the reply mid-sentence.
    let mut titles = crate::workers::titles::subscribe();
    let mut actions = crate::db::actions::subscribe();
    let stream = ChatStream::of(chat_id);
    let (resume, mut events) = stream.join().await;

    let init_msg = ServerMessage::Init {
        chat_id,
        status: STATUS_CONNECTED.to_string(),
    };
    if sender.send(init_msg.to_ws_message()).await.is_err() {
        return;
    }
    let _ws_active = crate::metrics::WsActiveGuard::acquire();
    let sender = Arc::new(Mutex::new(sender));
    if !forward(&sender, resume).await {
        return;
    }

    // Setup state for message loop
    let mut consecutive_errors = 0;
    let mut last_client_activity = Instant::now();
    let mut auth_interval = tokio::time::interval(AUTH_RECHECK_INTERVAL);
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
                    if !can_access_chat(&state, user_id, session_id, workspace_id)
                        .await
                        .unwrap_or(false)
                    {
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
            frame = events.recv() => {
                match frame {
                    Ok(frame) => {
                        if !send_server(&sender, frame).await {
                            break;
                        }
                    }
                    // Too far behind to apply the frames it missed, so take
                    // the turn from the top instead of rendering a gap.
                    Err(broadcast::error::RecvError::Lagged(frames)) => {
                        tracing::warn!(%chat_id, frames, "Chat connection fell behind, replaying");
                        let (mut resume, receiver) = stream.join().await;
                        events = receiver;
                        // The queue changes it missed may have emptied it, which
                        // a join alone says nothing about.
                        if !resume.iter().any(|frame| matches!(frame, ServerMessage::Queue { .. })) {
                            resume.push(ServerMessage::Queue {
                                messages: Vec::new(),
                                held: false,
                                started: None,
                            });
                        }
                        if !forward(&sender, resume).await {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
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
                            Ok(ClientMessage::Send { id, content, metadata }) => {
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

                                if let Some(key) = authorship::claimed(metadata.as_ref()) {
                                    let refusal = ServerMessage::Error {
                                        message: authorship::refusal(key),
                                    };
                                    let _ = send_server(&sender, refusal).await;
                                    continue;
                                }

                                let message = Queued { id, content, metadata, author: user_id };
                                let started = queue::with(chat_id, |queue| {
                                    queue
                                        .submit(message, generating(chat_id))
                                        .map(|started| begin(chat_id, started))
                                });
                                match started {
                                    // A message that starts at once never showed in the
                                    // queue, so there is no change for anyone to hear.
                                    Some(started) => {
                                        let told = started.0.id != id;
                                        drive(state.clone(), stream.clone(), chat_id, workspace_id, started, told);
                                    }
                                    None => announce(&stream, None).await,
                                }
                            }
                            Ok(ClientMessage::Cancel) => {
                                queue::with(chat_id, |queue| {
                                    queue.stop(generating(chat_id));
                                    cancel_turns(chat_id);
                                });
                            }
                            Ok(ClientMessage::Interrupt { id }) => {
                                let started = queue::with(chat_id, |queue| {
                                    match queue.interrupt(id, generating(chat_id)) {
                                        Interrupted::Cancel => {
                                            cancel_turns(chat_id);
                                            None
                                        }
                                        Interrupted::Start(started) => Some(begin(chat_id, started)),
                                        Interrupted::Missing => None,
                                    }
                                });
                                match started {
                                    Some(started) => {
                                        drive(state.clone(), stream.clone(), chat_id, workspace_id, started, true);
                                    }
                                    None => announce(&stream, None).await,
                                }
                            }
                            Ok(ClientMessage::EditQueued { id, content }) => {
                                if content.len() > MAX_MESSAGE_LENGTH {
                                    let error_msg = ServerMessage::Error {
                                        message: "Message too long".to_string(),
                                    };
                                    let _ = send_server(&sender, error_msg).await;
                                    continue;
                                }
                                queue::with(chat_id, |queue| {
                                    if content.trim().is_empty() {
                                        queue.remove(id)
                                    } else {
                                        queue.edit(id, content, user_id)
                                    }
                                });
                                announce(&stream, None).await;
                            }
                            Ok(ClientMessage::RemoveQueued { id }) => {
                                queue::with(chat_id, |queue| queue.remove(id));
                                announce(&stream, None).await;
                            }
                            Ok(ClientMessage::ApproveTool {
                                tool_call_id,
                                approved,
                            }) => {
                                let decided = crate::agent::ApprovalPolicy::decide_chat(
                                    chat_id,
                                    &tool_call_id,
                                    approved,
                                );
                                if !decided {
                                    let _ = send_server(
                                        &sender,
                                        ServerMessage::ToolApprovalClosed { tool_call_id },
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

            _ = auth_interval.tick() => {
                match can_access_chat(&state, user_id, session_id, workspace_id).await {
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
                        tracing::error!("Error re-checking chat authorization: {}", e);
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

            // Periodic ping and idle timeout check
            _ = ping_interval.tick() => {
                if last_client_activity.elapsed() > Duration::from_secs(WS_IDLE_TIMEOUT_SECS) {
                    tracing::info!("Closing idle WebSocket connection for chat {}", chat_id);
                    let _ = sender.lock().await.close().await;
                    return;
                }

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
) -> Result<Option<abnegate_comfy::SourceImage>, crate::services::media_source::Error> {
    use crate::services::media_source::{
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

fn generation_deadline(
    timeout: Duration,
) -> Result<tokio::time::Instant, Box<dyn std::error::Error + Send + Sync>> {
    tokio::time::Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| "Chat generation deadline is not representable".into())
}

async fn watch_lease(
    store: &crate::db::context::Store,
    lease: &crate::db::context::Lease,
) -> crate::db::context::Error {
    let mut interval = tokio::time::interval(Duration::from_millis(200));
    interval.tick().await;
    loop {
        interval.tick().await;
        if let Err(error) = store.assert_current(lease).await {
            return error;
        }
    }
}

/// One media job's progress, relayed to the chat from the moment it queues for
/// the media slot until its outcome is published.
struct MediaProgress {
    sender: mpsc::UnboundedSender<String>,
    relay: Option<tokio::task::JoinHandle<()>>,
}

impl MediaProgress {
    fn start(stream: &Arc<ChatStream>) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            sender,
            relay: Some(tokio::spawn(relay_progress(stream.clone(), receiver))),
        }
    }

    fn report(&self, message: &str) {
        let _ = self.sender.send(message.to_string());
    }

    fn sender(&self) -> mpsc::UnboundedSender<String> {
        self.sender.clone()
    }

    /// Stop relaying, so no status can follow the frame that settles the job.
    async fn stop(&mut self) {
        if let Some(relay) = self.relay.take() {
            relay.abort();
            let _ = relay.await;
        }
    }
}

impl Drop for MediaProgress {
    fn drop(&mut self) {
        if let Some(relay) = &self.relay {
            relay.abort();
        }
    }
}

async fn queue_media<'slots>(
    slots: &'slots Semaphore,
    progress: &MediaProgress,
) -> tokio::sync::SemaphorePermit<'slots> {
    if let Ok(permit) = slots.try_acquire() {
        return permit;
    }
    progress.report(MEDIA_QUEUED);
    slots
        .acquire()
        .await
        .expect("media semaphore is never closed")
}

async fn wait_media(
    generation: &mut Generation,
    session: &mut Session,
    stream: &ChatStream,
    progress: &mut MediaProgress,
) -> Result<Option<tokio::sync::SemaphorePermit<'static>>, Box<dyn std::error::Error + Send + Sync>>
{
    let waited: Result<_, Box<dyn std::error::Error + Send + Sync>> = tokio::select! {
        biased;
        _ = session.guard.lost() => Err(OWNERSHIP_LOST.into()),
        error = watch_lease(&session.store, &session.lease) => Err(error.into()),
        _ = generation.cancel.recv() => Ok(None),
        permit = queue_media(&IMAGE_GENERATIONS, progress) => Ok(Some(permit)),
    };
    let permit = match waited {
        Ok(Some(permit)) => permit,
        Ok(None) => {
            progress.stop().await;
            session.close().await?;
            publish(
                stream,
                ServerMessage::Cancelled {
                    message_id: Some(generation.message_id),
                },
            )
            .await;
            return Ok(None);
        }
        Err(error) => {
            progress.stop().await;
            return Err(error);
        }
    };
    if let Err(error) = session.store.assert_current(&session.lease).await {
        progress.stop().await;
        return Err(error.into());
    }
    Ok(Some(permit))
}

async fn await_media<T>(
    lost: impl Future<Output = ()>,
    cancellation: broadcast::Sender<()>,
    operation: impl Future<Output = T>,
    progress: &mut MediaProgress,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>> {
    tokio::pin!(lost);
    tokio::pin!(operation);
    tokio::select! {
        biased;
        _ = &mut lost => {
            let _ = cancellation.send(());
            let _ = operation.await;
            progress.stop().await;
            Err(OWNERSHIP_LOST.into())
        }
        result = &mut operation => Ok(result),
    }
}

async fn relay_progress(stream: Arc<ChatStream>, mut progress: mpsc::UnboundedReceiver<String>) {
    let started = tokio::time::Instant::now();
    let mut latest = MEDIA_WORKING.to_string();
    let mut heartbeat = tokio::time::interval_at(started + MEDIA_HEARTBEAT, MEDIA_HEARTBEAT);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            message = progress.recv() => {
                let Some(message) = message else {
                    return;
                };
                latest.clone_from(&message);
                heartbeat.reset();
                publish(&stream, ServerMessage::Status { message }).await;
            }
            _ = heartbeat.tick() => {
                let message = format!("{latest} {} elapsed", elapsed_label(started.elapsed()));
                publish(&stream, ServerMessage::Status { message }).await;
            }
        }
    }
}

fn elapsed_label(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{seconds} s")
    } else {
        format!("{} min", seconds / 60)
    }
}

/// How a finished ComfyUI job names itself in the message it saves and in the
/// errors it reports, so image, video, and upscale jobs deliver the same way.
#[derive(Clone, Copy)]
struct Delivery {
    /// Opens every failure: "Image generation", "Audio generation", "Upscaling".
    subject: &'static str,
    /// What the job produced, as the failures refer to it: "image" or "video".
    noun: &'static str,
    /// The assistant message body once the media is stored.
    content: &'static str,
    /// Stored instead when ComfyUI names a format this lane does not emit.
    fallback: MediaType,
}

/// What a generated file is stored as. Taking the extension and the announced
/// media type from one entry keeps an artifact URL and its attachment in step,
/// and a format outside the job's own lane falls back rather than mislabelling
/// the file.
fn stored_media(mime: &str, fallback: MediaType) -> MediaType {
    MediaType::for_mime(mime)
        .filter(|media| {
            media.is_audio() == fallback.is_audio() && media.is_video() == fallback.is_video()
        })
        .unwrap_or(fallback)
}

fn comfy_failure(subject: &str, error: &abnegate_comfy::Error) -> String {
    match error {
        abnegate_comfy::Error::Http(error) if error.is_connect() => format!(
            "{subject} failed: cannot reach ComfyUI. Start the image service and try again."
        ),
        abnegate_comfy::Error::Http(error) if error.is_timeout() => format!(
            "{subject} failed: ComfyUI did not respond in time. Check the image service and try again."
        ),
        _ => format!("{subject} failed: {error}"),
    }
}

/// Store what ComfyUI returned, save the assistant message, and announce both.
async fn deliver_media(
    stream: &Arc<ChatStream>,
    chat_id: Uuid,
    workspace_id: Uuid,
    media: Vec<abnegate_comfy::GeneratedImage>,
    store: &crate::services::artifacts::ArtifactStore,
    delivery: Delivery,
    generation: &Generation,
    session: &mut Session,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let Delivery {
        subject,
        noun,
        content,
        fallback,
    } = delivery;
    let assistant_message_id = generation.message_id;

    let mut attachments = Vec::new();
    let mut oversize = false;
    for item in media.into_iter().take(MAX_GENERATED_IMAGES) {
        if item.bytes.len() > MAX_ARTIFACT_BYTES {
            oversize = true;
            tracing::warn!("ComfyUI {noun} output exceeded artifact size limit");
            continue;
        }
        let media = stored_media(&item.mime, fallback);
        let url = match store
            .persist(
                workspace_id,
                chat_id,
                assistant_message_id,
                media.extension,
                &item.bytes,
            )
            .await
        {
            Ok(url) => url,
            Err(error) => {
                tracing::error!("Failed to persist generated {noun}: {error}");
                store
                    .cleanup_owner(workspace_id, chat_id, assistant_message_id)
                    .await;
                session.close().await?;
                publish(
                    stream,
                    ServerMessage::Error {
                        message: format!("{subject} failed: could not store the {noun}"),
                    },
                )
                .await;
                return Ok(());
            }
        };
        if let Some(attachment) = generated_media_attachment(&url, media.mime, attachments.len()) {
            attachments.push(attachment);
        }
    }
    if attachments.is_empty() {
        session.close().await?;
        let message = if oversize {
            format!("{subject} finished, but the {noun} is too large to store")
        } else {
            format!("{subject} produced no usable {noun}")
        };
        publish(stream, ServerMessage::Error { message }).await;
        return Ok(());
    }

    let metadata = image_metadata(&attachments);
    let mut replay = LlmMessage::assistant(content);
    replay.images = session::images(metadata.as_ref());
    if let Err(error) = session
        .store
        .finish(
            &session.lease,
            session.turn,
            content,
            metadata.clone(),
            false,
            Some(&ReplayMessage::from(&replay)),
        )
        .await
    {
        tracing::error!("Failed to persist generated {noun} message: {error}");
        store
            .cleanup_owner(workspace_id, chat_id, assistant_message_id)
            .await;
        session.close().await?;
        publish(
            stream,
            ServerMessage::Error {
                message: format!("{subject} failed: could not save the message"),
            },
        )
        .await;
        return Ok(());
    }
    publish(
        stream,
        ServerMessage::MessageStart {
            message_id: assistant_message_id,
            role: "assistant".to_string(),
            resumed: false,
        },
    )
    .await;
    for attachment in &attachments {
        let message_id = assistant_message_id;
        let attachment = attachment.clone();
        let frame = match MediaType::for_mime(&attachment.mime) {
            Some(media) if media.is_audio() => ServerMessage::Audio {
                message_id,
                attachment,
            },
            Some(media) if media.is_video() => ServerMessage::Video {
                message_id,
                attachment,
            },
            _ => ServerMessage::Image {
                message_id,
                attachment,
            },
        };
        publish(stream, frame).await;
    }
    session.close().await?;
    publish(
        stream,
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

async fn handle_image_generation(
    state: &AppState,
    stream: &Arc<ChatStream>,
    chat_id: Uuid,
    workspace_id: Uuid,
    prompt: &str,
    metadata: Option<&serde_json::Value>,
    mut image_config: crate::config::ComfyUiConfig,
    backend: LlmBackend,
    endpoint: Endpoint,
    generation: &mut Generation,
    session: &mut Session,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::services::artifacts::ArtifactStore;
    use abnegate_comfy::{Client as ComfyUiClient, Error as ComfyUiError};

    let assistant_message_id = generation.message_id;

    let identity = abnegate_comfy::inventory::bind_identity(&mut image_config, prompt);
    let client = match ComfyUiClient::new(image_config.clone()) {
        Ok(client) => client,
        Err(error) => {
            session.close().await?;
            publish(
                stream,
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
                session.close().await?;
                publish(
                    stream,
                    ServerMessage::Error {
                        message: format!("Image generation failed: {error}"),
                    },
                )
                .await;
                return Ok(());
            }
        };
    publish(
        stream,
        ServerMessage::Status {
            message: if source.is_some() {
                "Preparing image-to-image...".to_string()
            } else {
                "Preparing image generation...".to_string()
            },
        },
    )
    .await;
    let generation_prompt = if source.is_some()
        && client.prompt_mode() != abnegate_comfy::recipe::PromptMode::EditInstruction
    {
        crate::services::image_intent::ImageIntentClassifier::new(
            image_config.clone(),
            endpoint,
            backend,
        )
        .edit_prompt(prompt)
        .await
    } else {
        prompt.to_string()
    };
    let generation_prompt = match identity.as_ref() {
        Some(identity) => {
            abnegate_comfy::lora::identity_caption(&generation_prompt, &identity.trigger)
        }
        None => generation_prompt,
    };
    let mut progress = MediaProgress::start(stream);
    let Some(_generation_permit) = wait_media(generation, session, stream, &mut progress).await?
    else {
        return Ok(());
    };
    let cancellation = generation.cancellation();
    let result = await_media(
        session.guard.lost(),
        cancellation,
        client.generate(
            &generation_prompt,
            source.as_ref(),
            &mut generation.cancel,
            progress.sender(),
        ),
        &mut progress,
    )
    .await?;
    progress.stop().await;
    if result.is_ok() && generation.cancel.try_recv().is_ok() {
        session.close().await?;
        generation.cancelled(stream).await;
        return Ok(());
    }

    let images = match result {
        Ok(images) => images,
        Err(ComfyUiError::Cancelled) => {
            session.close().await?;
            publish(
                stream,
                ServerMessage::Cancelled {
                    message_id: generation.started.then_some(assistant_message_id),
                },
            )
            .await;
            return Ok(());
        }
        Err(error) => {
            let message = comfy_failure("Image generation", &error);
            session.close().await?;
            publish(stream, ServerMessage::Error { message }).await;
            return Ok(());
        }
    };

    deliver_media(
        stream,
        chat_id,
        workspace_id,
        images,
        &store,
        Delivery {
            subject: "Image generation",
            noun: "image",
            content: "Generated image.",
            fallback: MediaType::PNG,
        },
        generation,
        session,
    )
    .await
}

async fn handle_video_generation(
    state: &AppState,
    stream: &Arc<ChatStream>,
    chat_id: Uuid,
    workspace_id: Uuid,
    prompt: &str,
    metadata: Option<&serde_json::Value>,
    video_config: crate::config::ComfyUiConfig,
    generation: &mut Generation,
    session: &mut Session,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::services::artifacts::ArtifactStore;
    use abnegate_comfy::{Client as ComfyUiClient, Error as ComfyUiError};

    let assistant_message_id = generation.message_id;

    let client = match ComfyUiClient::new(video_config.clone()) {
        Ok(client) => client,
        Err(error) => {
            session.close().await?;
            publish(
                stream,
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
                session.close().await?;
                publish(
                    stream,
                    ServerMessage::Error {
                        message: format!("Video generation failed: {error}"),
                    },
                )
                .await;
                return Ok(());
            }
        };
    publish(
        stream,
        ServerMessage::Status {
            message: if source.is_some() {
                "Preparing image-to-video...".to_string()
            } else {
                "Preparing video generation...".to_string()
            },
        },
    )
    .await;
    let mut progress = MediaProgress::start(stream);
    let Some(_generation_permit) = wait_media(generation, session, stream, &mut progress).await?
    else {
        return Ok(());
    };
    let cancellation = generation.cancellation();
    let result = await_media(
        session.guard.lost(),
        cancellation,
        client.generate_video(
            prompt,
            source.as_ref(),
            &mut generation.cancel,
            progress.sender(),
        ),
        &mut progress,
    )
    .await?;
    progress.stop().await;
    if result.is_ok() && generation.cancel.try_recv().is_ok() {
        session.close().await?;
        generation.cancelled(stream).await;
        return Ok(());
    }

    let videos = match result {
        Ok(videos) => videos,
        Err(ComfyUiError::Cancelled) => {
            session.close().await?;
            publish(
                stream,
                ServerMessage::Cancelled {
                    message_id: generation.started.then_some(assistant_message_id),
                },
            )
            .await;
            return Ok(());
        }
        Err(error) => {
            let message = comfy_failure("Video generation", &error);
            session.close().await?;
            publish(stream, ServerMessage::Error { message }).await;
            return Ok(());
        }
    };

    deliver_media(
        stream,
        chat_id,
        workspace_id,
        videos,
        &store,
        Delivery {
            subject: "Video generation",
            noun: "video",
            content: "Generated video.",
            fallback: MediaType::WEBM,
        },
        generation,
        session,
    )
    .await
}

async fn handle_upscale(
    state: &AppState,
    stream: &Arc<ChatStream>,
    chat_id: Uuid,
    workspace_id: Uuid,
    prompt: &str,
    metadata: Option<&serde_json::Value>,
    upscale_config: crate::config::ComfyUiConfig,
    generation: &mut Generation,
    session: &mut Session,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::services::artifacts::ArtifactStore;
    use crate::services::media_source::{Kind, Source};
    use abnegate_comfy::{Client as ComfyUiClient, Error as ComfyUiError};

    const SUBJECT: &str = "Upscaling";
    let assistant_message_id = generation.message_id;

    let client = match ComfyUiClient::new(upscale_config.clone()) {
        Ok(client) => client,
        Err(error) => {
            session.close().await?;
            publish(
                stream,
                ServerMessage::Error {
                    message: format!("Upscaling is not configured: {error}"),
                },
            )
            .await;
            return Ok(());
        }
    };
    let store = ArtifactStore::new(upscale_config.artifact_root.clone());
    let source = match resolve_upscale_source(
        state,
        chat_id,
        workspace_id,
        prompt,
        metadata,
        &store,
    )
    .await
    {
        Ok(Some(source)) => source,
        Ok(None) => {
            session.close().await?;
            publish(
                stream,
                ServerMessage::Error {
                    message: "Upscaling needs an image or video: attach one or generate one first"
                        .to_string(),
                },
            )
            .await;
            return Ok(());
        }
        Err(error) => {
            session.close().await?;
            publish(
                stream,
                ServerMessage::Error {
                    message: format!("{SUBJECT} failed: {error}"),
                },
            )
            .await;
            return Ok(());
        }
    };
    let noun = match source.kind() {
        Kind::Image => "image",
        Kind::Video => "video",
    };
    publish(
        stream,
        ServerMessage::Status {
            message: format!("Preparing to upscale the {noun}..."),
        },
    )
    .await;
    let mut progress = MediaProgress::start(stream);
    let Some(_generation_permit) = wait_media(generation, session, stream, &mut progress).await?
    else {
        return Ok(());
    };
    let cancellation = generation.cancellation();
    let sender = progress.sender();
    let result = await_media(
        session.guard.lost(),
        cancellation,
        async {
            match &source {
                Source::Image(image) => {
                    client
                        .upscale_image(image, &mut generation.cancel, sender)
                        .await
                }
                Source::Video(video) => {
                    client
                        .upscale_video(video, &mut generation.cancel, sender)
                        .await
                }
            }
        },
        &mut progress,
    )
    .await?;
    progress.stop().await;
    if result.is_ok() && generation.cancel.try_recv().is_ok() {
        session.close().await?;
        generation.cancelled(stream).await;
        return Ok(());
    }

    let media = match result {
        Ok(media) => media,
        Err(ComfyUiError::Cancelled) => {
            session.close().await?;
            publish(
                stream,
                ServerMessage::Cancelled {
                    message_id: generation.started.then_some(assistant_message_id),
                },
            )
            .await;
            return Ok(());
        }
        Err(error) => {
            let message = comfy_failure(SUBJECT, &error);
            session.close().await?;
            publish(stream, ServerMessage::Error { message }).await;
            return Ok(());
        }
    };

    deliver_media(
        stream,
        chat_id,
        workspace_id,
        media,
        &store,
        Delivery {
            subject: SUBJECT,
            noun,
            content: match source.kind() {
                Kind::Image => "Upscaled image.",
                Kind::Video => "Upscaled video.",
            },
            fallback: match source.kind() {
                Kind::Image => MediaType::PNG,
                Kind::Video => MediaType::WEBM,
            },
        },
        generation,
        session,
    )
    .await
}

/// The media an upscale request acts on: this turn's attachment when it is the
/// kind the request named, otherwise the newest match on the thread. A turn
/// carrying a screenshot must not hide the clip "upscale the video" asked for.
async fn resolve_upscale_source(
    state: &AppState,
    chat_id: Uuid,
    workspace_id: Uuid,
    prompt: &str,
    metadata: Option<&serde_json::Value>,
    store: &crate::services::artifacts::ArtifactStore,
) -> Result<Option<crate::services::media_source::Source>, crate::services::media_source::Error> {
    use crate::services::media_source::{Error, resolve_source_media_from};

    let wanted = crate::services::image_intent::upscale_target(prompt);
    let attached = resolve_source_media_from(
        std::iter::once(metadata),
        workspace_id,
        chat_id,
        store,
        wanted,
    )
    .await?;
    if let Some(source) = attached
        && wanted.is_none_or(|kind| source.kind() == kind)
    {
        return Ok(Some(source));
    }
    let history = match chats::list_messages(state.db(), chat_id).await {
        Ok(messages) => messages,
        Err(error) => {
            tracing::warn!("Failed to load chat media for upscaling: {error}");
            return Err(Error::Unreadable);
        }
    };
    resolve_source_media_from(
        std::iter::once(metadata).chain(
            history
                .iter()
                .rev()
                .map(|message| message.metadata.as_ref()),
        ),
        workspace_id,
        chat_id,
        store,
        wanted,
    )
    .await
}

async fn handle_audio_generation(
    stream: &Arc<ChatStream>,
    chat_id: Uuid,
    workspace_id: Uuid,
    prompt: &str,
    audio_config: crate::config::ComfyUiConfig,
    generation: &mut Generation,
    session: &mut Session,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::services::artifacts::ArtifactStore;
    use abnegate_comfy::{Client as ComfyUiClient, Error as ComfyUiError};

    let assistant_message_id = generation.message_id;

    let client = match ComfyUiClient::new(audio_config.clone()) {
        Ok(client) => client,
        Err(error) => {
            session.close().await?;
            publish(
                stream,
                ServerMessage::Error {
                    message: format!("Audio generation is not configured: {error}"),
                },
            )
            .await;
            return Ok(());
        }
    };
    let store = ArtifactStore::new(audio_config.artifact_root.clone());
    publish(
        stream,
        ServerMessage::Status {
            message: "Preparing audio generation...".to_string(),
        },
    )
    .await;
    let mut progress = MediaProgress::start(stream);
    let Some(_generation_permit) = wait_media(generation, session, stream, &mut progress).await?
    else {
        return Ok(());
    };
    let cancellation = generation.cancellation();
    let result = await_media(
        session.guard.lost(),
        cancellation,
        client.generate_audio(prompt, &mut generation.cancel, progress.sender()),
        &mut progress,
    )
    .await?;
    progress.stop().await;
    if result.is_ok() && generation.cancel.try_recv().is_ok() {
        session.close().await?;
        generation.cancelled(stream).await;
        return Ok(());
    }

    let clips = match result {
        Ok(clips) => clips,
        Err(ComfyUiError::Cancelled) => {
            session.close().await?;
            publish(
                stream,
                ServerMessage::Cancelled {
                    message_id: generation.started.then_some(assistant_message_id),
                },
            )
            .await;
            return Ok(());
        }
        Err(error) => {
            let message = comfy_failure("Audio generation", &error);
            session.close().await?;
            publish(stream, ServerMessage::Error { message }).await;
            return Ok(());
        }
    };

    deliver_media(
        stream,
        chat_id,
        workspace_id,
        clips,
        &store,
        Delivery {
            subject: "Audio generation",
            noun: "audio",
            content: "Generated audio.",
            fallback: MediaType::FLAC,
        },
        generation,
        session,
    )
    .await
}

/// Handle a send message request
///
/// Chat requires write access (Member role or higher) since it creates messages.
/// This is intentionally stricter than context.rs which only requires membership.
/// `unattended` says nobody is watching this turn, which is what an automation's
/// firing is. It withholds the chat's auto-approve from that turn and nothing
/// else. Auto-approve is a standing "yes" a person gave while they were present
/// to see what they were saying yes to; a scheduled turn runs hours later with
/// nobody there, and its prompt can carry text the model itself wrote from
/// something it read, so a tool that writes to the host or leaves the workspace
/// must still be put to somebody. It waits, finds no one, and is declined —
/// which is the right answer when the alternative is acting unasked. Reading
/// tools are below that tier and run either way, so a watch still works.
/// Whether the user's message was saved: a queued turn stopped before then has
/// left nothing in the chat, so the queue puts it back.
async fn handle_send_message(
    state: &AppState,
    stream: &Arc<ChatStream>,
    chat_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    content: &str,
    metadata: Option<serde_json::Value>,
    mut request: Generation,
    unattended: bool,
) -> bool {
    let generation = CHAT_GENERATIONS
        .entry(chat_id)
        .or_insert_with(|| Arc::new(Semaphore::new(1)))
        .clone();
    let _permit = tokio::select! {
        biased;
        _ = request.cancel.recv() => {
            request.cancelled(stream).await;
            return false;
        }
        permit = generation.acquire() => permit.expect("chat semaphore is never closed"),
    };

    if !workspace_members::can_write(state.db(), workspace_id, user_id)
        .await
        .unwrap_or(false)
    {
        publish(
            stream,
            ServerMessage::Error {
                message: "Workspace access denied".to_string(),
            },
        )
        .await;
        return false;
    }
    let mut session = match Session::acquire(state, chat_id, workspace_id, request.message_id).await
    {
        Ok(session) => session,
        Err(error) => {
            publish(
                stream,
                ServerMessage::Error {
                    message: error.to_string(),
                },
            )
            .await;
            return false;
        }
    };
    if generation_deadline(state.config().chat.timeout).is_err() {
        let _ = session.close().await;
        publish(
            stream,
            ServerMessage::Error {
                message: "Chat generation deadline is not representable".into(),
            },
        )
        .await;
        return false;
    }
    crate::agent::ApprovalPolicy::register(chat_id, request.approvals.clone());
    let mut jobs: Vec<String> = Vec::new();
    let preparation = tokio::select! {
        biased;
        _ = session.guard.lost() => { let _=session.close().await; publish(stream,ServerMessage::Error {message:OWNERSHIP_LOST.into()}).await; return false; }
        _ = request.cancel.recv() => {
            let _=session.close().await;
                        request.cancelled(stream).await;
            return false;
        }
        result = prepare_message(state, chat_id, workspace_id, content, metadata.as_ref()) => result,
    };
    let mut saved = false;
    let result = async {
        let routing = preparation?;
        if request.is_cancelled() {
            let _=session.close().await;
                        request.cancelled(stream).await;
            return Ok(());
        }
        // Once persistence begins, finish the commit and acknowledgement
        // before honouring Stop. Dropping an INSERT future cannot roll it back.
        let mut message=LlmMessage::user(content);message.images=session::images(metadata.as_ref());
        let user_message=session.store.begin(&session.lease,session.turn,Uuid::new_v4(),content,metadata.clone(),ReplayMessage::from(&message)).await?;
        saved = true;
        crate::workers::titles::spawn(state.clone(),&user_message);
        spawn_message_embedding_task(state.clone(),user_message.id,chat_id,content.to_string());
        publish(stream,ServerMessage::MessageSaved {message_id:user_message.id,role:"user".into(),content:content.to_string(),metadata:metadata.clone()}).await;

        if request.is_cancelled() {
            let _=session.close().await;
                        request.cancelled(stream).await;
            return Ok(());
        }
        match routing {
            Routing::Image(config, backend, endpoint) => {
                handle_image_generation(
                    state,
                    stream,
                    chat_id,
                    workspace_id,
                    content,
                    metadata.as_ref(),
                    config,
                    backend,
                    endpoint,
                    &mut request,
                    &mut session,
                )
                .await
            }
            Routing::Video(config) => {
                handle_video_generation(
                    state,
                    stream,
                    chat_id,
                    workspace_id,
                    content,
                    metadata.as_ref(),
                    config,
                    &mut request,
                    &mut session,
                )
                .await
            }
            Routing::Audio(config) => {
                handle_audio_generation(
                    stream,
                    chat_id,
                    workspace_id,
                    content,
                    config,
                    &mut request,
                    &mut session,
                )
                .await
            }
            Routing::Upscale(config) => {
                handle_upscale(
                    state,
                    stream,
                    chat_id,
                    workspace_id,
                    content,
                    metadata.as_ref(),
                    config,
                    &mut request,
                    &mut session,
                )
                .await
            }
            Routing::Chat(mut chat, route) => {
                // Cleared before the prompt is built rather than after it.
                // `prepare_chat` renders the approval rules from this flag, so
                // setting it on the preparation instead would gate the tools
                // while telling the model they were not gated -- and a
                // confirmed call made on that belief would sit waiting out the
                // approval timeout for somebody who is not there.
                if unattended {
                    chat.auto_approve = false;
                }
                let web_search_requested = !chat.offline
                    && state
                        .config()
                        .web_search
                        .requested_for(content, metadata.as_ref());
                let preparation = tokio::select! {
                    biased;
                    _ = request.cancel.recv() => {
                        let _=session.close().await;
                        request.cancelled(stream).await;
                        return Ok(());
                    }
                    _ = session.guard.lost() => { return Err(OWNERSHIP_LOST.into()); }
                    result = prepare_chat(state, stream, chat_id, workspace_id, user_id, content, metadata.as_ref(), chat, *route, web_search_requested) => result?,
                };
                handle_chat_generation(state, stream, chat_id, workspace_id, user_id, preparation, &mut request, &mut session, &mut jobs).await
            }
        }
    }.await;
    // The one exit every path takes, including the one a disconnected socket
    // leaves running: a background job belongs to the turn that started it, and
    // a wait's registration and allowance belong to the turn that opened them.
    let exits = job_exits(chat_id, &jobs).await;
    Jobs::kill_session(ToolSession::Chat(chat_id)).await;
    wait::reset_session(ToolSession::Chat(chat_id));
    for job in exits {
        publish(
            stream,
            ServerMessage::JobExited {
                message_id: request.message_id,
                job,
            },
        )
        .await;
    }
    if let Err(error) = session.close().await {
        tracing::warn!(%error,"Could not close interrupted generation");
    }
    if let Err(error) = result {
        tracing::error!("Error handling send message: {error}");
        publish(
            stream,
            ServerMessage::Error {
                message: error.to_string(),
            },
        )
        .await;
    }
    saved
}

/// Run one chat turn with nobody connected.
///
/// A scheduled automation has a prompt and a chat but no browser, and this is
/// the same turn a person's message takes: the prompt is persisted as the user
/// message and the model answers it. Nothing here is a second code path, which
/// is the point — an automation's turn gets the same lease, the same per-chat
/// serialisation and the same recovery as a typed one.
///
/// It gets one thing deliberately different: approvals are not waived. The
/// chat's auto-approve is a person's standing yes, given while they were there
/// to see what it applied to, and this turn runs when they are not. See the
/// `unattended` flag on `handle_send_message` for what that withholds and what
/// it leaves alone.
///
/// No socket is needed because none was ever required. `ChatStream::of` keys a
/// broadcast channel by chat id and `publish` drops a frame nobody is
/// subscribed to, so the frames this turn emits reach whoever is connected and
/// are replayed to whoever connects later from the live-turn log.
///
/// `metadata` marks where the turn came from, so a reader can tell an
/// automation's message from one the person typed.
///
/// Returns the id the answer is stored under. A caller that has to read back
/// what this turn said — a watch keeping its own reading as the next firing's
/// baseline — needs to name the message rather than take the chat's latest,
/// which may by then be something the person typed. The id is allocated before
/// the turn runs, so it is returned whether or not an answer reached that row.
pub(crate) async fn run_turn(
    state: &AppState,
    chat_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    content: &str,
    metadata: Option<serde_json::Value>,
) -> Uuid {
    let stream = ChatStream::of(chat_id);
    let generation = Generation::new(chat_id);
    let message_id = generation.message_id;
    handle_send_message(
        state,
        &stream,
        chat_id,
        workspace_id,
        user_id,
        content,
        metadata,
        generation,
        // There is no socket here and no person behind one, which is the
        // definition of unattended.
        true,
    )
    .await;
    let released = queue::with(chat_id, |queue| match queue.release() {
        Finished::Next(started) => Some(Some(begin(chat_id, started))),
        Finished::Held => Some(None),
        Finished::Idle => None,
    });
    match released {
        Some(Some(started)) => drive(state.clone(), stream, chat_id, workspace_id, started, true),
        Some(None) => announce(&stream, None).await,
        None => {}
    }
    message_id
}

async fn prepare_message(
    state: &AppState,
    chat_id: Uuid,
    workspace_id: Uuid,
    content: &str,
    metadata: Option<&serde_json::Value>,
) -> Result<Routing, Box<dyn std::error::Error + Send + Sync>> {
    // Read the chat fresh rather than trusting the row captured at connect
    // time, so switching model or toggling agent mode takes effect on the next
    // message instead of the next reconnect.
    let chat = match chats::get_chat(state.db(), chat_id).await? {
        Some(chat) => chat,
        None => return Err("Chat not found".into()),
    };
    if chat.workspace_id != Some(workspace_id) {
        return Err("Chat does not belong to the authenticated workspace".into());
    }
    let route = Route::for_chat(state, &chat).await;
    let endpoint = route.endpoint()?.clone();
    let chat = route.apply(chat);
    let mut image_config = state.config().comfyui.clone();
    if let Some(settings) = route.settings() {
        settings.apply_to_comfyui(&mut image_config);
    }
    let mut backend = None;
    let intent = match crate::services::image_intent::reading(&image_config, content, metadata) {
        crate::services::image_intent::Reading::Settled(intent) => intent,
        crate::services::image_intent::Reading::Unsettled(lanes) => {
            let resolved = classifying(
                state,
                workspace_id,
                &chat,
                &route,
                &endpoint,
                &mut image_config,
            )
            .await?;
            let intent = crate::services::image_intent::ImageIntentClassifier::new(
                image_config.clone(),
                endpoint.clone(),
                resolved.clone(),
            )
            .settle(content, lanes)
            .await;
            backend = Some(resolved);
            intent
        }
    }
    .yielding_to_agent(chat.agent_enabled);

    if intent == crate::services::image_intent::GenerationIntent::Chat
        && endpoint.origin() == Origin::Instance
        && crate::services::model::Model::completion(&state.config().ollama_host, &chat.model_name)
            .await
            == Some(false)
    {
        return Err(crate::services::model::UNSUPPORTED.into());
    }

    Ok(match intent {
        crate::services::image_intent::GenerationIntent::Video => Routing::Video(image_config),
        crate::services::image_intent::GenerationIntent::Image => {
            let backend = match backend {
                Some(backend) => backend,
                None => {
                    classifying(
                        state,
                        workspace_id,
                        &chat,
                        &route,
                        &endpoint,
                        &mut image_config,
                    )
                    .await?
                }
            };
            Routing::Image(image_config, backend, endpoint)
        }
        crate::services::image_intent::GenerationIntent::Audio => Routing::Audio(image_config),
        crate::services::image_intent::GenerationIntent::Upscale => Routing::Upscale(image_config),
        crate::services::image_intent::GenerationIntent::Chat => {
            Routing::Chat(chat, Box::new(route))
        }
    })
}

/// The backend a workspace's classifier runs on, with `config` naming the
/// model it classifies with there: the login the chat runs on, while it can.
/// A workspace's backend that can't be resolved leaves it the instance's; a
/// chat's chosen provider is the one it runs on, or it fails.
async fn classifying(
    state: &AppState,
    workspace_id: Uuid,
    chat: &chats::ChatRow,
    route: &Route,
    endpoint: &Endpoint,
    config: &mut crate::config::ComfyUiConfig,
) -> Result<LlmBackend, crate::services::backend::Error> {
    let sticky = match chats::session(state.db(), chat.id).await {
        Ok(session) => session.and_then(|session| session.login),
        Err(error) => {
            tracing::warn!(chat_id = %chat.id, %error, "Could not read the chat's login; classifying on any");
            None
        }
    };
    let backend = match route.backend_for(state, sticky).await {
        Ok(backend) => backend,
        Err(error) if route.chosen().is_some() => return Err(error),
        Err(error) => {
            tracing::warn!(
                %workspace_id,
                %error,
                "Could not resolve the workspace's model backend; classifying on the instance's"
            );
            crate::services::backend::instance(state.config())
        }
    };
    let catalog = endpoint
        .catalog(&state.config().ollama_host, &backend)
        .await;
    let preferences = route.preferences(&config.classifier_model);
    config.classifier_model =
        crate::services::stages::classifier_model(&preferences, &catalog, &chat.model_name);
    Ok(backend)
}

async fn load_web_search(
    state: &AppState,
    chat_id: Uuid,
    content: &str,
    web_search_requested: bool,
    offline: bool,
) -> SearchContext {
    if offline {
        return SearchContext::Disabled;
    }
    let search = SearchContext::new(&state.config().web_search);
    if !web_search_requested || matches!(search, SearchContext::Disabled) {
        return search;
    }
    let query = sanitize_query(content);
    if query.is_empty() {
        return search;
    }
    match SearxngClient::new(state.config().web_search.clone()) {
        Ok(client) => match client.search(&query, None).await {
            Ok(mut hits) if !hits.is_empty() => {
                identify(state.db(), chat_id, &mut hits).await;
                SearchContext::Results(hits)
            }
            Ok(_) => SearchContext::Empty,
            Err(e) => {
                tracing::warn!("Web search failed: {}", e);
                SearchContext::Failed
            }
        },
        Err(e) => {
            tracing::warn!("Failed to create web search client: {}", e);
            SearchContext::Failed
        }
    }
}

/// Register each pre-turn hit against the chat and stamp on the identifier the
/// write returned, which for a URL this chat has already seen is the one it
/// still holds. The injected message is replaced every turn and never
/// persisted, so the registry is what outlives it to resolve a citation.
async fn identify(pool: &PgPool, chat_id: Uuid, hits: &mut [SearchHit]) {
    for hit in hits {
        let observed = chat_sources::observe(
            pool,
            chat_id,
            agent::identifier::Kind::Web,
            &hit.url,
            &hit.url,
            &hit.title,
        )
        .await;
        stamp(chat_id, hit, observed);
    }
}

/// Only the write knows the final identifier, because the registry lengthens a
/// digest that collides. A hit it did not accept stays bare rather than carry
/// one nothing can resolve, and never fails the turn.
fn stamp(chat_id: Uuid, hit: &mut SearchHit, observed: db::DbResult<chat_sources::Source>) {
    match observed {
        Ok(source) => hit.identifier = Some(source.identifier),
        Err(error) => tracing::warn!(
            %chat_id,
            url = %hit.url,
            %error,
            "Could not register a search hit; citing it without an identifier"
        ),
    }
}

/// What a turn's answer opens with when its project could not give it a directory, once for each
/// reason in a chat: repeated, it would open every answer and go into each one's replay and
/// embedding. It names no directory because Zone's tools and a CLI may each be in another.
async fn displacement(state: &AppState, chat_id: Uuid, reason: String) -> Option<String> {
    let notice = format!("> {reason} This chat is starting in its usual folder instead.\n\n");
    match chats::answered_with(state.db(), chat_id, &notice).await {
        Ok(true) => None,
        Ok(false) => Some(notice),
        Err(error) => {
            tracing::warn!(%chat_id, %error, "Could not read whether the chat was told its folder is unavailable");
            Some(notice)
        }
    }
}

/// The directory this turn's project starts it in, cloning its repository first when that is what
/// it takes, and why not when it should have had one. The caller's Stop drops this, which ends
/// the clone. Neither a clone that fails nor a project that can't be read fails the turn. A chat
/// with its agent off has no tools to use a directory with, so it isn't worth a clone, or a word
/// about not having one.
async fn project_directory(
    state: &AppState,
    stream: &ChatStream,
    chat_id: Uuid,
    workspace_id: Uuid,
    agentic: bool,
) -> (Option<std::path::PathBuf>, Option<String>) {
    match project_directory::locate(state, chat_id, workspace_id).await {
        Start::Elsewhere => (None, None),
        Start::Directory(directory) => (Some(directory), None),
        Start::Missing(_) | Start::Unavailable(_) if !agentic => (None, None),
        Start::Unavailable(reason) => (None, Some(reason)),
        Start::Missing(missing) => {
            publish(
                stream,
                ServerMessage::Status {
                    message: format!("Cloning {}…", missing.url()),
                },
            )
            .await;
            match missing.clone_repository().await {
                Ok(directory) => (Some(directory), None),
                Err(reason) => {
                    tracing::warn!(%chat_id, %reason, "Could not clone the project's repository");
                    (None, Some(reason))
                }
            }
        }
    }
}

async fn prepare_chat(
    state: &AppState,
    stream: &ChatStream,
    chat_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    content: &str,
    metadata: Option<&serde_json::Value>,
    mut chat: chats::ChatRow,
    route: Route,
    web_search_requested: bool,
) -> Result<ChatPreparation, Box<dyn std::error::Error + Send + Sync>> {
    let (directory, displaced) =
        project_directory(state, stream, chat_id, workspace_id, chat.agent_enabled).await;
    let previous = match chats::session(state.db(), chat_id).await {
        Ok(previous) => previous,
        Err(error) => {
            tracing::warn!(%chat_id, %error, "Could not read the chat's agent session; starting a fresh one");
            None
        }
    };
    let configured = route.settings().and_then(|settings| settings.agent());
    let model = handover::Model {
        requested: chat.model_name.clone(),
        preferences: route.preferences(&state.config().comfyui.classifier_model),
        message: content.to_string(),
        image: crate::services::media_source::has_image_attachment(metadata),
        agentic: chat.agent_enabled,
    };
    let running = configured.map(|agent| model.on(agent));
    let mut resolved = route
        .chat(state, &[], previous.as_ref(), running.as_deref())
        .await?;
    if let Some(directory) = &directory {
        resolved.backend = crate::services::backend::in_directory(resolved.backend, directory);
    }
    let organization = configured.and_then(|agent| route.organization_on(agent));
    let opening = match organization.zip(configured) {
        Some((organization, configured)) => {
            handover::opening(
                state,
                organization,
                configured,
                previous.as_ref(),
                &mut resolved,
                running.as_deref(),
                directory.as_deref(),
            )
            .await
        }
        None => None,
    };
    if let Some(organization) = organization {
        handover::settle(state.config(), organization, &resolved).await;
    }
    let current = resolved.login.clone();
    let chosen = route.chosen().is_some();
    let endpoint = route.into_endpoint()?;
    let catalog = endpoint
        .catalog(&state.config().ollama_host, &resolved.backend)
        .await;
    chat.model_name = crate::services::stages::chat_model(
        &model.requested,
        &model.preferences,
        &catalog,
        &model.message,
        model.image,
        model.agentic,
    );
    if web_search_requested && !sanitize_query(content).is_empty() {
        publish(
            stream,
            ServerMessage::Status {
                message: "Searching the web...".into(),
            },
        )
        .await;
    }
    let driver = Driver::on(&resolved.backend, model.requested.clone());
    let mut preparation = session::build(
        state,
        &chat,
        user_id,
        None,
        session::Mode::Generation(session::Generation {
            resolved,
            session: previous.clone(),
            directory: directory.clone(),
            origin: db::tasks::Origin::of_turn(metadata),
        }),
        endpoint,
        driver,
    )
    .await?;
    preparation.displaced = match displaced {
        Some(reason) => displacement(state, chat_id, reason).await,
        None => None,
    };
    preparation.handover = match (organization, current, &preparation.session) {
        (Some(organization), Some(current), Some(_)) => {
            let handover = Handover::new(organization, current, model, previous.as_ref(), opening)
                .in_directory(directory);
            Some(match chosen {
                true => handover.strict(),
                false => handover,
            })
        }
        _ => None,
    };
    let search = load_web_search(state, chat_id, content, web_search_requested, chat.offline).await;
    let agentic = preparation.agentic;
    let character = chat.character.as_ref();
    let prompt = session::system_prompt(
        &chat,
        &preparation.tools,
        agentic,
        &search.capability(),
        &preparation.environment,
        &preparation.memory,
        &preparation.skills,
    );
    let mut retrieval = String::new();
    if !agentic && character.is_none() {
        let query_embedding = match state.embedding_service() {
            Some(embedding_service) => {
                match embedding_service
                    .embed(&abnegate_index::embed_query_text(
                        embedding_service.model(),
                        content,
                    ))
                    .await
                {
                    Ok(embedding) => Some(embedding),
                    Err(error) => {
                        tracing::warn!(%error, "Knowledge query embed failed; keyword only");
                        None
                    }
                }
            }
            None => None,
        };

        let attached = chat_attached_sources::scope(state.db(), Some(chat_id)).await;
        let mut knowledge_hits = Vec::new();
        if let (Some(embedding), None) = (query_embedding.as_deref(), attached.as_ref()) {
            match knowledge::search_knowledge_entries(
                state.db(),
                embedding,
                workspace_id,
                MAX_CONTEXT_IN_PROMPT as i64,
                0.5,
            )
            .await
            {
                Ok(hits) => knowledge_hits = hits,
                Err(error) => tracing::warn!(%error, "Knowledge semantic search failed"),
            }
        }
        if attached.is_none() {
            match knowledge::search_knowledge_keyword(
                state.db(),
                content,
                workspace_id,
                MAX_CONTEXT_IN_PROMPT as i64,
            )
            .await
            {
                Ok(hits) => {
                    knowledge_hits = knowledge::fuse_knowledge_hits(
                        knowledge_hits,
                        hits,
                        content,
                        MAX_CONTEXT_IN_PROMPT,
                    );
                }
                Err(error) => tracing::warn!(%error, "Knowledge keyword search failed"),
            }
        }

        let mut source_lines = Vec::new();
        if let Some(context_service) = state.context_service() {
            let filters = zone_context::embeddings::SearchFilters {
                workspace_id: Some(workspace_id),
                source_ids: attached.clone(),
                categories: None,
                min_quality: None,
                since: None,
            };
            match context_service
                .search_hybrid_with_embedding(
                    content,
                    query_embedding.as_deref(),
                    MAX_CONTEXT_RESULTS,
                    Some(filters),
                    None,
                )
                .await
            {
                Ok(results) => {
                    source_lines = results
                        .into_iter()
                        .take(MAX_CONTEXT_IN_PROMPT)
                        .map(|result| {
                            format_retrieved_line(
                                "source",
                                &result.item_title,
                                &result.item_uri,
                                &result.chunk_text,
                            )
                        })
                        .collect();
                }
                Err(error) => tracing::warn!(%error, "Context search failed"),
            }
        }

        let knowledge_lines: Vec<String> = knowledge_hits
            .into_iter()
            .map(|hit| {
                format_retrieved_line(
                    "knowledge",
                    &hit.title,
                    &format!("knowledge://{}", hit.entry_id),
                    &hit.content,
                )
            })
            .collect();

        let context_lines =
            interleave_context_lines(knowledge_lines, source_lines, MAX_CONTEXT_IN_PROMPT);
        if !context_lines.is_empty() {
            retrieval = session::prompt::retrieved(&context_lines);
        }
    }
    preparation.context.entries[0].message = LlmMessage::system(format!("{prompt}{retrieval}"));
    preparation.context.search(&search);
    pin(
        state,
        chat_id,
        workspace_id,
        &mut preparation,
        &prompt,
        &retrieval,
        &search,
    )
    .await;
    Ok(preparation)
}

/// Settles the coding agent session `preparation` runs in, when it runs in one: a session the
/// chat resumes is sent only what it has not seen, opened by one system note. One with nothing
/// left to answer, or whose place in the chat can't be read, starts afresh on the whole
/// transcript. Either keeps the whole transcript, for a resume the agent refuses and for a
/// handover that replays it on another login.
async fn pin(
    state: &AppState,
    chat_id: Uuid,
    workspace_id: Uuid,
    preparation: &mut ChatPreparation,
    prompt: &str,
    retrieval: &str,
    search: &SearchContext,
) {
    let Some(pinned) = preparation.session.as_mut() else {
        return;
    };
    let store = crate::db::context::Store::new(state.db().clone(), chat_id, Some(workspace_id));
    let latest = store.latest().await;
    pinned.sees(
        session::prompt::stable(prompt),
        latest.as_ref().copied().unwrap_or_default(),
    );
    let Some(previous) = pinned.resumed().cloned() else {
        pinned.keep(preparation.context.clone());
        return;
    };
    let unseen = match latest {
        Ok(_) => store.after(previous.entry).await,
        Err(error) => Err(error),
    }
    .unwrap_or_else(|error| {
        tracing::warn!(%chat_id, %error, "Could not read what the agent session has not seen; starting a fresh one");
        Vec::new()
    });
    let note = session::tail::note(prompt, previous.prompt.as_deref(), retrieval, search);
    match session::tail::assemble(&preparation.context, &unseen, note) {
        Some(tail) => {
            let replay = std::mem::replace(&mut preparation.context, tail);
            pinned.keep(replay);
        }
        None => {
            let backend = pinned.renew(preparation.llm.backend().clone());
            preparation.llm = preparation.llm.clone().with_backend(backend);
            pinned.keep(preparation.context.clone());
        }
    }
}

/// A sandboxed chat (`chats.agent_sandboxed`) gives the agent zone's tools and
/// nothing else. An unsandboxed one leaves it the file and shell tools it
/// ships with, which run outside zone. Claude asks zone before running one,
/// under the chat's approval policy; codex's are confined by its sandbox alone.
fn builtin_tools(sandboxed: bool) -> BuiltinTools {
    match sandboxed {
        true => BuiltinTools::Withheld,
        false => BuiltinTools::Granted,
    }
}

/// A turn's coding agent reaches the turn's tools only when it is served them,
/// but is watched either way. Both belong to the backend they were attached
/// to, so a turn that moves to another backend equips it again.
fn equipped(
    client: LlmClient,
    served: Option<&AgentTools>,
    observer: &Arc<dyn Observer>,
    sandboxed: bool,
) -> LlmClient {
    let client = match served {
        Some(served) => client.with_toolset(served.lease.toolset(), builtin_tools(sandboxed)),
        None => client,
    };
    client.with_observer(observer.clone())
}

/// `jobs` collects the background jobs this turn started and has not yet
/// reported as exited, so the teardown that kills them can say each one is gone.
async fn handle_chat_generation(
    state: &AppState,
    stream: &ChatStream,
    chat_id: Uuid,
    workspace_id: Uuid,
    user_id: Uuid,
    preparation: ChatPreparation,
    generation: &mut Generation,
    session: &mut Session,
    jobs: &mut Vec<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ChatPreparation {
        mut model,
        agentic,
        auto_approve,
        sandboxed,
        mut tools,
        mut context,
        llm: llm_client,
        endpoint,
        stop,
        mut budget,
        timeout,
        environment: _,
        memory: _,
        skills: _,
        session: mut pinned,
        mut handover,
        displaced,
        effort,
        driver,
    } = preparation;
    let offline = tools.offline();
    let origin = tools.origin();
    let directory = tools.working_directory().to_path_buf();
    let mut replay = context.clone();
    let definitions = agentic.then(|| tools.definitions().to_vec());
    // Bound for the whole turn on purpose. The token is revoked when the lease
    // drops, and the agent is still calling with it until its last round has
    // returned.
    let mut agent_tools = agentic
        .then(|| {
            AgentTools::serve(
                llm_client.backend(),
                &mut tools,
                crate::mcp::Scope {
                    workspace: workspace_id,
                    chat: chat_id,
                    user: Some(user_id),
                    approval: generation.approvals.clone(),
                    calls: budget.max_tool_calls,
                    builtin: builtin_tools(sandboxed),
                },
                &crate::mcp::local_endpoint(state.config()),
            )
        })
        .flatten();
    let mut token_filter = TokenFilter::new(stop);
    let assistant_message_id = generation.message_id;
    let stream_deadline = generation_deadline(timeout)?;
    if generation.cancel.try_recv().is_ok() {
        session.close().await?;
        generation.cancelled(stream).await;
        return Ok(());
    }
    let tracker = Tracker::new(
        state.clone(),
        chat_id,
        assistant_message_id,
        Arc::new(SubagentFrames {
            stream: ChatStream::of(chat_id),
        }),
    );
    let observer: Arc<dyn Observer> = Arc::new(Observers::new(
        Recorder::new(state.clone(), chat_id, workspace_id),
        tracker,
    ));
    let mut llm_client = equipped(llm_client, agent_tools.as_ref(), &observer, sandboxed);

    publish(
        stream,
        ServerMessage::MessageStart {
            message_id: assistant_message_id,
            role: "assistant".to_string(),
            resumed: false,
        },
    )
    .await;
    generation.started = true;

    let mut handovers: Vec<Notice> = Vec::new();
    let mut usage: Option<Usage> = None;
    let mut logins: Vec<Uuid> = handover.iter().map(Handover::login).collect();
    let mut observed = router::Observed::default();
    if let Some(notice) = handover.as_mut().and_then(Handover::opened) {
        publish(
            stream,
            ServerMessage::Handover {
                message_id: assistant_message_id,
                notice: notice.clone(),
            },
        )
        .await;
        handovers.push(notice);
    }

    let mut full_content = String::new();
    let mut pending_content = String::new();
    let mut round_reasoning = Thinking::default();
    let mut pending_images = Vec::<String>::new();
    let mut generated_images = Vec::new();
    let mut chunk_index = 0;
    let mut cancelled = false;
    let mut failure = None;
    let mut blocked = None;
    let mut response_truncated = false;
    let mut asked = false;
    let mut waited = false;
    let mut cut_short = false;
    let mut tool_calls: Vec<ToolCallRecord> = Vec::new();
    let mut citations: Vec<Citation> = Vec::new();
    let mut action_receipts: Vec<ActionReceipt> = Vec::new();
    let mut last_snapshot: Option<Instant> = None;
    let mut waits: Vec<String> = Vec::new();
    // Whether the round is the answer going on from what another login wrote of it, which a
    // refused resume replays with that part and the instruction to go on.
    let mut continuing = false;

    if let Some(notice) = displaced {
        emit_chunk(
            stream,
            &mut full_content,
            &mut chunk_index,
            notice,
            &mut response_truncated,
        )
        .await;
    }
    let told = full_content.len();

    loop {
        wait::set_ceiling(ToolSession::Chat(chat_id), stream_deadline);
        let written = (full_content.len(), tool_calls.len());
        let round = agent::run_with_context(
            AgentRun {
                llm: llm_client.clone(),
                model: model.clone(),
                tools,
                messages: Vec::new(),
                budget,
                approval: {
                    generation.approvals.set_auto(auto_approve);
                    generation.approvals.clone()
                },
            },
            &mut context,
            agentic,
        );
        let mut events: Pin<Box<dyn Stream<Item = AgentEvent> + Send + '_>> =
            match agent_tools.as_mut() {
                Some(served) => Box::pin(crate::mcp::merged(round, &mut served.calls)),
                None => Box::pin(round),
            };
        let mut pending_wait: Option<(Waited, Spend)> = None;
        let mut refused = false;
        let mut handing: Option<Cause> = None;

        loop {
            tokio::select! {
                biased;
                _ = session.guard.lost() => { failure=Some(OWNERSHIP_LOST.into()); break; }
                // Check for cancellation before polling another event.
                _ = generation.cancel.recv() => {
                    cancelled = true;
                    tracing::debug!("Stream cancelled for message {}", assistant_message_id);
                    break;
                }

                _ = tokio::time::sleep_until(stream_deadline) => {
                    tracing::warn!("LLM stream timeout for chat {}, message {}", chat_id, assistant_message_id);
                    failure = Some(GENERATION_TIMED_OUT.to_string());
                    break;
                }

                // Process agent events
                event = events.next() => {
                    let mut persist = false;
                    let mut persist_now = false;
                    let mut stop_stream = false;
                    match event {
                        Some(AgentEvent::Canonical(entry)) => {
                            if entry.message.role == LlmRole::Assistant {
                                let leftover=token_filter.finish();
                                if !leftover.is_empty() {
                                    pending_content.push_str(&leftover);
                                    let _=emit_chunk(stream,&mut full_content,&mut chunk_index,leftover,&mut response_truncated).await;
                                    persist = true;
                                }
                            }
                            if let Err(error)=session.store.append(&session.lease,session.turn,std::slice::from_ref(&entry)).await {failure=Some(error.to_string());break;}
                            pending_images.retain(|image| !entry.message.images.contains(image) && !entry.message.generated_images.contains(image));
                            replay.append(&entry);
                            pending_content.clear();
                            if let Some((call, job)) = spawned_job(&entry.message) {
                                if let Some(record) = tool_calls.iter_mut().find(|record| record.id == call) {
                                    record.job = Some(job.clone());
                                }
                                jobs.push(job.id.clone());
                                let job_started = ServerMessage::JobStarted {
                                    message_id: assistant_message_id,
                                    tool_call_id: call,
                                    job,
                                };
                                publish(stream, job_started).await;
                                persist_now = true;
                            }
                        }
                        Some(AgentEvent::Consumed(ids)) => {
                            if let Err(error)=session.store.consumed(&session.lease,&ids).await {failure=Some(error.to_string());break;}
                            for entry in &mut replay.entries {if ids.contains(&entry.id){entry.consumed=true;}}
                        }
                        Some(AgentEvent::Checkpoint {previous,summary}) => {
                            let expected=previous.as_ref().map(session::stored_summary);
                            if let Err(error)=session.store.checkpoint(&session.lease,expected.as_ref(),&session::stored_summary(&summary)).await {failure=Some(error.to_string());break;}
                            replay.summary=Some(summary);
                        }
                        Some(AgentEvent::Context(usage)) => {
                            if usage.status==abnegate_agent::context::ContextStatus::Blocked { blocked=Some(usage.clone()); }
                            if let Err(error)=session.store.assert_current(&session.lease).await {failure=Some(error.to_string());break;}
                            publish(stream,ServerMessage::Context {chat_id,message_id:Some(assistant_message_id),usage}).await;
                        }
                        Some(AgentEvent::Usage(observed)) => {
                            tracing::debug!(prompt_tokens=observed.prompt_tokens,completion_tokens=observed.completion_tokens,"Observed provider usage");
                            usage = Some(usage.map_or(observed, |spent| spent.plus(observed)));
                        }
                        Some(AgentEvent::Window(window)) => {
                            tracing::debug!(window=%window.name,used_percent=?window.used_percent,"Observed agent usage window");
                            if let Some(handover) = &handover {
                                observed.observe(handover.login(), window);
                            }
                        }
                        Some(AgentEvent::Session(id)) => {
                            if let Some(pinned) = pinned.as_mut() {
                                pinned.announced(id);
                                remember(state, chat_id, pinned.record(pinned.entry())).await;
                            }
                        }
                        Some(AgentEvent::Finalizing(message)) => { publish(stream,ServerMessage::Status {message}).await; }
                        Some(AgentEvent::Reasoning(content)) => {
                            round_reasoning.push(&content, || full_content.chars().count());
                            publish(stream, ServerMessage::Reasoning { content }).await;
                            persist = true;
                        }
                        Some(AgentEvent::Chunk(content)) => {
                            let filtered = match token_filter.push(&content) {
                                FilterStep::Hold => continue,
                                FilterStep::Emit(text) => text,
                                FilterStep::Halt(text) => {
                                    pending_content.push_str(&text);
                                    if !text.is_empty() {
                                        let _ = emit_chunk(
                                            stream,
                                            &mut full_content,
                                            &mut chunk_index,
                                            text,
                                            &mut response_truncated,
                                        )
                                        .await;
                                    }
                                    persist_now = true;
                                    stop_stream = true;
                                    String::new()
                                }
                            };
                            if !stop_stream {
                                pending_content.push_str(&filtered);
                                if !emit_chunk(
                                    stream,
                                    &mut full_content,
                                    &mut chunk_index,
                                    filtered,
                                    &mut response_truncated,
                                )
                                .await
                                {
                                    pending_content.clone_from(&full_content);
                                    persist_now = true;
                                    stop_stream = true;
                                } else {
                                    persist = true;
                                }
                            }
                        }
                        Some(AgentEvent::ToolApprovalRequired { id, name, arguments, reason, preview }) => {
                            if let Some(record) = tool_calls.iter_mut().find(|r| r.id == id) {
                                record.detail = AWAITING_APPROVAL_DETAIL.to_string();
                                record.preview.clone_from(&preview);
                            }
                            let arguments = crate::agent::memory::trace_arguments(&name, &arguments)
                                .unwrap_or(arguments);
                            let reason = crate::agent::memory::trace_reason(&name, reason);
                            let tool_msg = ServerMessage::ToolApprovalRequired {
                                message_id: assistant_message_id,
                                tool_call_id: id,
                                name,
                                arguments,
                                reason,
                                preview,
                            };
                            publish(stream, tool_msg).await;
                            persist_now = true;
                        }
                        Some(AgentEvent::ToolCallStarted { id, name, arguments }) => {
                            // Recorded before the tool runs so a turn cancelled
                            // mid-call still shows what it was doing.
                            let thought = round_reasoning.take();
                            let (record, tool_msg) = trace_started(
                                assistant_message_id,
                                id,
                                name,
                                &arguments,
                                full_content.chars().count(),
                                thought,
                            );
                            tool_calls.push(record);
                            publish(stream, tool_msg).await;
                            persist_now = true;
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

                            if abnegate_agent::tool::is_vision_url(&url) && !replay.entries.iter().any(|entry|entry.message.images.contains(&url) || entry.message.generated_images.iter().any(|image|image.image_url.url==url)) {
                                pending_images.push(url);
                            }
                            let media_msg = if attachment.mime.starts_with("audio/") {
                                ServerMessage::Audio {
                                    message_id: assistant_message_id,
                                    attachment: attachment.clone(),
                                }
                            } else {
                                ServerMessage::Image {
                                    message_id: assistant_message_id,
                                    attachment: attachment.clone(),
                                }
                            };
                            generated_images.push(attachment);
                            publish(stream, media_msg).await;
                            persist_now = true;
                        }
                        Some(AgentEvent::ToolCallCompleted { id, name, success, detail, duration_ms, citations: observed, receipt, refusal }) => {
                            let detail = crate::agent::memory::trace_detail(&name, &detail);
                            if let Some(record) = tool_calls.iter_mut().find(|r| r.id == id) {
                                record.success = success;
                                record.detail = detail.clone();
                                record.duration_ms = duration_ms;
                                record.refusal = refusal;
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
                                refusal,
                            };
                            publish(stream, tool_msg).await;
                            if let Some(receipt) = receipt {
                                let receipt_msg = ServerMessage::ActionReceipt {
                                    message_id: assistant_message_id,
                                    receipt: receipt.clone(),
                                };
                                action_receipts.push(receipt);
                                publish(stream, receipt_msg).await;
                            }
                            persist_now = true;
                        }
                        Some(AgentEvent::QuestionRequired { tool_call_id, questions, .. }) => {
                            // The live frame log is cleared by the message_end this
                            // turn is about to reach, so a reader who reloads has
                            // only the stored record to rebuild the card from.
                            // Persist it before the socket can go.
                            if let Some(record) = tool_calls.iter_mut().find(|r| r.id == tool_call_id) {
                                record.detail = AWAITING_ANSWER_DETAIL.to_string();
                                record.questions.clone_from(&questions);
                            }
                            asked = true;
                            let question_msg = ServerMessage::QuestionRequired {
                                message_id: assistant_message_id,
                                tool_call_id,
                                questions,
                            };
                            publish(stream, question_msg).await;
                            persist_now = true;
                        }
                        Some(AgentEvent::WaitRequired { tool_call_id, waiting, spent }) => {
                            // Persisted before the wait rather than after it, for
                            // the reason the question arm gives: the live frame log
                            // is cleared by the message_end this turn reaches, and
                            // a reader who reloads mid-wait has only the stored
                            // record to rebuild the card from.
                            if let Some(record) = tool_calls.iter_mut().find(|r| r.id == tool_call_id) {
                                record.waiting = Some(waiting.clone());
                            }
                            waited = true;
                            let wait_started = ServerMessage::WaitStarted {
                                message_id: assistant_message_id,
                                tool_call_id: tool_call_id.clone(),
                                waiting: waiting.clone(),
                            };
                            publish(stream, wait_started).await;
                            pending_wait = Some((Waited::new(tool_call_id, waiting), spent));
                            persist_now = true;
                            stop_stream = true;
                        }
                        Some(AgentEvent::Limited(limit)) => {
                            if handover.is_some() {
                                handing = Some(Cause::limited(limit));
                            } else {
                                failure = Some(self::failure(&endpoint, llm_client.backend(), limit.message));
                            }
                            break;
                        }
                        Some(AgentEvent::Failed(message)) => {
                            if (full_content.len(), tool_calls.len()) == written
                                && pinned.as_ref().is_some_and(|pinned| pinned.refused(&message))
                            {
                                tracing::info!(%chat_id, %message, "The agent has no session to resume; replaying the chat in a fresh one");
                                refused = true;
                                break;
                            }
                            let remedied = crate::services::backend::remedied(llm_client.backend(), message);
                            if remedied.signed_out && handover.is_some() {
                                handing = Some(Cause::signed_out(remedied.message));
                            } else {
                                failure = Some(endpoint.scrub(&remedied.message));
                            }
                            break;
                        }
                        Some(AgentEvent::CutShort) => cut_short = true,
                        None => {
                            // Stream ended
                            break;
                        }
                    }
                    if persist_now
                        || (persist
                            && last_snapshot
                                .map(|instant| instant.elapsed() >= LIVE_SNAPSHOT_INTERVAL)
                                .unwrap_or(true))
                    {
                        match publish_live_assistant(
                            session,
                            &full_content,
                            &tool_calls,
                            &citations,
                            &action_receipts,
                            &generated_images,
                            &round_reasoning,
                            &handovers,
                            usage,
                        )
                        .await
                        {
                            Ok(()) => last_snapshot = Some(Instant::now()),
                            Err(error) => {
                                tracing::warn!("Failed to persist live assistant snapshot: {error}");
                                if error.contains("ownership") || error.contains("expired") {
                                    failure = Some(error);
                                    break;
                                }
                            }
                        }
                    }
                    if stop_stream {
                        break;
                    }
                }
            }
        }

        // Drop the producer before acknowledging cancellation so it cannot emit
        // more events after the terminal frame.
        drop(events);

        if refused
            && !cancelled
            && let Some((whole, backend)) = pinned
                .as_mut()
                .and_then(|pinned| pinned.restart(llm_client.backend().clone()))
        {
            llm_client = equipped(
                llm_client.with_backend(backend),
                agent_tools.as_ref(),
                &observer,
                sandboxed,
            );
            replay = whole.clone();
            context = match continuing {
                true => handover::replayed(&whole),
                false => whole,
            };
            // A coding agent reaches zone's tools over the turn's MCP lease, so
            // the round it refused ran on none of its own, as this one does.
            tools = ChatTools::empty();
            continue;
        }
        if let Some(cause) = handing.take() {
            let remaining = stream_deadline.saturating_duration_since(tokio::time::Instant::now());
            let moved = match handover.as_mut() {
                Some(handover) if !cancelled => {
                    observed.record(state, handover.login()).await;
                    let session = pinned.as_ref().and_then(|pinned| pinned.id());
                    handover.next(state, &cause, session, remaining).await
                }
                _ => Err(None),
            };
            let switch = match moved {
                Ok(switch) => switch,
                Err(ended) => {
                    failure = Some(match ended {
                        Some(error) => error.to_string(),
                        None => self::failure(&endpoint, llm_client.backend(), cause.message),
                    });
                    break;
                }
            };
            let leftover = token_filter.finish();
            if !leftover.is_empty() {
                pending_content.push_str(&leftover);
                let _ = emit_chunk(
                    stream,
                    &mut full_content,
                    &mut chunk_index,
                    leftover,
                    &mut response_truncated,
                )
                .await;
            }
            let wrote = (full_content.len(), tool_calls.len()) != written;
            if let Some(entry) = handover::partial(&std::mem::take(&mut pending_content)) {
                if let Err(error) = session
                    .store
                    .append(&session.lease, session.turn, std::slice::from_ref(&entry))
                    .await
                {
                    failure = Some(error.to_string());
                    break;
                }
                replay.append(&entry);
                if let Some(pinned) = pinned.as_mut() {
                    pinned.extend(&entry);
                }
            }
            tracing::info!(%chat_id, from = %switch.from.id, to = %switch.to.id, carried = switch.carried, "Handing the turn over to another login");
            let notice = switch.notice(
                full_content.chars().count(),
                tool_calls.len(),
                round_reasoning.take().map(|thought| thought.text),
            );
            publish(
                stream,
                ServerMessage::Status {
                    message: format!("Switching to {}…", switch.to.label),
                },
            )
            .await;
            publish(
                stream,
                ServerMessage::Handover {
                    message_id: assistant_message_id,
                    notice: notice.clone(),
                },
            )
            .await;
            handovers.push(notice);
            if !logins.contains(&switch.to.id) {
                logins.push(switch.to.id);
            }
            let whole = pinned
                .as_mut()
                .and_then(|pinned| {
                    pinned.switch(switch.to.clone(), switch.session());
                    pinned.transcript().cloned()
                })
                .unwrap_or_else(|| replay.clone());
            if let Some(pinned) = &pinned {
                match switch.carried {
                    true => remember(state, chat_id, pinned.record(pinned.entry())).await,
                    false => forget(state, chat_id).await,
                }
            }
            if let Some(repicked) = switch.model.clone() {
                model = repicked;
            }
            llm_client = llm_client.with_backend(crate::services::backend::effort(
                crate::services::backend::ahead_of(switch.backend.clone(), remaining),
                effort,
            ));
            llm_client = equipped(llm_client, agent_tools.as_ref(), &observer, sandboxed);
            tools = ChatTools::empty();
            continuing = wrote;
            context = match (wrote, switch.carried) {
                (true, true) => handover::carried(&whole),
                (true, false) => handover::replayed(&whole),
                (false, true) => context,
                (false, false) => whole,
            };
            if let Err(error) = publish_live_assistant(
                session,
                &full_content,
                &tool_calls,
                &citations,
                &action_receipts,
                &generated_images,
                &round_reasoning,
                &handovers,
                usage,
            )
            .await
            {
                tracing::warn!("Failed to persist the handover: {error}");
                if error.contains("ownership") || error.contains("expired") {
                    failure = Some(error);
                    break;
                }
            }
            last_snapshot = Some(Instant::now());
            continue;
        }
        if cancelled || failure.is_some() {
            break;
        }
        let Some((opened, spent)) = pending_wait else {
            break;
        };
        let outcome = match settle_wait(
            wait::await_outcome(
                ToolSession::Chat(chat_id),
                &opened.tool_call_id,
                wait::deadline(&opened.waiting),
            ),
            session.guard.lost(),
            &mut generation.cancel,
            stream_deadline,
        )
        .await
        {
            WaitEnd::Settled(outcome) => outcome,
            WaitEnd::Cancelled => {
                cancelled = true;
                break;
            }
            WaitEnd::Lost => {
                failure = Some(OWNERSHIP_LOST.to_string());
                break;
            }
            WaitEnd::Expired => {
                failure = Some(GENERATION_TIMED_OUT.to_string());
                break;
            }
        };
        if opened.waiting.kind == wait::KIND_JOB
            && let Some(job) = job_wait_exit(chat_id, &opened.waiting.id, jobs).await
        {
            let exited = ServerMessage::JobExited {
                message_id: assistant_message_id,
                job,
            };
            publish(stream, exited).await;
        }
        let Outcome { verdict, text } = outcome;
        let settled = ServerMessage::WaitSettled {
            message_id: assistant_message_id,
            settled: WaitSettled {
                tool_call_id: opened.tool_call_id.clone(),
                outcome: text.clone(),
                verdict,
            },
        };
        publish(stream, settled).await;

        // The console has the outcome from that frame; the model needs it in
        // its context, and a turn started later rebuilds context from
        // canonical entries alone, so the pair is stored as well as mirrored.
        let resumed = wait::resume_with_outcome(&mut replay, &opened, text, &mut waits);
        if let Err(error) = session
            .store
            .append(&session.lease, session.turn, &resumed)
            .await
        {
            failure = Some(error.to_string());
            break;
        }
        if let Err(error) = publish_live_assistant(
            session,
            &full_content,
            &tool_calls,
            &citations,
            &action_receipts,
            &generated_images,
            &round_reasoning,
            &handovers,
            usage,
        )
        .await
        {
            tracing::warn!("Failed to persist the wait that settled: {error}");
            if error.contains("ownership") || error.contains("expired") {
                failure = Some(error);
                break;
            }
        }
        last_snapshot = Some(Instant::now());

        // ChatTools is not Clone and the run took the set it was given by
        // value, so the run that resumes the turn needs one of its own.
        tools = ChatTools::build(WorkspaceScope {
            state: state.clone(),
            workspace_id,
            chat_id: Some(chat_id),
            user_id,
            offline,
            origin,
        })
        .await
        .starting_in(Some(directory.clone()))
        .await;
        // A planner chat keeps its two closing calls across a rebuilt
        // catalog, as `session::build` gave them to the first turn.
        tools = match crate::db::chats::link(state.db(), chat_id).await {
            Ok(Some(link))
                if link.purpose == crate::db::chats::ChatPurpose::ProjectPlanner
                    && link.project_id.is_none() =>
            {
                tools.with_planner()
            }
            _ => tools,
        };
        context = replay.clone();
        budget = budget.less(spent);
    }

    for login in logins {
        settle(state, login, observed.take(login));
    }

    let owned = failure.as_deref() != Some(OWNERSHIP_LOST);
    if let (true, Some(driver), Some(occupancy)) = (owned, driver, llm_client.occupancy()) {
        let ran = Driver::on(llm_client.backend(), driver.model.clone()).unwrap_or(driver);
        ran.record(state.db(), chat_id, occupancy).await;
        replay.policy = session::policy(
            &state.config().chat,
            &Capacity::reported(&ran.model, occupancy.window),
        );
        replay.reported = occupancy.used;
    }

    let leftover = token_filter.finish();
    if !leftover.is_empty() {
        pending_content.push_str(&leftover);
        let _ = emit_chunk(
            stream,
            &mut full_content,
            &mut chunk_index,
            leftover,
            &mut response_truncated,
        )
        .await;
    }
    leave_unfinished(&mut tool_calls);

    // Nothing generated yet: there is no reply worth keeping. A turn that ran
    // tools or produced an image before it broke is worth keeping even with no
    // prose, because both show the reader what was attempted.
    if (cancelled || failure.is_some())
        && full_content.len() == told
        && tool_calls.is_empty()
        && generated_images.is_empty()
    {
        match session.store.interrupt(&session.lease, session.turn).await {
            Ok(()) => {}
            Err(crate::db::context::Error::LeaseLost) => {
                session.settle(None, None, None).await?;
            }
            Err(error) => return Err(error.into()),
        }
        session.close().await?;
        publish(
            stream,
            ServerMessage::Context {
                chat_id,
                message_id: Some(assistant_message_id),
                usage: blocked.unwrap_or_else(|| replay.usage(&model, definitions.as_deref())),
            },
        )
        .await;
        let terminal = match failure {
            Some(message) => ServerMessage::Error { message },
            None => ServerMessage::Cancelled {
                message_id: Some(assistant_message_id),
            },
        };
        publish(stream, terminal).await;
        return Ok(());
    }

    let ending = if response_truncated {
        Ending::Truncated
    } else if failure.is_some() {
        Ending::Interrupted
    } else if cut_short {
        Ending::CutShort
    } else {
        Ending::Finished
    };
    if let Some(note) = ending.note() {
        full_content.push_str(note);
    }

    // Tools can run without the model ever producing prose. The turn is still
    // worth keeping for its trace, but an assistant message with no content
    // reads as a bug, and providers reject one when it comes back as history.
    // An image is its own answer, so it does not need the placeholder.
    // A turn that ended on a question or a wait is not silent but waiting, and
    // saying so keeps the card's own bubble from reading as a failure.
    if generated_images.is_empty()
        && fill_silence(&mut full_content, told, silent_turn(asked, waited))
    {
        let kept = full_content[..told].chars().count();
        clamp_offsets(kept, &mut tool_calls, &mut handovers, &mut round_reasoning);
    }

    merge_cited_sources(state, chat_id, &full_content, &mut citations).await;

    // Images, the tool trace, citations, and write receipts share one
    // metadata object, so a turn that produced more than one keeps all of them.
    let assistant_metadata = merge_metadata(
        image_metadata(&generated_images),
        &tool_calls,
        &citations,
        &action_receipts,
        Some(&round_reasoning),
        &handovers,
        usage,
    );

    let partial = if !pending_content.is_empty() || !pending_images.is_empty() {
        let mut message = LlmMessage::assistant(pending_content);
        message.images = pending_images;
        Some(ReplayMessage::from(&message))
    } else {
        None
    };
    match session
        .store
        .finish(
            &session.lease,
            session.turn,
            &full_content,
            assistant_metadata.clone(),
            cancelled || failure.is_some() || response_truncated,
            partial.as_ref(),
        )
        .await
    {
        Ok(msg) => {
            if let Some(pinned) = &pinned {
                let entry = session.store.latest().await.unwrap_or_else(|error| {
                    tracing::warn!(%chat_id, %error, "Could not read how far the chat has run; the agent session keeps its place");
                    pinned.entry()
                });
                remember(state, chat_id, pinned.record(entry)).await;
            }
            let history = session.store.load().await?;
            replay
                .entries
                .retain(|entry| entry.message.role == LlmRole::System);
            replay
                .entries
                .extend(history.entries.into_iter().map(|entry| {
                    abnegate_agent::context::Entry::new(entry.id, entry.message.into_message())
                        .with_consumed(entry.consumed)
                }));
            replay.summary = history.summary.map(session::core_summary);
            replay.search(&SearchContext::new(&state.config().web_search));
            let usage = blocked.unwrap_or_else(|| replay.usage(&model, definitions.as_deref()));
            publish(
                stream,
                ServerMessage::Context {
                    chat_id,
                    message_id: Some(assistant_message_id),
                    usage,
                },
            )
            .await;
            session.close().await?;
            // Spawn background task to generate assistant message embedding
            if !full_content.trim().is_empty() {
                spawn_message_embedding_task(state.clone(), msg.id, chat_id, full_content.clone());
            }

            // Send message end with the same id MessageStart carried.
            //
            // `msg.id` is that same id, not a new one: `finish` stores the
            // answer under the turn's own id, and the turn is this generation's
            // `message_id`. Said plainly because the comment that used to sit
            // here claimed the database generated it, which is what a caller
            // reading back the answer by id would have been misled by.
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
            publish(stream, end_msg).await;

            tracing::debug!(
                "Assistant message completed: chat_id={}, message_id={}, db_id={}, length={}",
                chat_id,
                assistant_message_id,
                msg.id,
                msg.content.len()
            );
        }
        Err(error) => {
            // The fenced close is refused, and this turn's row is ours alone.
            if matches!(error, crate::db::context::Error::LeaseLost)
                && let Err(refused) = session
                    .settle(Some(&full_content), assistant_metadata, partial.as_ref())
                    .await
            {
                tracing::warn!(%refused, "Could not settle a turn whose lease was lost");
            }
            return Err(format!("Failed to save response: {error}").into());
        }
    }

    Ok(())
}

/// Forgets the agent session the chat's turns resumed, for a turn that moved to a fresh one the
/// chat records once it has an id.
async fn forget(state: &AppState, chat_id: Uuid) {
    if let Err(error) = chats::set_session(state.db(), chat_id, None).await {
        tracing::warn!(%chat_id, %error, "Could not forget the chat's agent session");
    }
}

/// Brings what the organization knows of `login`'s usage up to date once a turn on it has
/// ended, starting from the `windows` the turn observed on it, without holding the answer up for
/// it.
fn settle(state: &AppState, login: Uuid, windows: Vec<abnegate_llm::Window>) {
    let state = state.clone();
    tokio::spawn(async move {
        router::observe(&state, login, &windows).await;
        router::settle(&state, login).await;
    });
}

/// Records `session` as the agent session the chat's turns resume. One that can't be recorded
/// costs the next turn only a replay of the whole transcript.
async fn remember(state: &AppState, chat_id: Uuid, session: Option<chats::ChatSession>) {
    let Some(session) = session else {
        return;
    };
    if let Err(error) = chats::set_session(state.db(), chat_id, Some(&session)).await {
        tracing::warn!(%chat_id, %error, "Could not record the chat's agent session");
    }
}

/// What a reply's source markers resolved to.
struct CitedSources<'a> {
    citations: Vec<Citation>,
    resolved: usize,
    unresolved: Vec<&'a str>,
}

/// Turn the source markers a reply wrote into citations for the sources this
/// chat's registry actually holds.
///
/// Resolution is a lookup, never a recompute. Every marker in the reply is
/// resolved in one query against the registry, so an identifier minted three
/// turns ago still cites: the row is what makes it real, not a digest the
/// server would have to re-derive from an input it no longer has.
///
/// A marker with no row behind it produces no citation, and the reply text is
/// left exactly as the model wrote it. An unresolved marker is a fabricated
/// attribution, and stripping it would leave a confident sentence with nothing
/// visible left to check, which is the worse of the two failures: a marker a
/// reader can see resolves to nothing is evidence of the fabrication. The
/// warning and the counter are what make a fabricating model visible to an
/// operator.
async fn merge_cited_sources(
    state: &AppState,
    chat_id: Uuid,
    reply: &str,
    citations: &mut Vec<Citation>,
) {
    let identifiers = cited_identifiers(reply);
    if identifiers.is_empty() {
        return;
    }

    let sources = match db::chat_sources::resolve(state.db(), chat_id, &identifiers).await {
        Ok(sources) => sources,
        Err(error) => {
            tracing::warn!("Failed to resolve sources cited in chat {chat_id}: {error}");
            return;
        }
    };

    let cited = cited_sources(&identifiers, &sources);
    if !cited.unresolved.is_empty() {
        tracing::warn!(
            "Chat {chat_id} cited {} sources it never retrieved: {}",
            cited.unresolved.len(),
            cited
                .unresolved
                .iter()
                .map(|identifier| agent::identifier::render(identifier))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    crate::metrics::record_citation_markers(crate::metrics::MARKER_RESOLVED, cited.resolved);
    crate::metrics::record_citation_markers(
        crate::metrics::MARKER_UNRESOLVED,
        cited.unresolved.len(),
    );
    agent::citations::merge(citations, cited.citations);
}

/// Every distinct identifier the reply cites, in the order it first appears.
fn cited_identifiers(reply: &str) -> Vec<String> {
    agent::identifier::markers(reply)
        .into_iter()
        .map(|(kind, digest)| agent::identifier::token(kind, &digest))
        .collect()
}

fn cited_sources<'a>(
    identifiers: &'a [String],
    sources: &[db::chat_sources::Source],
) -> CitedSources<'a> {
    let registry: std::collections::HashMap<&str, &db::chat_sources::Source> = sources
        .iter()
        .map(|source| (source.identifier.as_str(), source))
        .collect();

    let mut citations = Vec::with_capacity(identifiers.len());
    let mut unresolved = Vec::new();
    let mut resolved = 0;

    for identifier in identifiers {
        let Some(source) = registry.get(identifier.as_str()) else {
            unresolved.push(identifier.as_str());
            continue;
        };
        resolved += 1;
        if let Some(kind) = citation_kind(source.kind) {
            citations.push(agent::citations::from_source(
                kind,
                &source.identifier,
                &source.title,
                &source.uri,
                source.first_observed_at,
            ));
        }
    }

    CitedSources {
        citations,
        resolved,
        unresolved,
    }
}

/// How a registry source is rendered as a citation.
///
/// Every kind the registry mints has a citation shape, because dropping one
/// here does not just lose a chip: the reply still carries the marker, and the
/// console renders a marker whose citation never arrived as unresolved — a
/// fabrication notice against a source the server did retrieve.
///
/// A chat message mints no registry rows, so it has no shape to render and
/// yields nothing rather than a citation labelled as something it is not.
const fn citation_kind(kind: agent::identifier::Kind) -> Option<agent::CitationKind> {
    match kind {
        agent::identifier::Kind::Web => Some(agent::CitationKind::Web),
        agent::identifier::Kind::Doc => Some(agent::CitationKind::WorkspaceDocument),
        agent::identifier::Kind::Kb => Some(agent::CitationKind::KnowledgePassage),
        agent::identifier::Kind::Chat => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::workspaces;
    use sqlx::PgPool;

    #[test]
    fn retrieved_context_helpers_normalize_bound_and_interleave_sources() {
        assert_eq!(snippet_line("  one\n two   three ", 20), "one two three");
        assert_eq!(snippet_line("one two three", 7), "one two…");
        assert_eq!(snippet_line("åßç", 2), "åß…");
        assert_eq!(
            format_retrieved_line("knowledge", "Runbook", "knowledge://one", "  safe\n text "),
            "- [knowledge] Runbook (knowledge://one): safe text"
        );

        let knowledge = vec!["k1".to_string(), "k2".to_string(), "k3".to_string()];
        let sources = vec!["s1".to_string(), "s2".to_string()];
        assert_eq!(
            interleave_context_lines(knowledge.clone(), sources.clone(), 4),
            ["k1", "s1", "k2", "s2"]
        );
        assert_eq!(
            interleave_context_lines(knowledge, sources, 8),
            ["k1", "s1", "k2", "s2", "k3"]
        );
        assert_eq!(
            interleave_context_lines(vec!["k".into()], vec!["s".into()], 1),
            ["k"]
        );
        assert!(interleave_context_lines(vec!["k".into()], vec!["s".into()], 0).is_empty());
        assert!(interleave_context_lines(Vec::new(), Vec::new(), 5).is_empty());
    }

    /// The capability tail closes each arm here because these four calls pass
    /// no memory. A user with something remembered reads the block after that
    /// tail, which `session`'s own tests pin; what this one holds is that the
    /// persona, the agent contracts and the tail are unchanged by its arrival.
    #[tokio::test]
    async fn system_prompt_preserves_persona_and_agent_contracts() {
        let state = AppState::for_tests();
        let tools = agent::ChatTools::preview(agent::WorkspaceScope {
            state,
            workspace_id: Uuid::new_v4(),
            chat_id: Some(Uuid::new_v4()),
            user_id: Uuid::new_v4(),
            offline: false,
            origin: crate::db::tasks::Origin::Member,
        })
        .await;
        let character = ChatCharacter {
            name: "Ari".into(),
            system_prompt: Some("Stay {{char}}.".into()),
            ..Default::default()
        };
        let environment = agent::prompt::Environment::at(
            chrono::DateTime::parse_from_rfc3339("2026-09-09T09:30:00+12:00").unwrap(),
            "Pacific/Auckland",
            std::path::PathBuf::from("/srv/zone"),
        );
        const CAPABILITY: &str = "Web search is unavailable this turn.";
        const IDENTITY: &str =
            "You are Zone's assistant, answering inside one of the user's workspaces.";

        let persona = session::system_prompt(
            &session::chat_row(Some(character.clone()), false, false),
            &tools,
            false,
            CAPABILITY,
            &environment,
            "",
            "",
        );
        assert!(persona.starts_with("Stay Ari."), "{persona}");
        assert!(persona.ends_with(CAPABILITY), "{persona}");
        assert!(!persona.contains("You can call these tools"), "{persona}");

        let agent = session::system_prompt(
            &session::chat_row(None, true, true),
            &tools,
            true,
            CAPABILITY,
            &environment,
            "",
            "",
        );
        assert!(agent.contains("You can call these tools"), "{agent}");
        assert!(
            agent.contains("without waiting for confirmation"),
            "{agent}"
        );
        assert!(agent.ends_with(CAPABILITY), "{agent}");

        let combined = session::system_prompt(
            &session::chat_row(Some(character), true, false),
            &tools,
            true,
            CAPABILITY,
            &environment,
            "",
            "",
        );
        assert!(combined.starts_with("Stay Ari.\n\n"), "{combined}");
        assert!(
            combined.contains("wait for the user to approve"),
            "{combined}"
        );

        let plain = session::system_prompt(
            &session::chat_row(None, false, false),
            &tools,
            false,
            CAPABILITY,
            &environment,
            "",
            "",
        );
        assert!(plain.contains(IDENTITY), "{plain}");
        assert!(!plain.contains("You can call these tools"), "{plain}");
        assert!(plain.ends_with(CAPABILITY), "{plain}");
    }

    #[tokio::test]
    async fn blank_web_search_requests_do_not_create_a_client() {
        let state = AppState::for_tests();
        assert!(matches!(
            load_web_search(&state, Uuid::new_v4(), " \n\t ", true, false).await,
            SearchContext::Disabled
        ));
    }

    #[tokio::test]
    async fn an_offline_chat_does_not_search_the_web() {
        let state = AppState::for_tests();
        assert!(matches!(
            load_web_search(
                &state,
                Uuid::new_v4(),
                "What is the latest news on OpenAI?",
                true,
                true
            )
            .await,
            SearchContext::Disabled
        ));
    }

    #[tokio::test]
    async fn a_disabled_search_does_not_leave_the_host() {
        let state = AppState::for_tests();
        assert!(matches!(
            load_web_search(
                &state,
                Uuid::new_v4(),
                "What is the latest news on OpenAI?",
                true,
                false
            )
            .await,
            SearchContext::Disabled
        ));
    }

    fn web_hit(title: &str, url: &str) -> SearchHit {
        SearchHit::new(title, url, "")
    }

    fn written(chat_id: Uuid, hit: &SearchHit, identifier: &str) -> chat_sources::Source {
        let now = chrono::Utc::now();
        chat_sources::Source {
            chat_id,
            identifier: identifier.to_string(),
            kind: agent::identifier::Kind::Web,
            key: hit.url.clone(),
            uri: hit.url.clone(),
            title: hit.title.clone(),
            first_observed_at: now,
            last_observed_at: now,
        }
    }

    #[test]
    fn a_hit_carries_the_identifier_the_registry_returned() {
        let chat_id = Uuid::new_v4();
        let mut hit = web_hit("Tide tables", "https://example.com/tides?day=3");

        let minted = agent::identifier::mint(agent::identifier::Kind::Web, &hit.url);
        let extended = agent::identifier::extend(&minted, &hit.url)
            .expect("a freshly minted identifier can still be lengthened");
        assert_ne!(
            minted, extended,
            "the fixture must differ from a local mint, or it could not tell the two apart"
        );

        let source = written(chat_id, &hit, &extended);
        stamp(chat_id, &mut hit, Ok(source));

        assert_eq!(
            hit.identifier.as_deref(),
            Some(extended.as_str()),
            "the hit must carry the identifier the write returned, not one recomputed here"
        );
    }

    #[test]
    fn a_failed_registry_write_leaves_only_that_hit_bare() {
        let chat_id = Uuid::new_v4();
        let mut hits = vec![
            web_hit("Reachable", "https://example.com/one"),
            web_hit("Unwritable", "https://example.com/two"),
        ];
        let identifier = agent::identifier::mint(agent::identifier::Kind::Web, &hits[0].url);

        let source = written(chat_id, &hits[0], &identifier);
        stamp(chat_id, &mut hits[0], Ok(source));
        stamp(chat_id, &mut hits[1], Err(sqlx::Error::PoolClosed));

        assert_eq!(
            hits[0].identifier.as_deref(),
            Some(identifier.as_str()),
            "a neighbouring write that succeeded still stamps its own hit"
        );
        assert_eq!(
            hits[1].identifier, None,
            "a hit the registry rejected must stay bare rather than cite what cannot resolve"
        );

        let prompt = SearchContext::Results(hits).prompt();
        assert!(
            prompt.contains("Search outcome for this turn: succeeded"),
            "a failed write must not downgrade the turn's search outcome: {prompt}"
        );
        assert!(
            prompt.contains(&format!("[{identifier}]")),
            "the identified hit is cited by identifier: {prompt}"
        );
        assert!(
            prompt.contains("2. Unwritable"),
            "the bare hit keeps a positional ordinal: {prompt}"
        );
    }

    #[tokio::test]
    async fn an_unreachable_registry_leaves_every_hit_bare_without_failing_the_turn() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(250))
            .connect_lazy("postgres://postgres@127.0.0.1:1/zone_absent")
            .expect("a lazy pool needs no server");
        let mut hits = vec![
            web_hit("First", "https://example.com/one"),
            web_hit("Second", "https://example.com/two"),
        ];

        identify(&pool, Uuid::new_v4(), &mut hits).await;

        assert!(
            hits.iter().all(|hit| hit.identifier.is_none()),
            "no hit may carry an identifier the registry never stored: {hits:?}"
        );
        assert_eq!(hits.len(), 2, "an unwritable registry drops no hit");
        assert!(
            SearchContext::Results(hits)
                .prompt()
                .contains("Search outcome for this turn: succeeded"),
            "the search context still reports the results it retrieved"
        );
    }

    #[tokio::test]
    async fn unavailable_history_degrades_image_reuse_to_no_source() {
        let state = AppState::for_tests();
        let store = crate::services::artifacts::ArtifactStore::new(std::env::temp_dir());

        assert!(
            resolve_generation_source(
                &state,
                Uuid::new_v4(),
                Uuid::new_v4(),
                "Change the background to a forest",
                None,
                &store,
            )
            .await
            .unwrap()
            .is_none()
        );
    }

    mod routing {
        use super::*;
        use crate::config::{AgentConfig, Config, ModelBackend};
        use crate::db::ai_settings::PROVIDER_CLAUDE_CODE;
        use crate::db::chats::provider::Choice;
        use crate::services::provider::Provider;
        use crate::services::stages::AUTO;
        use crate::services::stages::testing::{MODEL_FLAG, StandIn};
        use abnegate_agent_cli::AgentKind;
        use tempfile::TempDir;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;

        const CATALOG: &str = "/api/tags";
        const QUESTION: &str = "What is a monad?";
        const FAST: &str = "haiku";

        /// An organization on `provider`, with a workspace and a chat left on
        /// automatic model selection.
        struct Organization {
            pool: PgPool,
            id: Uuid,
            workspace: Uuid,
            chat: Uuid,
        }

        impl Organization {
            async fn on(provider: &str) -> Self {
                let pool = PgPool::connect(
                    &std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL"),
                )
                .await
                .expect("the test database");
                let suffix = Uuid::new_v4().simple().to_string();
                let organization =
                    db::organizations::create_organization(&pool, "Routing", &suffix, None)
                        .await
                        .expect("an organization");
                let workspace =
                    workspaces::create_workspace(&pool, organization.id, "Routing", &suffix, None)
                        .await
                        .expect("a workspace");
                sqlx::query(
                    "INSERT INTO organization_ai_settings (organization_id, provider) VALUES ($1, $2)",
                )
                .bind(organization.id)
                .bind(provider)
                .execute(&pool)
                .await
                .expect("the organization's AI settings");
                let chat =
                    chats::create_chat(&pool, Some(workspace.id), "Routing", AUTO, false, false)
                        .await
                        .expect("a chat");
                Self {
                    pool,
                    id: organization.id,
                    workspace: workspace.id,
                    chat: chat.id,
                }
            }

            /// Sets the chat on `provider` and `model`, or back on its workspace's.
            async fn choose(&self, provider: Option<Provider>, model: Option<&str>) {
                let choice = provider.map(|provider| Choice {
                    provider,
                    model: model.map(str::to_string),
                });
                chats::provider::set(&self.pool, self.chat, choice.as_ref())
                    .await
                    .expect("the chat's provider");
            }

            /// Deletes the chat first, which waits for whatever its last turn is still writing,
            /// and then the organization.
            async fn remove(&self) {
                chats::delete_chat(&self.pool, self.chat)
                    .await
                    .expect("the chat to be removed");
                sqlx::query("DELETE FROM organizations WHERE id = $1")
                    .bind(self.id)
                    .execute(&self.pool)
                    .await
                    .expect("the organization to be removed");
            }
        }

        #[tokio::test]
        async fn a_turn_that_routes_no_media_reads_no_model_catalog() {
            let ollama = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path(CATALOG))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"models": []})),
                )
                .mount(&ollama)
                .await;
            let organization = Organization::on(PROVIDER_SELF_HOSTED).await;
            let state = AppState::new(
                Config {
                    ollama_host: ollama.uri(),
                    ..crate::state::test_config()
                },
                organization.pool.clone(),
                None,
            );

            let routing = prepare_message(
                &state,
                organization.chat,
                organization.workspace,
                QUESTION,
                None,
            )
            .await;
            organization.remove().await;

            assert!(matches!(routing, Ok(Routing::Chat(..))));
            let reads = ollama
                .received_requests()
                .await
                .expect("the requests Ollama received")
                .iter()
                .filter(|request| request.url.path() == CATALOG)
                .count();
            assert_eq!(
                reads, 0,
                "a turn that routes no media read the model catalog"
            );
        }

        /// `prepare_message` and `prepare_chat` for one turn on `state`.
        async fn prepare_turn(
            state: &AppState,
            organization: &Organization,
        ) -> Result<ChatPreparation, String> {
            let routing = prepare_message(
                state,
                organization.chat,
                organization.workspace,
                QUESTION,
                None,
            )
            .await
            .map_err(|error| error.to_string())?;
            let Routing::Chat(chat, route) = routing else {
                return Err("the turn routed to media".to_string());
            };
            prepare_chat(
                state,
                &ChatStream::of(organization.chat),
                organization.chat,
                organization.workspace,
                Uuid::new_v4(),
                QUESTION,
                None,
                chat,
                *route,
                false,
            )
            .await
            .map_err(|error| error.to_string())
        }

        const UNREACHABLE: &str = "http://127.0.0.1:9";

        #[tokio::test]
        async fn a_turn_reads_its_workspaces_ai_settings_once() {
            let organization = Organization::on(PROVIDER_SELF_HOSTED).await;
            let state = AppState::new(
                Config {
                    litellm_host: UNREACHABLE.to_string(),
                    ollama_host: UNREACHABLE.to_string(),
                    ..crate::state::test_config()
                },
                organization.pool.clone(),
                None,
            );

            let prepared = prepare_turn(&state, &organization).await;
            organization.remove().await;

            if let Err(error) = prepared {
                panic!("the turn was not prepared: {error}");
            }
            assert_eq!(
                crate::services::route::reads::of(organization.workspace),
                1,
                "one turn read its workspace's AI settings more than once"
            );
        }

        #[tokio::test]
        async fn a_turn_on_a_chosen_provider_reads_its_workspaces_ai_settings_once() {
            let organization = Organization::on(PROVIDER_CLAUDE_CODE).await;
            organization.choose(Some(Provider::SelfHosted), None).await;
            let state = AppState::new(
                Config {
                    litellm_host: UNREACHABLE.to_string(),
                    ollama_host: UNREACHABLE.to_string(),
                    ..crate::state::test_config()
                },
                organization.pool.clone(),
                None,
            );

            let prepared = prepare_turn(&state, &organization).await;
            organization.remove().await;

            let preparation =
                prepared.unwrap_or_else(|error| panic!("the turn was not prepared: {error}"));
            assert!(
                matches!(preparation.llm.backend(), LlmBackend::Http),
                "the chat ran on its workspace's agent rather than the provider it chose"
            );
            assert_eq!(
                crate::services::route::reads::of(organization.workspace),
                1,
                "one turn on a chosen provider read its workspace's AI settings more than once"
            );
        }

        const WINDOW: i64 = 8192;

        /// The context limit of one turn of `organization`'s chat once it keeps
        /// [`WINDOW`] for its workspace's provider.
        async fn windowed_limit(organization: &Organization) -> Option<u64> {
            chats::update_chat(
                &organization.pool,
                organization.chat,
                None,
                None,
                None,
                None,
                None,
                Some(WINDOW),
                None,
            )
            .await
            .expect("the chat's context window");
            let state = AppState::new(
                Config {
                    litellm_host: UNREACHABLE.to_string(),
                    ollama_host: UNREACHABLE.to_string(),
                    ..crate::state::test_config()
                },
                organization.pool.clone(),
                None,
            );

            let prepared = prepare_turn(&state, organization).await;
            organization.remove().await;

            prepared
                .unwrap_or_else(|error| panic!("the turn was not prepared: {error}"))
                .context
                .policy
                .limit
        }

        #[tokio::test]
        async fn a_chat_on_its_workspaces_provider_keeps_its_context_window() {
            let organization = Organization::on(PROVIDER_SELF_HOSTED).await;

            let limit = windowed_limit(&organization).await;

            assert_eq!(limit, u64::try_from(WINDOW).ok());
        }

        #[tokio::test]
        async fn a_chat_on_a_coding_agent_is_not_held_to_a_window_it_keeps() {
            let organization = Organization::on(PROVIDER_CLAUDE_CODE).await;

            let limit = windowed_limit(&organization).await;

            assert_eq!(
                limit, None,
                "an agent's window is unknown until it reports one, whatever the chat keeps"
            );
        }

        #[tokio::test]
        async fn a_chosen_provider_is_not_held_to_the_window_kept_for_its_workspaces_provider() {
            let organization = Organization::on(PROVIDER_CLAUDE_CODE).await;
            organization.choose(Some(Provider::SelfHosted), None).await;

            let limit = windowed_limit(&organization).await;

            assert_ne!(
                limit,
                u64::try_from(WINDOW).ok(),
                "a chosen provider was held to the window the chat keeps for its workspace's provider"
            );
        }

        #[tokio::test]
        async fn a_chat_pinned_to_an_instance_model_runs_on_a_model_the_endpoint_settings_save() {
            const SAVED: &str = "gpt-4o-mini";
            let ollama = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/api/show"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({"capabilities": ["embedding"]})),
                )
                .mount(&ollama)
                .await;
            let organization = Organization::on(PROVIDER_SELF_HOSTED).await;
            sqlx::query(
                "UPDATE organization_ai_settings SET litellm_host = $2, model_fast = $3, \
                 completions_routed = true WHERE organization_id = $1",
            )
            .bind(organization.id)
            .bind("http://gateway.example:4000")
            .bind(SAVED)
            .execute(&organization.pool)
            .await
            .expect("the organization's endpoint");
            sqlx::query("UPDATE chats SET model_name = 'llama3.2:3b' WHERE id = $1")
                .bind(organization.chat)
                .execute(&organization.pool)
                .await
                .expect("the chat's pinned model");
            let state = AppState::new(
                Config {
                    litellm_host: UNREACHABLE.to_string(),
                    ollama_host: ollama.uri(),
                    ..crate::state::test_config()
                },
                organization.pool.clone(),
                None,
            );

            let prepared = prepare_turn(&state, &organization).await;
            organization.remove().await;

            let preparation = prepared.unwrap_or_else(|error| {
                panic!("the turn on the saved endpoint was not prepared: {error}")
            });
            assert_eq!(preparation.endpoint.origin(), Origin::Settings);
            assert_eq!(
                preparation.model, SAVED,
                "the instance's model name was sent to the saved endpoint"
            );
            let shown = ollama
                .received_requests()
                .await
                .expect("the requests Ollama received")
                .len();
            assert_eq!(
                shown, 0,
                "the instance's Ollama was asked about a model the saved endpoint runs"
            );
            assert_eq!(
                preparation.context.policy.limit,
                Some(state.config().chat.context),
                "the saved endpoint's turn has no context limit to compact against"
            );
        }

        #[tokio::test]
        async fn a_turn_that_routes_no_media_leaves_the_organizations_agent_unresolved() {
            let organization = Organization::on(PROVIDER_CLAUDE_CODE).await;
            let agents = TempDir::new().expect("an agent state root");
            let state = AppState::new(
                Config {
                    agents: AgentConfig {
                        state: agents.path().to_path_buf(),
                        host_login: true,
                        ..crate::state::test_agents()
                    },
                    ..crate::state::test_config()
                },
                organization.pool.clone(),
                None,
            );

            let routing = prepare_message(
                &state,
                organization.chat,
                organization.workspace,
                QUESTION,
                None,
            )
            .await;
            organization.remove().await;

            assert!(matches!(routing, Ok(Routing::Chat(..))));
            assert!(
                !agents.path().join(organization.id.to_string()).exists(),
                "a turn that routes no media resolved the organization's agent"
            );
        }

        /// An organization on Claude Code with `haiku` as its Fast model,
        /// signed in through the host, and media routing on.
        async fn routing_media_on_claude(
            executable: Option<std::path::PathBuf>,
        ) -> (Organization, TempDir, AppState) {
            let organization = Organization::on(PROVIDER_CLAUDE_CODE).await;
            sqlx::query(
                "UPDATE organization_ai_settings SET model_fast = $2 WHERE organization_id = $1",
            )
            .bind(organization.id)
            .bind(FAST)
            .execute(&organization.pool)
            .await
            .expect("the organization's Fast model");
            let agents = TempDir::new().expect("an agent state root");
            let state = AppState::new(
                Config {
                    model_backend: ModelBackend::Cli {
                        agent: AgentKind::Claude,
                        executable,
                    },
                    agents: AgentConfig {
                        state: agents.path().to_path_buf(),
                        host_login: true,
                        ..crate::state::test_agents()
                    },
                    comfyui: {
                        let mut comfyui = crate::services::comfy::settings::development();
                        comfyui.enabled = true;
                        comfyui.classifier_timeout = Duration::from_secs(20);
                        comfyui
                    },
                    ..crate::state::test_config()
                },
                organization.pool.clone(),
                None,
            );
            (organization, agents, state)
        }

        #[tokio::test]
        async fn an_image_route_carries_the_backend_it_was_resolved_on() {
            let (organization, _agents, state) = routing_media_on_claude(None).await;

            let routing = prepare_message(
                &state,
                organization.chat,
                organization.workspace,
                "generate an image of a lighthouse at dusk",
                None,
            )
            .await;
            organization.remove().await;

            let Ok(Routing::Image(
                config,
                LlmBackend::Cli {
                    agent, settings, ..
                },
                _,
            )) = routing
            else {
                panic!("expected an image route carrying the organization's agent");
            };
            assert_eq!(agent, AgentKind::Claude);
            assert_eq!(config.classifier_model, FAST);
            assert_eq!(
                settings.working_directory,
                Some(
                    state
                        .config()
                        .agents
                        .work(organization.id, AgentKind::Claude)
                )
            );
        }

        /// A Claude login of the organization on `access`, with `headroom` percent of its usage
        /// left.
        async fn claude_login(
            state: &AppState,
            organization: &Organization,
            access: &str,
            headroom: f64,
        ) -> Uuid {
            let tokens = abnegate_agent_cli::login::claude::Tokens::new(
                abnegate_secret::SecretValue::new(access),
                None,
                chrono::Utc::now() + chrono::TimeDelta::hours(8),
                None,
                "user:inference",
                None,
            );
            let sealed = crate::services::login::claude::Sealed::seal(&tokens, state.master_key())
                .expect("tokens to seal");
            let login = db::agent_logins::insert(
                &organization.pool,
                &db::agent_logins::Insert {
                    organization_id: organization.id,
                    agent: AgentKind::Claude.as_str(),
                    account: Some(access),
                    credential: Some(&sealed),
                    label: Some(access),
                    expires_at: None,
                },
            )
            .await
            .expect("a stored login")
            .id;
            db::agent_logins::observe(
                &organization.pool,
                login,
                &crate::services::login::usage::Snapshot {
                    windows: Vec::new(),
                    headroom: Some(headroom),
                    fetched_at: chrono::Utc::now(),
                },
            )
            .await
            .expect("the login's usage");
            login
        }

        /// The token the classifier of an image turn on `organization`'s chat runs with.
        async fn classified_on(state: &AppState, organization: &Organization) -> Option<String> {
            let routing = prepare_message(
                state,
                organization.chat,
                organization.workspace,
                "generate an image of a lighthouse at dusk",
                None,
            )
            .await;
            let Ok(Routing::Image(_, LlmBackend::Cli { settings, .. }, _)) = routing else {
                panic!("expected an image route carrying the organization's agent");
            };
            settings.credential.expose().map(str::to_string)
        }

        /// A Codex login of the organization, signed in within its own home.
        async fn codex_login(state: &AppState, organization: &Organization) -> Uuid {
            let login = db::agent_logins::insert(
                &organization.pool,
                &db::agent_logins::Insert {
                    organization_id: organization.id,
                    agent: AgentKind::Codex.as_str(),
                    account: Some("codex"),
                    credential: None,
                    label: Some("codex"),
                    expires_at: None,
                },
            )
            .await
            .expect("a stored login")
            .id;
            db::agent_logins::observe(
                &organization.pool,
                login,
                &crate::services::login::usage::Snapshot {
                    windows: Vec::new(),
                    headroom: Some(90.0),
                    fetched_at: chrono::Utc::now(),
                },
            )
            .await
            .expect("the login's usage");
            let home = state
                .config()
                .agents
                .create_login_home(organization.id, AgentKind::Codex, login)
                .expect("the login's home");
            std::fs::write(
                home.join("auth.json"),
                r#"{"tokens":{"access_token":"codex-access","refresh_token":"codex-refresh","account_id":"acct-42"}}"#,
            )
            .expect("the codex sign-in");
            login
        }

        #[tokio::test]
        async fn a_chat_that_chose_claude_code_never_runs_its_turn_or_classifier_on_codex() {
            let organization = Organization::on(PROVIDER_CLAUDE_CODE).await;
            let agents = TempDir::new().expect("an agent state root");
            let state = AppState::new(
                Config {
                    agents: AgentConfig {
                        state: agents.path().to_path_buf(),
                        host_login: false,
                        ..crate::state::test_agents()
                    },
                    comfyui: {
                        let mut comfyui = crate::services::comfy::settings::development();
                        comfyui.enabled = true;
                        comfyui.classifier_timeout = Duration::from_secs(20);
                        comfyui
                    },
                    ..crate::state::test_config()
                },
                organization.pool.clone(),
                None,
            );
            codex_login(&state, &organization).await;
            let signed_out = crate::services::backend::Error::SignedOut {
                agent: AgentKind::Claude,
            }
            .to_string();

            let following = prepare_turn(&state, &organization).await;
            organization.choose(Some(Provider::ClaudeCode), None).await;
            let turn = prepare_turn(&state, &organization).await;
            let classified = prepare_message(
                &state,
                organization.chat,
                organization.workspace,
                "generate an image of a lighthouse at dusk",
                None,
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string());
            organization.remove().await;

            let following = following.unwrap_or_else(|error| {
                panic!("a chat following its workspace lost its fallback to codex: {error}")
            });
            assert!(
                matches!(
                    following.llm.backend(),
                    LlmBackend::Cli {
                        agent: AgentKind::Codex,
                        ..
                    }
                ),
                "a chat following its workspace did not fall back to codex"
            );
            assert_eq!(
                turn.err().as_deref(),
                Some(signed_out.as_str()),
                "the turn of a chat that chose Claude Code ran on codex"
            );
            assert_eq!(
                classified.err().as_deref(),
                Some(signed_out.as_str()),
                "the classifier of a chat that chose Claude Code ran on codex or the instance"
            );
        }

        #[tokio::test]
        async fn classifying_runs_on_the_chats_own_login() {
            const TIGHT: &str = "tight-access";
            const ROOMY: &str = "roomy-access";
            let (organization, _agents, state) = routing_media_on_claude(None).await;
            let tight = claude_login(&state, &organization, TIGHT, 10.0).await;
            let roomy = claude_login(&state, &organization, ROOMY, 90.0).await;

            let unsessioned = classified_on(&state, &organization).await;
            db::chats::set_session(
                &organization.pool,
                organization.chat,
                Some(&db::chats::ChatSession {
                    login: Some(tight),
                    id: Uuid::new_v4().to_string(),
                    agent: AgentKind::Claude,
                    entry: 0,
                    prompt: None,
                }),
            )
            .await
            .expect("the chat's session");
            let sessioned = classified_on(&state, &organization).await;
            let mut used = Vec::new();
            for login in [tight, roomy] {
                used.push(
                    db::agent_logins::get(&organization.pool, login)
                        .await
                        .expect("the login to be readable")
                        .expect("the login to be there")
                        .last_used_at,
                );
            }
            organization.remove().await;

            assert_eq!(unsessioned.as_deref(), Some(ROOMY));
            assert_eq!(
                sessioned.as_deref(),
                Some(TIGHT),
                "the classifier left the login the chat runs on"
            );
            assert_eq!(used, [None, None], "classifying recorded a login as used");
        }

        mod sessions {
            use super::*;
            use crate::db::workspace_members::WorkspaceRole;

            const FIRST: &str = "Where is the deploy checklist?";
            const SECOND: &str = "Who last changed it?";
            const ANSWER: &str = "In the Operations space.";
            const RUN: char = '\u{1e}';
            const REFUSE: &str = "refuse";
            const SCRIPT: &str = r#"#!/bin/sh
[ "$#" -gt 0 ] || exit 0
session=''
refused=''
previous=''
for argument in "$@"; do
  case "$previous" in
    --session-id) session="$argument" ;;
    --resume) session="$argument"; [ -e 'DIR/refuse' ] && refused=1 ;;
  esac
  previous="$argument"
done
cat >> 'DIR/stdin'
printf '\036' >> 'DIR/stdin'
pwd -P >> 'DIR/directories'
printf '\036' >> 'DIR/directories'
printf '%s\0' "$@" >> 'DIR/arguments'
printf '\036' >> 'DIR/arguments'
if [ -n "$refused" ]; then
  echo "No conversation found with session ID: $session" >&2
  exit 1
fi
printf '{"type":"system","subtype":"init","session_id":"%s"}\n' "$session"
cat 'DIR/reply.jsonl'
"#;

            /// A claude that records the arguments and the prompt of every
            /// run, announces the session it was pinned to or asked to resume,
            /// and refuses to resume any while `refuse` exists.
            struct Agent {
                directory: TempDir,
                executable: std::path::PathBuf,
            }

            /// One run of the agent that ran a chat turn: one pinned to a session.
            struct Run {
                arguments: Vec<String>,
                stdin: String,
                directory: String,
            }

            impl Run {
                fn flag(&self, flag: &str) -> Option<&str> {
                    self.arguments
                        .iter()
                        .position(|argument| argument == flag)
                        .and_then(|index| self.arguments.get(index + 1))
                        .map(String::as_str)
                }
            }

            impl Agent {
                fn new() -> Self {
                    use std::os::unix::fs::PermissionsExt;

                    let directory = TempDir::new().expect("a directory for the stand-in claude");
                    let reply = [
                        serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": ANSWER}]}}),
                        serde_json::json!({"type": "result", "subtype": "success", "is_error": false}),
                    ]
                    .map(|event| event.to_string())
                    .join("\n");
                    std::fs::write(directory.path().join("reply.jsonl"), format!("{reply}\n"))
                        .expect("the stand-in's reply");
                    let executable = directory.path().join("claude");
                    std::fs::write(
                        &executable,
                        SCRIPT.replace("DIR", &directory.path().display().to_string()),
                    )
                    .expect("the stand-in claude");
                    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))
                        .expect("the stand-in claude to be executable");
                    crate::services::stages::testing::wait_until_executable(&executable);
                    Self {
                        directory,
                        executable,
                    }
                }

                fn refuse(&self) {
                    std::fs::write(self.directory.path().join(REFUSE), "")
                        .expect("the refusal marker");
                }

                fn recorded(&self, name: &str) -> Vec<String> {
                    std::fs::read(self.directory.path().join(name))
                        .map(|recorded| {
                            String::from_utf8_lossy(&recorded)
                                .split_terminator(RUN)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default()
                }

                /// The runs that served a chat turn, in order: a title or a
                /// classifier runs in no session of the chat's.
                fn runs(&self) -> Vec<Run> {
                    self.recorded("arguments")
                        .into_iter()
                        .zip(self.recorded("stdin"))
                        .zip(self.recorded("directories"))
                        .map(|((arguments, stdin), directory)| Run {
                            arguments: arguments
                                .split_terminator('\0')
                                .map(str::to_string)
                                .collect(),
                            stdin: zone_core::llm::cli::prompt(&stdin),
                            directory: directory.trim().to_string(),
                        })
                        .filter(|run| run.flag("--session-id").or(run.flag("--resume")).is_some())
                        .collect()
                }
            }

            struct Chat {
                organization: Organization,
                state: AppState,
                agent: Agent,
                login: Uuid,
                user: Uuid,
                _agents: TempDir,
            }

            impl Chat {
                async fn on_claude() -> Self {
                    Self::cloning_under(std::env::temp_dir().join("zone-clones")).await
                }

                async fn cloning_under(clones: std::path::PathBuf) -> Self {
                    let organization = Organization::on(PROVIDER_CLAUDE_CODE).await;
                    let agent = Agent::new();
                    let agents = TempDir::new().expect("an agent state root");
                    let state = AppState::new(
                        Config {
                            litellm_host: UNREACHABLE.to_string(),
                            ollama_host: UNREACHABLE.to_string(),
                            model_backend: ModelBackend::Cli {
                                agent: AgentKind::Claude,
                                executable: Some(agent.executable.clone()),
                            },
                            agents: AgentConfig {
                                state: agents.path().to_path_buf(),
                                ..crate::state::test_agents()
                            },
                            clones,
                            ..crate::state::test_config()
                        },
                        organization.pool.clone(),
                        None,
                    );
                    let login = claude_login(&state, &organization, "session-access", 50.0).await;
                    let user = db::users::create_user(
                        &organization.pool,
                        &format!("{}@sessions.test", Uuid::new_v4().simple()),
                        "not-a-real-hash",
                        None,
                        false,
                    )
                    .await
                    .expect("a user")
                    .id;
                    db::workspace_members::add_member(
                        &organization.pool,
                        organization.workspace,
                        user,
                        WorkspaceRole::Member,
                        None,
                    )
                    .await
                    .expect("a member of the workspace");
                    Self {
                        organization,
                        state,
                        agent,
                        login,
                        user,
                        _agents: agents,
                    }
                }

                /// Runs one turn on `content` and returns the id of its answer.
                async fn ask(&self, content: &str) -> Uuid {
                    run_turn(
                        &self.state,
                        self.organization.chat,
                        self.organization.workspace,
                        self.user,
                        content,
                        None,
                    )
                    .await
                }

                /// Puts the chat in a new project whose source is the folder `path`.
                async fn in_project(&self, path: &std::path::Path) {
                    self.sourced(
                        db::sources::FILESYSTEM,
                        serde_json::json!({"base_path": path}),
                    )
                    .await;
                }

                /// Puts the chat in a new project whose source is `kind` with `config`.
                async fn sourced(&self, kind: &str, config: serde_json::Value) {
                    let pool = &self.organization.pool;
                    let source = db::sources::create_source(
                        pool,
                        db::sources::NewSource {
                            workspace_id: self.organization.workspace,
                            name: &Uuid::new_v4().to_string(),
                            source_type: kind,
                            config,
                            description: None,
                            url: None,
                            credentials_encrypted: None,
                            github_installation_id: None,
                        },
                    )
                    .await
                    .expect("a source");
                    let project = db::projects::create(
                        pool,
                        db::projects::NewProject {
                            name: "Project",
                            description: None,
                            workspace_id: Some(self.organization.workspace),
                            source_id: Some(source.id),
                            status: None,
                        },
                    )
                    .await
                    .expect("a project");
                    sqlx::query("UPDATE chats SET project_id = $1 WHERE id = $2")
                        .bind(project.id)
                        .bind(self.organization.chat)
                        .execute(pool)
                        .await
                        .expect("the chat in the project");
                }

                /// Turns the chat's agent on, which is what a chat needs a directory for.
                async fn with_agent(&self) {
                    sqlx::query("UPDATE chats SET agent_enabled = true WHERE id = $1")
                        .bind(self.organization.chat)
                        .execute(&self.organization.pool)
                        .await
                        .expect("the chat's agent on");
                }

                fn home(&self) -> std::path::PathBuf {
                    self.state.config().agents.login_home(
                        self.organization.id,
                        AgentKind::Claude,
                        self.login,
                    )
                }

                fn work(&self) -> std::path::PathBuf {
                    std::fs::canonicalize(
                        self.state
                            .config()
                            .agents
                            .work(self.organization.id, AgentKind::Claude),
                    )
                    .expect("the working directory")
                }

                /// The file the real claude keeps session `id` in, for turns run in `directory`.
                fn transcript(&self, directory: &std::path::Path, id: &str) -> std::path::PathBuf {
                    self.home()
                        .join("projects")
                        .join(abnegate_agent_cli::Session::project(directory))
                        .join(format!("{id}.jsonl"))
                }

                async fn answer(&self) -> String {
                    sqlx::query_scalar(
                        "SELECT content FROM messages WHERE chat_id = $1 AND role = 'assistant' \
                         ORDER BY created_at DESC LIMIT 1",
                    )
                    .bind(self.organization.chat)
                    .fetch_one(&self.organization.pool)
                    .await
                    .expect("the chat's answer")
                }

                async fn session(&self) -> Option<chats::ChatSession> {
                    chats::session(&self.organization.pool, self.organization.chat)
                        .await
                        .expect("the chat's session")
                }

                async fn latest(&self) -> i64 {
                    crate::db::context::Store::new(
                        self.organization.pool.clone(),
                        self.organization.chat,
                        Some(self.organization.workspace),
                    )
                    .latest()
                    .await
                    .expect("the chat's latest entry")
                }

                /// Sets the chat on `provider` and `model`, or back on its workspace's.
                async fn choose(&self, provider: Option<Provider>, model: Option<&str>) {
                    self.organization.choose(provider, model).await;
                }

                async fn remove(&self) {
                    self.organization.remove().await;
                    sqlx::query("DELETE FROM users WHERE id = $1")
                        .bind(self.user)
                        .execute(&self.organization.pool)
                        .await
                        .expect("the user to be removed");
                }
            }

            /// The blocks a rendered prompt is made of: each label, and what it says.
            fn blocks(stdin: &str) -> Vec<(String, String)> {
                let mut blocks: Vec<(String, String)> = Vec::new();
                for part in stdin.split("\n\n") {
                    match part.split_once(":\n").filter(|(label, _)| {
                        ["System", "User", "Assistant", "Tool"].contains(label)
                    }) {
                        Some((label, said)) => blocks.push((label.to_string(), said.to_string())),
                        None => {
                            if let Some((_, said)) = blocks.last_mut() {
                                said.push_str("\n\n");
                                said.push_str(part);
                            }
                        }
                    }
                }
                blocks
            }

            #[tokio::test]
            async fn a_resumed_chat_turn_sends_its_agent_only_what_the_session_has_not_seen() {
                let chat = Chat::on_claude().await;

                chat.ask(FIRST).await;
                let first = chat.session().await;
                chat.ask(SECOND).await;
                let second = chat.session().await;
                let latest = chat.latest().await;
                let runs = chat.agent.runs();
                chat.remove().await;

                let [opening, resumed] = runs.as_slice() else {
                    panic!("expected two sessioned runs, got {}", runs.len());
                };
                let pinned = opening
                    .flag("--session-id")
                    .expect("the first turn pinned a session");
                assert_eq!(opening.flag("--resume"), None);
                assert!(opening.stdin.starts_with("System:\n"), "{}", opening.stdin);
                assert!(
                    opening.stdin.contains(&format!("User:\n{FIRST}")),
                    "{}",
                    opening.stdin
                );

                let first = first.expect("the first turn recorded its session");
                assert_eq!(first.id, pinned);
                assert_eq!(first.login, Some(chat.login));
                assert_eq!(first.agent, AgentKind::Claude);
                assert_eq!(first.prompt.as_ref().map(String::len), Some(64));

                assert_eq!(resumed.flag("--resume"), Some(pinned));
                assert_eq!(resumed.flag("--session-id"), None);
                let sent = blocks(&resumed.stdin);
                let labels: Vec<&str> = sent.iter().map(|(label, _)| label.as_str()).collect();
                assert_eq!(labels, ["System", "User"], "{}", resumed.stdin);
                assert!(
                    sent[0].1.starts_with(crate::agent::prompt::NOW),
                    "{}",
                    sent[0].1
                );
                assert_eq!(
                    sent[0].1.lines().count(),
                    1,
                    "an unchanged prompt was sent again: {}",
                    sent[0].1
                );
                assert_eq!(sent[1].1, SECOND);

                let second = second.expect("the second turn kept its session");
                assert_eq!(second.id, pinned);
                assert_eq!(second.prompt, first.prompt);
                assert!(second.entry > first.entry);
                assert_eq!(second.entry, latest);
            }

            const THIRD: &str = "And before that?";
            const ELSEWHERE: &str = "Answered by the chosen endpoint.";
            const CHOSEN: &str = "gpt-chosen";

            /// An OpenAI-compatible endpoint that answers every completion with `ELSEWHERE`.
            async fn endpoint() -> MockServer {
                let server = MockServer::start().await;
                let chunk = serde_json::json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":CHOSEN,"choices":[{"index":0,"delta":{"content":ELSEWHERE},"finish_reason":null}]});
                let end = serde_json::json!({"id":"completion","object":"chat.completion.chunk","created":0,"model":CHOSEN,"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
                Mock::given(method("POST"))
                    .respond_with(
                        ResponseTemplate::new(200)
                            .insert_header("Content-Type", "text/event-stream")
                            .set_body_string(format!(
                                "data: {chunk}\n\ndata: {end}\n\ndata: [DONE]\n\n"
                            )),
                    )
                    .mount(&server)
                    .await;
                server
            }

            #[tokio::test]
            async fn a_chat_switched_off_its_agent_and_back_resumes_with_the_turns_it_missed() {
                let chat = Chat::on_claude().await;
                let server = endpoint().await;
                sqlx::query(
                    "UPDATE organization_ai_settings SET openai_base_url = $2, completions_routed = true \
                     WHERE organization_id = $1",
                )
                .bind(chat.organization.id)
                .bind(format!("{}/v1", server.uri()))
                .execute(&chat.organization.pool)
                .await
                .expect("the organization's OpenAI endpoint");

                chat.ask(FIRST).await;
                let first = chat
                    .session()
                    .await
                    .expect("the first turn recorded its session");
                chat.choose(Some(Provider::OpenAI), Some(CHOSEN)).await;
                chat.ask(SECOND).await;
                let elsewhere = chat.agent.runs().len();
                let sent: Vec<serde_json::Value> = server
                    .received_requests()
                    .await
                    .expect("the requests the endpoint received")
                    .iter()
                    .map(|request| request.body_json().expect("a JSON completion"))
                    .collect();
                chat.choose(Some(Provider::ClaudeCode), None).await;
                chat.ask(THIRD).await;
                let latest = chat.latest().await;
                let back = chat.session().await;
                let runs = chat.agent.runs();
                let answers: Vec<String> = sqlx::query_scalar(
                    "SELECT content FROM messages WHERE chat_id = $1 AND role = 'assistant' ORDER BY created_at",
                )
                .bind(chat.organization.chat)
                .fetch_all(&chat.organization.pool)
                .await
                .expect("the chat's answers");
                chat.remove().await;

                assert_eq!(elsewhere, 1, "the turn on the chosen endpoint ran claude");
                assert_eq!(answers, [ANSWER, ELSEWHERE, ANSWER]);
                assert!(
                    sent.iter()
                        .any(|body| body["model"] == CHOSEN && body.to_string().contains(SECOND)),
                    "the chosen endpoint was not asked the second question on its model: {sent:?}"
                );
                let [_, resumed] = runs.as_slice() else {
                    panic!("expected two sessioned runs, got {}", runs.len());
                };
                let sent = blocks(&resumed.stdin);
                match resumed.flag("--resume") {
                    Some(id) => {
                        assert_eq!(id, first.id);
                        let said: Vec<(&str, &str)> = sent
                            .iter()
                            .skip(1)
                            .map(|(label, said)| (label.as_str(), said.as_str()))
                            .collect();
                        assert_eq!(
                            said,
                            [("User", SECOND), ("Assistant", ELSEWHERE), ("User", THIRD)],
                            "the resumed session was not sent the turn it missed: {}",
                            resumed.stdin
                        );
                    }
                    None => {
                        for said in [FIRST, ANSWER, SECOND, ELSEWHERE, THIRD] {
                            assert!(
                                resumed.stdin.contains(said),
                                "the fresh session was not sent {said:?}: {}",
                                resumed.stdin
                            );
                        }
                    }
                }
                let back = back.expect("the turn back on claude kept its session");
                assert_eq!(back.entry, latest);
            }

            #[tokio::test]
            async fn a_chat_switched_to_codex_and_back_never_resumes_the_codex_session() {
                let chat = Chat::on_claude().await;
                chat.ask(FIRST).await;
                let first = chat
                    .session()
                    .await
                    .expect("the first turn recorded its session");
                let codex = codex_login(&chat.state, &chat.organization).await;
                chat.choose(Some(Provider::Codex), None).await;
                let thread = chats::ChatSession {
                    login: Some(codex),
                    id: "019a0000-0000-7000-8000-000000000000".to_string(),
                    agent: AgentKind::Codex,
                    entry: chat.latest().await,
                    prompt: first.prompt.clone(),
                };
                chats::set_session(
                    &chat.organization.pool,
                    chat.organization.chat,
                    Some(&thread),
                )
                .await
                .expect("the codex turn's session");
                chat.choose(Some(Provider::ClaudeCode), None).await;
                chat.ask(SECOND).await;
                let back = chat.session().await;
                let runs = chat.agent.runs();
                chat.remove().await;

                let [_, fresh] = runs.as_slice() else {
                    panic!("expected two sessioned runs, got {}", runs.len());
                };
                assert_eq!(
                    fresh.flag("--resume"),
                    None,
                    "claude resumed codex's thread"
                );
                let pinned = fresh
                    .flag("--session-id")
                    .expect("claude started a fresh session");
                assert_ne!(pinned, thread.id);
                assert_ne!(pinned, first.id);
                for said in [FIRST, ANSWER, SECOND] {
                    assert!(
                        fresh.stdin.contains(said),
                        "the fresh session was not sent {said:?}: {}",
                        fresh.stdin
                    );
                }
                let back = back.expect("the turn back on claude recorded its session");
                assert_eq!(back.agent, AgentKind::Claude);
                assert_eq!(back.id, pinned);
                assert_eq!(back.login, Some(chat.login));
            }

            #[tokio::test]
            async fn a_project_chats_turn_runs_its_agent_in_the_projects_folder() {
                let chat = Chat::on_claude().await;
                let project = TempDir::new().expect("a project folder");
                let physical = std::fs::canonicalize(project.path()).expect("the real path");
                chat.in_project(project.path()).await;

                chat.ask(FIRST).await;
                let runs = chat.agent.runs();
                let answer = chat.answer().await;
                chat.remove().await;

                let [run] = runs.as_slice() else {
                    panic!("expected one sessioned run, got {}", runs.len());
                };
                assert_eq!(run.directory, physical.to_str().expect("a UTF-8 path"));
                assert_eq!(answer, ANSWER, "a chat in its directory is told of nothing");
            }

            #[tokio::test]
            async fn a_chat_with_no_project_runs_its_agent_in_the_organizations_directory() {
                let chat = Chat::on_claude().await;

                chat.ask(FIRST).await;
                let runs = chat.agent.runs();
                let work = chat.work();
                chat.remove().await;

                let [run] = runs.as_slice() else {
                    panic!("expected one sessioned run, got {}", runs.len());
                };
                assert_eq!(run.directory, work.to_str().expect("a UTF-8 path"));
            }

            /// The session a chat started before it had a project, or before its project had a
            /// folder here, is the one it goes on in: claude finds a session by the directory it
            /// runs in, so the file moves with the chat.
            #[tokio::test]
            async fn a_chat_that_comes_to_start_in_a_folder_resumes_the_session_it_had() {
                let chat = Chat::on_claude().await;
                let project = TempDir::new().expect("a project folder");
                let physical = std::fs::canonicalize(project.path()).expect("the real path");

                chat.ask(FIRST).await;
                let first = chat.session().await.expect("the first turn's session");
                let work = chat.work();
                let before = chat.transcript(&work, &first.id);
                std::fs::create_dir_all(before.parent().expect("a folder")).expect("the folder");
                std::fs::write(&before, "{}\n").expect("the transcript claude keeps");

                chat.in_project(project.path()).await;
                chat.ask(SECOND).await;
                let second = chat.session().await.expect("the second turn's session");
                let runs = chat.agent.runs();
                let after = chat.transcript(&physical, &first.id);
                let (moved, left) = (after.is_file(), before.exists());
                chat.remove().await;

                let [opening, resumed] = runs.as_slice() else {
                    panic!("expected two sessioned runs, got {}", runs.len());
                };
                assert_eq!(opening.directory, work.to_str().expect("a UTF-8 path"));
                assert_eq!(resumed.directory, physical.to_str().expect("a UTF-8 path"));
                assert_eq!(resumed.flag("--resume"), Some(first.id.as_str()));
                assert_eq!(second.id, first.id);
                assert!(
                    moved,
                    "claude would not find its session from the new directory"
                );
                assert!(!left, "a session stays in one place");
            }

            #[tokio::test]
            async fn a_project_folder_that_is_gone_leaves_the_turn_answering_and_says_why() {
                let chat = Chat::on_claude().await;
                let removed = TempDir::new().expect("a parent");
                let gone = removed.path().join("gone");
                chat.in_project(&gone).await;
                chat.with_agent().await;

                chat.ask(FIRST).await;
                let runs = chat.agent.runs();
                let answer = chat.answer().await;
                let work = chat.work();
                chat.remove().await;

                let [run] = runs.as_slice() else {
                    panic!("expected one sessioned run, got {}", runs.len());
                };
                assert_eq!(run.directory, work.to_str().expect("a UTF-8 path"));
                assert!(answer.starts_with("> "), "{answer}");
                assert!(
                    answer.contains(gone.to_str().expect("a UTF-8 path")),
                    "{answer}"
                );
                assert!(
                    answer.ends_with(ANSWER),
                    "the turn was not answered: {answer}"
                );
            }

            /// The reason is said once. A source that stays unavailable must not open every answer
            /// in the chat with it, or carry it into each answer's replay and embedding.
            #[tokio::test]
            async fn a_chat_is_told_once_why_it_is_not_in_its_folder() {
                let chat = Chat::on_claude().await;
                let removed = TempDir::new().expect("a parent");
                chat.in_project(&removed.path().join("gone")).await;
                chat.with_agent().await;

                chat.ask(FIRST).await;
                let first = chat.answer().await;
                chat.ask(SECOND).await;
                let second = chat.answer().await;
                chat.remove().await;

                assert!(first.starts_with("> "), "{first}");
                assert_eq!(second, ANSWER, "the reason was repeated");
            }

            /// A CLI runs in the organization's directory and Zone's tools in another, so the notice
            /// names neither.
            #[tokio::test]
            async fn the_notice_does_not_name_a_directory_the_agent_is_not_in() {
                let chat = Chat::on_claude().await;
                let removed = TempDir::new().expect("a parent");
                chat.in_project(&removed.path().join("gone")).await;
                chat.with_agent().await;

                chat.ask(FIRST).await;
                let answer = chat.answer().await;
                chat.remove().await;

                assert!(
                    !answer.contains("working in"),
                    "the notice names a directory: {answer}"
                );
            }

            /// A clone root nothing can be made in would say so, were anything cloned.
            #[tokio::test]
            async fn a_chat_with_its_agent_off_is_not_cloned_for_or_told_of_it() {
                let blocked = TempDir::new().expect("a parent");
                let file = blocked.path().join("file");
                std::fs::write(&file, "").expect("a file where the clones would go");
                let chat = Chat::cloning_under(file.join("clones")).await;
                chat.sourced(
                    db::sources::GITHUB,
                    serde_json::json!({"owner": "owner", "repo": "repository"}),
                )
                .await;

                chat.ask(FIRST).await;
                let answer = chat.answer().await;
                chat.remove().await;

                assert_eq!(answer, ANSWER);
            }

            /// A clone root nothing can be made in stands in for every way a clone fails, with no
            /// network in the test.
            #[tokio::test]
            async fn a_clone_that_fails_leaves_the_turn_answering_and_says_why() {
                let blocked = TempDir::new().expect("a parent");
                let file = blocked.path().join("file");
                std::fs::write(&file, "").expect("a file where the clones would go");
                let chat = Chat::cloning_under(file.join("clones")).await;
                chat.sourced(
                    db::sources::GITHUB,
                    serde_json::json!({"owner": "owner", "repo": "repository"}),
                )
                .await;
                chat.with_agent().await;

                chat.ask(FIRST).await;
                let runs = chat.agent.runs();
                let answer = chat.answer().await;
                let work = chat.work();
                chat.remove().await;

                let [run] = runs.as_slice() else {
                    panic!("expected one sessioned run, got {}", runs.len());
                };
                assert_eq!(run.directory, work.to_str().expect("a UTF-8 path"));
                assert!(
                    answer.starts_with("> Zone couldn't clone https://github.com/owner/repository"),
                    "{answer}"
                );
                assert!(
                    answer.ends_with(ANSWER),
                    "the turn was not answered: {answer}"
                );
            }

            #[tokio::test]
            async fn a_resume_the_agent_refuses_is_replayed_whole_in_a_fresh_session() {
                let chat = Chat::on_claude().await;

                chat.ask(FIRST).await;
                let first = chat
                    .session()
                    .await
                    .expect("the first turn recorded its session");
                chat.agent.refuse();
                chat.ask(SECOND).await;
                let second = chat.session().await;
                let runs = chat.agent.runs();
                let answers: Vec<String> = sqlx::query_scalar(
                    "SELECT content FROM messages WHERE chat_id = $1 AND role = 'assistant' ORDER BY created_at",
                )
                .bind(chat.organization.chat)
                .fetch_all(&chat.organization.pool)
                .await
                .expect("the chat's answers");
                chat.remove().await;

                let [_, refused, replayed] = runs.as_slice() else {
                    panic!("expected three sessioned runs, got {}", runs.len());
                };
                assert_eq!(refused.flag("--resume"), Some(first.id.as_str()));
                let fresh = replayed
                    .flag("--session-id")
                    .expect("the replay pinned a fresh session");
                assert_ne!(fresh, first.id);
                assert_eq!(replayed.flag("--resume"), None);
                for said in [FIRST, ANSWER, SECOND] {
                    assert!(
                        replayed.stdin.contains(said),
                        "the replay left out {said:?}: {}",
                        replayed.stdin
                    );
                }
                assert_eq!(answers, [ANSWER, ANSWER], "the refusal reached the reader");
                let second = second.expect("the replay recorded its session");
                assert_eq!(second.id, fresh);
                assert_eq!(second.prompt, first.prompt);
            }

            #[tokio::test]
            async fn a_refused_resume_replays_with_the_tools_and_the_watch_of_the_turn() {
                const SURVEY: &str = "toolu_01Survey";
                let chat = Chat::on_claude().await;
                sqlx::query(
                    "UPDATE chats SET agent_enabled = TRUE, agent_sandboxed = FALSE WHERE id = $1",
                )
                .bind(chat.organization.chat)
                .execute(&chat.organization.pool)
                .await
                .expect("an agentic, unsandboxed chat");

                chat.ask(FIRST).await;
                std::fs::write(
                    chat.agent.directory.path().join("reply.jsonl"),
                    subagents_until_the_first_result(),
                )
                .expect("the stand-in's reply");
                chat.agent.refuse();
                let stream = ChatStream::of(chat.organization.chat);
                let (_, mut frames) = stream.join().await;
                let watched = tokio::spawn(async move {
                    loop {
                        match frames.recv().await {
                            Ok(ServerMessage::Subagent { subagent, .. })
                                if subagent.external_id == SURVEY =>
                            {
                                return subagent.message_id;
                            }
                            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                            Err(broadcast::error::RecvError::Closed) => return None,
                        }
                    }
                });
                let replayed_turn = chat.ask(SECOND).await;
                let watched = tokio::time::timeout(Duration::from_secs(30), watched)
                    .await
                    .expect("nothing watched the replay's subagents: no survey reached its readers")
                    .expect("the chat's readers");
                drop(stream);
                let runs = chat.agent.runs();
                let stored =
                    db::chat_subagents::list(&chat.organization.pool, chat.organization.chat)
                        .await
                        .expect("the chat's subagents");
                chat.remove().await;

                let [served, _, replayed] = runs.as_slice() else {
                    panic!("expected three sessioned runs, got {}", runs.len());
                };
                assert!(
                    served.flag("--mcp-config").is_some(),
                    "the chat serves its agent tools: {:?}",
                    served.arguments
                );
                assert!(
                    replayed.flag("--mcp-config").is_some(),
                    "the replay lost zone's tools: {:?}",
                    replayed.arguments
                );
                for flag in ["--tools", "--permission-prompt-tool"] {
                    assert_eq!(
                        replayed.flag(flag),
                        served.flag(flag),
                        "the replay changed {flag}"
                    );
                }
                assert_eq!(
                    watched,
                    Some(replayed_turn),
                    "the survey was not the replayed turn's"
                );
                assert!(
                    stored.iter().any(|row| row.external_id == SURVEY),
                    "the replay's survey reached its readers but not the store: {stored:?}"
                );
            }

            /// Claude's subagents fixture up to the turn's first result, which
            /// leaves its background review still running when the turn ends.
            fn subagents_until_the_first_result() -> String {
                const FIXTURE: &str = include_str!(
                    "../../../zone_core/src/llm/client/fixtures/claude/subagents.jsonl"
                );
                let mut reply = String::new();
                for line in FIXTURE
                    .lines()
                    .filter(|line| !line.contains(r#""subtype":"init""#))
                {
                    reply.push_str(line);
                    reply.push('\n');
                    if line.starts_with(r#"{"type":"result""#) {
                        return reply;
                    }
                }
                panic!("the fixture has no result");
            }

            #[tokio::test]
            async fn a_turns_subagents_reach_its_readers_and_its_store_and_are_swept_when_it_ends()
            {
                use abnegate_agent_cli::{StepStatus, SubagentStatus};

                const SURVEY: &str = "toolu_01Survey";
                const REVIEW: &str = "toolu_04Review";
                let chat = Chat::on_claude().await;
                std::fs::write(
                    chat.agent.directory.path().join("reply.jsonl"),
                    subagents_until_the_first_result(),
                )
                .expect("the stand-in's reply");
                let stream = ChatStream::of(chat.organization.chat);
                let (_, mut frames) = stream.join().await;

                chat.ask(FIRST).await;
                let mut started = None;
                let mut announced = Vec::new();
                let mut replayed = Vec::new();
                tokio::time::timeout(Duration::from_secs(30), async {
                    loop {
                        match frames.recv().await.expect("the chat's frames") {
                            ServerMessage::MessageStart { message_id, .. } => {
                                started = Some(message_id);
                            }
                            ServerMessage::Subagent {
                                subagent, steps, ..
                            } => {
                                replayed.extend(stream.join().await.0);
                                let swept = subagent.external_id == REVIEW
                                    && subagent.status == SubagentStatus::Stopped;
                                announced.push((subagent, steps));
                                if swept {
                                    return;
                                }
                            }
                            _ => {}
                        }
                    }
                })
                .await
                .expect("the review is swept once the turn ends");
                let answer: Uuid = sqlx::query_scalar(
                    "SELECT id FROM messages WHERE chat_id = $1 AND role = 'assistant'",
                )
                .bind(chat.organization.chat)
                .fetch_one(&chat.organization.pool)
                .await
                .expect("the chat's answer");
                let stored =
                    db::chat_subagents::list(&chat.organization.pool, chat.organization.chat)
                        .await
                        .expect("the chat's subagents");
                let survey = stored
                    .iter()
                    .find(|row| row.external_id == SURVEY)
                    .expect("the survey")
                    .clone();
                let steps = db::chat_subagents::steps(
                    &chat.organization.pool,
                    chat.organization.chat,
                    survey.id,
                )
                .await
                .expect("the survey's steps")
                .expect("the survey");
                chat.remove().await;

                assert_eq!(
                    started,
                    Some(answer),
                    "the console's answer is the stored one"
                );
                assert!(
                    announced
                        .iter()
                        .all(|(subagent, _)| subagent.message_id == Some(answer)),
                    "{announced:?}"
                );
                let named: std::collections::BTreeSet<&str> = announced
                    .iter()
                    .map(|(subagent, _)| subagent.external_id.as_str())
                    .collect();
                assert_eq!(named, std::collections::BTreeSet::from([SURVEY, REVIEW]));
                assert!(
                    !replayed
                        .iter()
                        .any(|frame| matches!(frame, ServerMessage::Subagent { .. })),
                    "a joining client reads subagents over REST: {replayed:?}"
                );
                let statuses: Vec<(&str, &str)> = stored
                    .iter()
                    .map(|row| (row.external_id.as_str(), row.status.as_str()))
                    .collect();
                assert_eq!(
                    statuses,
                    [
                        (SURVEY, SubagentStatus::Completed.as_str()),
                        (REVIEW, SubagentStatus::Stopped.as_str()),
                    ]
                );
                let taken: Vec<(&str, Option<&str>, StepStatus)> = steps
                    .iter()
                    .map(|step| (step.name.as_str(), step.subject.as_deref(), step.status))
                    .collect();
                assert_eq!(
                    taken,
                    [
                        ("Grep", Some("pub mod"), StepStatus::Succeeded),
                        ("Read", Some("/w/src/missing.rs"), StepStatus::Failed),
                    ]
                );
                assert_eq!(survey.step_count, 2);
            }
        }

        #[tokio::test]
        async fn an_unsettled_turn_asks_the_organizations_agent_once_and_routes_on_it() {
            let claude = StandIn::answering("IMAGE");
            let (organization, _agents, state) =
                routing_media_on_claude(Some(claude.executable.clone())).await;

            let routing = prepare_message(
                &state,
                organization.chat,
                organization.workspace,
                "generate ambient rain sounds",
                None,
            )
            .await;
            organization.remove().await;

            let Ok(Routing::Image(_, LlmBackend::Cli { settings, .. }, _)) = routing else {
                panic!("expected the agent's IMAGE to route the turn to an image");
            };
            assert_eq!(settings.executable.as_ref(), Some(&claude.executable));
            let runs = claude.runs();
            assert_eq!(runs.len(), 1, "{runs:?}");
            assert!(
                runs[0].windows(2).any(|pair| pair == [MODEL_FLAG, FAST]),
                "the classifier did not run on the Fast model: {runs:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_provider_rejecting_the_saved_key_reaches_the_reader_without_it() {
        use crate::services::endpoint::testing::settings;
        use abnegate_secret::{REDACTED, SecretValue};
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        use zone_context::embeddings::providers::PROVIDER_SELF_HOSTED;

        const KEY: &str = "sk-organization-0123456789abcdef";
        const MASKED: &str = "sk-org************cdef";
        let provider = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "error": {
                    "message": format!("Incorrect API key provided: {MASKED}. The key {KEY} is not valid."),
                    "type": "invalid_request_error",
                    "code": "invalid_api_key"
                }
            })))
            .mount(&provider)
            .await;
        let mut saved = settings(PROVIDER_SELF_HOSTED);
        saved.litellm_host = Some(provider.uri());
        saved.litellm_key = Some(SecretValue::new(KEY.to_string()));
        let endpoint = Endpoint::resolve(&crate::state::test_config(), &saved);
        let llm = endpoint.llm("llama3.2:3b", 0.7, 64, LlmBackend::Http);

        let Err(error) = llm.chat_stream(&[LlmMessage::user("Hello")], None).await else {
            panic!("the provider rejected the key, so the completion must fail");
        };
        let reported = format!("Failed to generate response: {error}");
        assert!(
            !reported.contains(KEY) && !reported.contains(MASKED),
            "the client reported the key it was sent: {reported}"
        );
        let message = failure(&endpoint, &LlmBackend::Http, reported);

        assert!(
            !message.contains(KEY),
            "the key reached the reader: {message}"
        );
        assert!(
            !message.contains(MASKED),
            "the key's masked echo reached the reader: {message}"
        );
        assert!(
            message.contains("Incorrect API key provided") && message.contains(REDACTED),
            "the reader lost why the turn failed: {message}"
        );
    }

    #[test]
    fn a_placeholder_keeps_every_offset_ahead_of_it() {
        let notice = "> The folder is gone.\n\n";
        let mut content = format!("{notice}\n\n");
        let kept = notice.chars().count();
        let mut calls = vec![ToolCallRecord {
            at: Some(kept + 2),
            reasoning_at: Some(kept + 1),
            ..call("read_file", true)
        }];
        let mut handovers = vec![moved(kept + 2)];
        let mut thinking = Thinking {
            text: "Still thinking.".to_string(),
            at: kept + 2,
        };

        assert!(fill_silence(
            &mut content,
            notice.len(),
            STOPPED_BEFORE_ANSWERING
        ));
        clamp_offsets(kept, &mut calls, &mut handovers, &mut thinking);

        assert_eq!(
            calls[0].at,
            Some(kept),
            "the call no longer cuts the placeholder"
        );
        assert_eq!(calls[0].reasoning_at, Some(kept));
        assert_eq!(handovers[0].at, kept);
        assert_eq!(thinking.at, kept);
        assert!(
            !fill_silence(&mut format!("{notice}Written."), notice.len(), "[silent]"),
            "an answer with prose is not silent, so nothing moves"
        );
    }

    #[test]
    fn thinking_remembers_where_it_began_and_hands_that_to_its_call() {
        let mut thinking = Thinking::default();
        thinking.push("Look at ", || 13);
        thinking.push("the file.", || 20);

        let thought = thinking.take().expect("thinking was written");
        assert_eq!(thought.at, 13, "the offset is where the thinking began");
        assert_eq!(thought.text, "Look at the file.");
        assert!(thinking.take().is_none(), "the call took it");

        thinking.push("Next.", || 30);
        assert_eq!(thinking.at, 30, "the next thought starts its own offset");
    }

    #[test]
    fn a_silent_turn_keeps_its_placeholder_after_a_notice() {
        let notice = "> The folder is gone.\n\n";
        for (written, expected) in [
            ("", format!("{notice}[silent]")),
            ("  \n", format!("{notice}[silent]")),
            ("Written.", format!("{notice}Written.")),
        ] {
            let mut content = format!("{notice}{written}");
            fill_silence(&mut content, notice.len(), "[silent]");
            assert_eq!(content, expected);
        }

        let mut silent = "  ".to_string();
        fill_silence(&mut silent, 0, "[silent]");
        assert_eq!(
            silent, "[silent]",
            "with no notice it replaces the whole answer"
        );
    }

    #[tokio::test]
    async fn chunk_emission_tracks_indices_and_stops_before_the_response_limit() {
        let stream = ChatStream::of(Uuid::new_v4());
        let mut events = stream.events.subscribe();
        let mut content = String::new();
        let mut index = 0;
        let mut truncated = false;

        assert!(
            emit_chunk(
                &stream,
                &mut content,
                &mut index,
                String::new(),
                &mut truncated
            )
            .await
        );
        assert!(events.try_recv().is_err());
        assert!(
            emit_chunk(
                &stream,
                &mut content,
                &mut index,
                "hello".into(),
                &mut truncated
            )
            .await
        );
        assert_eq!(content, "hello");
        assert_eq!(index, 1);
        assert!(!truncated);
        assert!(matches!(
            events.recv().await,
            Ok(ServerMessage::Chunk { content, index: 0 }) if content == "hello"
        ));

        content = "x".repeat(MAX_RESPONSE_LENGTH);
        assert!(
            !emit_chunk(
                &stream,
                &mut content,
                &mut index,
                "y".into(),
                &mut truncated
            )
            .await
        );
        assert!(truncated);
        assert_eq!(content.len(), MAX_RESPONSE_LENGTH);
        assert!(events.try_recv().is_err());
    }

    fn started(message_id: Uuid) -> ServerMessage {
        ServerMessage::MessageStart {
            message_id,
            role: "assistant".to_string(),
            resumed: false,
        }
    }

    fn chunk(content: &str, index: u32) -> ServerMessage {
        ServerMessage::Chunk {
            content: content.to_string(),
            index,
        }
    }

    fn frame_kinds(frames: &[ServerMessage]) -> Vec<String> {
        frames
            .iter()
            .map(|frame| {
                serde_json::to_value(frame).unwrap()["type"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    fn waiting() -> Waiting {
        Waiting {
            kind: wait::KIND_JOB.to_string(),
            id: JOB_ID.to_string(),
            reference: None,
            deadline: "2026-09-13T00:05:00+00:00".to_string(),
        }
    }

    const JOB_ID: &str = "job_9f3c1a7b2e04";
    const JOB_LOG: &str = "/w/.zone/jobs/job_9f3c1a7b2e04.log";

    fn spawned() -> JobStarted {
        JobStarted::new(JOB_ID, 48213, JOB_LOG)
    }

    #[test]
    fn a_turn_in_flight_replays_as_resumed() {
        let message_id = Uuid::new_v4();
        let mut turn = LiveTurn::default();
        turn.record(&started(message_id));
        turn.record(&chunk("Reading ", 0));

        let replay = turn.replay();
        assert!(
            matches!(
                replay.first(),
                Some(ServerMessage::MessageStart { resumed: true, message_id: replayed, .. })
                    if *replayed == message_id
            ),
            "a joining connection revives the message it already has: {replay:?}"
        );
    }

    #[test]
    fn replayed_text_arrives_as_one_frame() {
        let mut turn = LiveTurn::default();
        turn.record(&started(Uuid::new_v4()));
        turn.record(&chunk("Rust ", 0));
        turn.record(&chunk("is ", 1));
        turn.record(&chunk("fine", 2));

        let replay = turn.replay();
        assert_eq!(replay.len(), 2, "one start and one chunk: {replay:?}");
        assert!(
            matches!(&replay[1], ServerMessage::Chunk { content, .. } if content == "Rust is fine"),
            "{replay:?}"
        );
    }

    #[test]
    fn replay_keeps_the_order_tools_ran_in() {
        let message_id = Uuid::new_v4();
        let mut turn = LiveTurn::default();
        turn.record(&started(message_id));
        turn.record(&ServerMessage::Reasoning {
            content: "I should look".to_string(),
        });
        turn.record(&ServerMessage::ToolCall {
            message_id,
            tool_call_id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: "{}".to_string(),
            at: 0,
            reasoning: None,
            reasoning_at: None,
            reason: None,
        });
        turn.record(&chunk("Found it", 0));

        assert_eq!(
            frame_kinds(&turn.replay()),
            vec!["message_start", "reasoning", "tool_call", "chunk"],
            "thinking still sits with the call it preceded"
        );
    }

    fn moved(at: usize) -> Notice {
        Notice {
            from: "a@example.com".to_string(),
            to: "c@example.com".to_string(),
            from_agent: abnegate_agent_cli::AgentKind::Claude,
            agent: abnegate_agent_cli::AgentKind::Codex,
            reason: crate::services::chat::handover::Reason::Limit,
            resets_at: chrono::DateTime::parse_from_rfc3339("2026-09-23T06:10:00Z")
                .ok()
                .map(|at| at.with_timezone(&chrono::Utc)),
            carried: false,
            at,
            calls: 0,
            reasoning: None,
        }
    }

    #[test]
    fn a_handover_frame_reads_as_the_console_parses_it() {
        let message_id = Uuid::new_v4();

        let frame = serde_json::to_value(ServerMessage::Handover {
            message_id,
            notice: moved(12),
        })
        .expect("a frame");

        assert_eq!(
            frame,
            serde_json::json!({
                "type": "handover",
                "message_id": message_id,
                "from": "a@example.com",
                "to": "c@example.com",
                "from_agent": "claude",
                "agent": "codex",
                "reason": "limit",
                "resets_at": "2026-09-23T06:10:00Z",
                "carried": false,
                "at": 12,
                "calls": 0,
            })
        );
    }

    #[test]
    fn a_joining_connection_sees_the_handover_where_the_answer_moved() {
        let message_id = Uuid::new_v4();
        let mut turn = LiveTurn::default();
        turn.record(&started(message_id));
        turn.record(&chunk("The Nile, ", 0));
        turn.record(&ServerMessage::Handover {
            message_id,
            notice: moved(10),
        });
        turn.record(&chunk("the Amazon", 1));

        let replay = turn.replay();
        assert_eq!(
            frame_kinds(&replay),
            vec!["message_start", "chunk", "handover", "chunk"],
            "the divider stays between the two logins' text"
        );
    }

    #[test]
    fn an_answer_that_moved_keeps_each_move_and_what_every_login_spent() {
        let spent = Usage::new(120, 30);

        let merged = merge_metadata(None, &[], &[], &[], None, &[moved(10)], Some(spent))
            .expect("a move produces metadata");
        let unmoved = merge_metadata(None, &[], &[], &[], None, &[], Some(spent));

        assert_eq!(
            merged,
            serde_json::json!({
                "handovers": [{
                    "from": "a@example.com",
                    "to": "c@example.com",
                    "from_agent": "claude",
                    "agent": "codex",
                    "reason": "limit",
                    "resets_at": "2026-09-23T06:10:00Z",
                    "carried": false,
                    "at": 10,
                    "calls": 0,
                }],
                "usage": {"prompt_tokens": 120, "completion_tokens": 30, "total_tokens": 150},
            })
        );
        assert_eq!(
            unmoved, None,
            "a turn on one login keeps its metadata as it was"
        );
    }

    #[test]
    fn adjacent_reasoning_frames_coalesce_and_an_error_ends_the_live_turn() {
        let mut turn = LiveTurn::default();
        turn.record(&started(Uuid::new_v4()));
        turn.record(&ServerMessage::Reasoning {
            content: "Inspect ".into(),
        });
        turn.record(&ServerMessage::Reasoning {
            content: "state".into(),
        });
        let replay = turn.replay();
        assert!(matches!(
            &replay[1],
            ServerMessage::Reasoning { content } if content == "Inspect state"
        ));
        turn.record(&ServerMessage::Error {
            message: "stopped".into(),
        });
        assert!(turn.replay().is_empty());
    }

    #[test]
    fn a_finished_turn_leaves_nothing_to_replay() {
        let message_id = Uuid::new_v4();
        let mut turn = LiveTurn::default();
        turn.record(&started(message_id));
        turn.record(&chunk("Done", 0));
        turn.record(&ServerMessage::MessageEnd {
            message_id,
            content: "Done".to_string(),
            metadata: None,
            error: None,
        });

        assert!(
            turn.replay().is_empty(),
            "the saved message is what a joining connection loads"
        );
    }

    #[test]
    fn a_cancelled_turn_leaves_nothing_to_replay() {
        let message_id = Uuid::new_v4();
        let mut turn = LiveTurn::default();
        turn.record(&started(message_id));
        turn.record(&chunk("Partly", 0));
        turn.record(&ServerMessage::Cancelled {
            message_id: Some(message_id),
        });

        assert!(turn.replay().is_empty());
    }

    #[test]
    fn frames_outside_a_turn_are_not_replayed() {
        let mut turn = LiveTurn::default();
        turn.record(&ServerMessage::MessageSaved {
            message_id: Uuid::new_v4(),
            role: "user".to_string(),
            content: "hello".to_string(),
            metadata: None,
        });

        assert!(
            turn.replay().is_empty(),
            "the chat a joining connection loads already has these"
        );
    }

    #[test]
    fn a_status_before_the_message_starts_replays_until_the_turn_settles() {
        let queued = ServerMessage::Status {
            message: MEDIA_QUEUED.to_string(),
        };
        let mut turn = LiveTurn::default();
        turn.record(&ServerMessage::Status {
            message: "Preparing video generation...".to_string(),
        });
        turn.record(&queued);

        assert_eq!(
            frame_kinds(&turn.replay()),
            vec!["status"],
            "only the latest status replays"
        );
        assert!(
            matches!(
                turn.replay().as_slice(),
                [ServerMessage::Status { message }] if message == MEDIA_QUEUED
            ),
            "{:?}",
            turn.replay()
        );

        turn.record(&ServerMessage::Cancelled { message_id: None });
        assert!(turn.replay().is_empty(), "a settled wait leaves nothing");

        turn.record(&queued);
        turn.record(&started(Uuid::new_v4()));
        assert_eq!(
            frame_kinds(&turn.replay()),
            vec!["message_start"],
            "a started message replaces the status it was waiting under"
        );
    }

    #[tokio::test]
    async fn every_connection_on_a_chat_sees_the_same_frames() {
        let stream = ChatStream::of(Uuid::new_v4());
        let (_, mut first) = stream.join().await;
        let message_id = Uuid::new_v4();
        publish(&stream, started(message_id)).await;
        publish(&stream, chunk("Half ", 0)).await;

        // A second connection arrives mid-turn.
        let (replay, mut second) = stream.join().await;
        publish(&stream, chunk("a reply", 1)).await;

        assert_eq!(replay.len(), 2, "{replay:?}");
        assert!(
            matches!(&replay[1], ServerMessage::Chunk { content, .. } if content == "Half "),
            "{replay:?}"
        );
        assert!(matches!(
            second.recv().await,
            Ok(ServerMessage::Chunk { .. })
        ));
        assert!(
            second.try_recv().is_err(),
            "the joining connection gets what it missed once, not twice"
        );
        assert!(matches!(
            first.recv().await,
            Ok(ServerMessage::MessageStart { resumed: false, .. })
        ));
        for expected in ["Half ", "a reply"] {
            match first.recv().await {
                Ok(ServerMessage::Chunk { content, .. }) => assert_eq!(content, expected),
                other => panic!("expected a chunk, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn a_chat_stream_lives_as_long_as_someone_holds_it() {
        let chat_id = Uuid::new_v4();
        let stream = ChatStream::of(chat_id);
        assert!(
            Arc::ptr_eq(&stream, &ChatStream::of(chat_id)),
            "a reconnecting client joins the stream the generation is publishing to"
        );

        drop(stream);
        assert!(
            !CHAT_STREAMS.contains_key(&chat_id),
            "the last holder leaving frees the chat's frames"
        );
    }

    fn subagent(chat_id: Uuid) -> ChatSubagent {
        let now = chrono::Utc::now();
        ChatSubagent {
            id: Uuid::new_v4(),
            chat_id,
            message_id: Some(Uuid::new_v4()),
            external_id: "toolu_01Agent".to_string(),
            description: "Ask for a review".to_string(),
            kind: None,
            model: None,
            background: true,
            status: abnegate_agent_cli::SubagentStatus::Running,
            tool_uses: None,
            duration_ms: None,
            tokens: None,
            last_tool: None,
            summary: None,
            error: None,
            step_count: 1,
            revision: 2,
            started_at: now,
            finished_at: None,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn a_subagent_reaches_every_connection_but_is_never_replayed() {
        let chat_id = Uuid::new_v4();
        let stream = ChatStream::of(chat_id);
        publish(&stream, started(Uuid::new_v4())).await;
        let (_, mut connection) = stream.join().await;
        let frames = SubagentFrames {
            stream: stream.clone(),
        };
        let changed = subagent(chat_id);
        let step = SubagentStep::new("toolu_x", "Bash", abnegate_agent_cli::StepStatus::Running);

        frames.announce(changed.clone(), vec![step]).await;

        let frame = connection.recv().await.expect("the subagent frame");
        assert_eq!(
            serde_json::to_value(&frame).expect("a frame"),
            serde_json::json!({
                "type": "subagent",
                "chat_id": chat_id,
                "subagent": serde_json::to_value(&changed).expect("a subagent"),
                "steps": [{"id": "toolu_x", "name": "Bash", "subject": null, "status": "running"}],
            })
        );
        assert_eq!(
            frame_kinds(&stream.join().await.0),
            ["message_start"],
            "a joining client reads subagents over REST"
        );
    }

    #[tokio::test]
    async fn removed_subagents_reach_every_connection_but_are_never_replayed() {
        let chat_id = Uuid::new_v4();
        let stream = ChatStream::of(chat_id);
        publish(&stream, started(Uuid::new_v4())).await;
        let (_, mut connection) = stream.join().await;
        let removed = vec![Uuid::new_v4(), Uuid::new_v4()];

        announce_removed_subagents(chat_id, Vec::new()).await;
        announce_removed_subagents(chat_id, removed.clone()).await;

        let frame = connection.recv().await.expect("the removal frame");
        assert_eq!(
            serde_json::to_value(&frame).expect("a frame"),
            serde_json::json!({
                "type": "subagents_removed",
                "chat_id": chat_id,
                "ids": removed,
            }),
            "a removal of nothing is not sent"
        );
        assert_eq!(
            frame_kinds(&stream.join().await.0),
            ["message_start"],
            "a joining client reads subagents over REST"
        );
    }

    #[derive(Debug)]
    struct Watching;

    impl Observer for Watching {
        fn observe(&self, _output: &abnegate_agent_cli::Output) {}
    }

    fn observed(client: &LlmClient) -> Option<Arc<dyn Observer>> {
        client.observer().cloned()
    }

    fn cli() -> LlmBackend {
        LlmBackend::cli(
            abnegate_agent_cli::AgentKind::Claude,
            abnegate_agent_cli::CliSettings::default(),
        )
    }

    #[test]
    fn a_coding_agent_is_watched_whether_or_not_it_is_served_tools() {
        let observer: Arc<dyn Observer> = Arc::new(Watching);
        let client = LlmClient::new(abnegate_llm::LlmConfig::default()).with_backend(cli());

        let client = equipped(client, None, &observer, true);
        assert!(
            observed(&client).is_some_and(|attached| Arc::ptr_eq(&attached, &observer)),
            "a turn the agent tools are off for is still watched"
        );

        let client = client.with_backend(cli());
        assert!(
            observed(&client).is_none(),
            "a new backend starts unwatched"
        );
        let client = equipped(client, None, &observer, true);
        assert!(
            observed(&client).is_some_and(|attached| Arc::ptr_eq(&attached, &observer)),
            "a turn handed over to another login is watched again"
        );

        let endpoint = equipped(
            LlmClient::new(abnegate_llm::LlmConfig::default()),
            None,
            &observer,
            true,
        );
        assert!(matches!(endpoint.backend(), LlmBackend::Http));
    }

    #[test]
    fn a_coding_agent_served_tools_is_watched_beside_them() {
        let observer: Arc<dyn Observer> = Arc::new(Watching);
        let mut tools = ChatTools::empty();
        let served = AgentTools::serve(
            &cli(),
            &mut tools,
            crate::mcp::testing::scope(Uuid::new_v4()),
            "http://127.0.0.1:1/mcp",
        )
        .expect("a coding agent is served the turn's tools");

        for (sandboxed, kept) in [
            (true, BuiltinTools::Withheld),
            (false, BuiltinTools::Granted),
        ] {
            let client = equipped(
                LlmClient::new(abnegate_llm::LlmConfig::default()).with_backend(cli()),
                Some(&served),
                &observer,
                sandboxed,
            );

            assert!(
                client.toolset().is_some(),
                "sandboxed {sandboxed}: the agent reaches the turn's tools"
            );
            assert_eq!(client.builtin_tools(), kept, "sandboxed {sandboxed}");
            assert!(
                observed(&client).is_some_and(|attached| Arc::ptr_eq(&attached, &observer)),
                "sandboxed {sandboxed}: serving tools does not unwatch the agent"
            );
        }
    }

    #[tokio::test]
    async fn a_joining_connection_sees_the_queue_and_later_changes() {
        let chat_id = Uuid::new_v4();
        let stream = ChatStream::of(chat_id);
        let (frames, _) = stream.join().await;
        assert!(frames.is_empty(), "an empty queue sends nothing on join");

        let first = Queued {
            id: Uuid::new_v4(),
            content: "first".into(),
            metadata: None,
            author: Uuid::new_v4(),
        };
        let second = Queued {
            id: Uuid::new_v4(),
            content: "second".into(),
            ..first.clone()
        };
        queue::with(chat_id, |queue| queue.submit(first, false));
        queue::with(chat_id, |queue| queue.submit(second.clone(), false));

        let (frames, mut events) = stream.join().await;
        let [
            ServerMessage::Queue {
                messages,
                held,
                started,
            },
        ] = frames.as_slice()
        else {
            panic!(
                "a reload sees the queue: {}",
                frame_kinds(&frames).join(",")
            );
        };
        assert_eq!(messages, std::slice::from_ref(&second));
        assert!(!held);
        assert!(started.is_none());

        queue::with(chat_id, |queue| queue.remove(second.id));
        announce(&stream, None).await;
        let Ok(ServerMessage::Queue { messages, .. }) = events.recv().await else {
            panic!("every connection hears the queue change");
        };
        assert!(messages.is_empty());
        queue::with(chat_id, |queue| queue.finish(true));
    }

    #[test]
    fn a_queue_frame_carries_what_the_console_renders() {
        let id = Uuid::new_v4();
        let frame = ServerMessage::Queue {
            messages: vec![Queued {
                id,
                content: "next".into(),
                metadata: Some(serde_json::json!({ "attachments": [] })),
                author: Uuid::new_v4(),
            }],
            held: true,
            started: None,
        };
        assert_eq!(
            serde_json::to_value(&frame).unwrap(),
            serde_json::json!({
                "type": "queue",
                "messages": [{ "id": id, "content": "next", "metadata": { "attachments": [] } }],
                "held": true,
            })
        );
    }

    #[test]
    fn queue_actions_deserialize() {
        let id = Uuid::new_v4();
        let send: ClientMessage = serde_json::from_value(
            serde_json::json!({ "type": "send", "id": id, "content": "hi" }),
        )
        .unwrap();
        assert!(matches!(send, ClientMessage::Send { id: sent, .. } if sent == id));
        let interrupt: ClientMessage =
            serde_json::from_value(serde_json::json!({ "type": "interrupt", "id": id })).unwrap();
        assert!(matches!(interrupt, ClientMessage::Interrupt { id: target } if target == id));
        let edit: ClientMessage = serde_json::from_value(
            serde_json::json!({ "type": "edit_queued", "id": id, "content": "changed" }),
        )
        .unwrap();
        assert!(matches!(edit, ClientMessage::EditQueued { content, .. } if content == "changed"));
        let remove: ClientMessage =
            serde_json::from_value(serde_json::json!({ "type": "remove_queued", "id": id }))
                .unwrap();
        assert!(matches!(remove, ClientMessage::RemoveQueued { id: target } if target == id));
    }

    /// A user with a live session, and a chat in a workspace it can write to.
    async fn seed_chat(pool: &PgPool, secret: &str) -> (String, Uuid) {
        let suffix = Uuid::new_v4().simple().to_string();
        let user = db::users::create_user(
            pool,
            &format!("join-{suffix}@example.com"),
            "unused-hash",
            None,
            false,
        )
        .await
        .unwrap();
        let organization = db::organizations::create_organization(
            pool,
            &format!("Join {suffix}"),
            &format!("join-org-{suffix}"),
            None,
        )
        .await
        .unwrap();
        let workspace = workspaces::create_workspace(
            pool,
            organization.id,
            &format!("Join {suffix}"),
            &format!("join-ws-{suffix}"),
            None,
        )
        .await
        .unwrap();
        workspace_members::add_member(
            pool,
            workspace.id,
            user.id,
            workspace_members::WorkspaceRole::Owner,
            None,
        )
        .await
        .unwrap();
        let chat = chats::create_chat(
            pool,
            Some(workspace.id),
            "Join",
            "llama3.2:3b",
            false,
            false,
        )
        .await
        .unwrap();
        let session = sessions::create_session(
            pool,
            user.id,
            &format!("refresh-{suffix}"),
            None,
            None,
            None,
            (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
        )
        .await
        .unwrap();
        let token = crate::auth::jwt::create_session_access_token(
            user.id,
            &user.email,
            Vec::new(),
            Vec::new(),
            false,
            session.id,
            secret,
            chrono::Duration::minutes(15),
        )
        .unwrap();
        (token, chat.id)
    }

    /// The `type` of the next text frame, ignoring control frames.
    async fn frame_type(
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> String {
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(10), socket.next())
                .await
                .expect("a frame within ten seconds")
                .expect("an open socket")
                .expect("a readable frame");
            if let tokio_tungstenite::tungstenite::Message::Text(text) = frame {
                let frame: serde_json::Value = serde_json::from_str(&text).unwrap();
                return frame["type"].as_str().unwrap().to_string();
            }
        }
    }

    /// Holding the live-turn lock stops `join` from completing, so a socket
    /// that announces `init` while it is held has announced before it could
    /// receive anything, and a frame published in that gap would never reach it.
    /// The handler takes its own hold of the stream right before it joins, so
    /// the strong count rising to two is the moment it is parked on the lock.
    #[tokio::test]
    async fn init_is_not_announced_until_the_socket_has_joined_the_stream() {
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://postgres:postgres@localhost:5432/zone_test".to_string()
        });
        let pool = PgPool::connect(&database_url)
            .await
            .expect("a migrated test database");
        let config = crate::state::test_config();
        let (token, chat_id) = seed_chat(&pool, config.jwt_secret()).await;
        let state = AppState::new(config, pool, None);
        state.disable_mcp();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, crate::routes::create_router(state)).await;
        });

        let stream = ChatStream::of(chat_id);
        let joining = stream.live.lock().await;
        let (mut socket, _) =
            tokio_tungstenite::connect_async(format!("ws://{address}/ws/chats/{chat_id}"))
                .await
                .unwrap();
        socket
            .send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::json!({"type": "auth", "token": token})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let arrived = tokio::time::Instant::now() + Duration::from_secs(10);
        while Arc::strong_count(&stream) < 2 {
            assert!(
                tokio::time::Instant::now() < arrived,
                "the handler never took hold of the chat's stream"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(500), socket.next())
                .await
                .is_err(),
            "init was announced before the socket had joined the chat's stream"
        );

        drop(joining);
        assert_eq!(frame_type(&mut socket).await, "init");
        publish(&stream, started(Uuid::new_v4())).await;
        assert_eq!(
            frame_type(&mut socket).await,
            "message_start",
            "a frame published after init has to reach the socket"
        );
    }

    #[tokio::test]
    async fn generation_registration_reports_cancellation_and_cleans_up() {
        let chat_id = Uuid::new_v4();
        let stream = ChatStream::of(chat_id);
        let mut events = stream.events.subscribe();
        let mut generation = Generation::new(chat_id);
        let key = (chat_id, generation.message_id);
        assert!(CHAT_CANCELLATIONS.contains_key(&key));
        assert!(!generation.is_cancelled());

        CHAT_CANCELLATIONS.get(&key).unwrap().send(()).unwrap();
        assert!(generation.is_cancelled());
        generation.cancelled(&stream).await;
        assert!(matches!(
            events.recv().await,
            Ok(ServerMessage::Cancelled { message_id: None })
        ));

        generation.started = true;
        generation.cancelled(&stream).await;
        assert!(matches!(
            events.recv().await,
            Ok(ServerMessage::Cancelled { message_id: Some(id) }) if id == generation.message_id
        ));
        drop(generation);
        assert!(!CHAT_CANCELLATIONS.contains_key(&key));
    }

    #[tokio::test(start_paused = true)]
    async fn a_video_job_comfy_leaves_silent_for_over_two_hours_keeps_reporting() {
        const SILENCE: Duration = Duration::from_secs(2 * 60 * 60 + 60);
        const LONGEST_QUIET: Duration = Duration::from_secs(60);
        const GENERATING: &str = "Generating video...";

        let stream = ChatStream::of(Uuid::new_v4());
        let mut events = stream.events.subscribe();
        let listener = tokio::spawn(async move {
            let mut heard = Vec::new();
            loop {
                match events.recv().await {
                    Ok(ServerMessage::Status { message }) => {
                        heard.push((tokio::time::Instant::now(), message));
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return heard,
                }
            }
        });
        let mut progress = MediaProgress::start(&stream);
        let sender = progress.sender();
        let silent_comfy = async move {
            let _ = sender.send("Video queued...".to_string());
            let _ = sender.send(GENERATING.to_string());
            tokio::time::sleep(SILENCE).await;
            "rendered"
        };
        let (cancellation, _) = broadcast::channel(1);
        let started = tokio::time::Instant::now();

        let rendered = await_media(
            std::future::pending(),
            cancellation,
            silent_comfy,
            &mut progress,
        )
        .await
        .expect("nothing took the lease");

        assert_eq!(rendered, "rendered");
        assert!(
            started.elapsed() >= SILENCE,
            "the job was cut short after {:?}",
            started.elapsed()
        );
        progress.stop().await;
        drop(stream);
        let heard = listener.await.expect("the listener ends with the stream");
        let mut previous = started;
        for (at, _) in &heard {
            let quiet = at.duration_since(previous);
            assert!(
                quiet <= LONGEST_QUIET,
                "the console heard nothing for {quiet:?} while ComfyUI was silent"
            );
            previous = *at;
        }
        let tail = (started + SILENCE).saturating_duration_since(previous);
        assert!(
            tail <= LONGEST_QUIET,
            "the console heard nothing for the last {tail:?} of the job"
        );
        assert!(
            heard
                .iter()
                .skip(2)
                .all(|(_, message)| message.starts_with(GENERATING)),
            "a status during the silence has to say what the job is doing: {heard:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_job_queued_behind_another_keeps_reporting_while_it_waits() {
        const OTHER_JOB: Duration = Duration::from_secs(2 * 60 * 60);

        let slots = Semaphore::new(1);
        let other = slots.acquire().await.expect("the slot is free");
        let stream = ChatStream::of(Uuid::new_v4());
        let mut events = stream.events.subscribe();
        let listener = tokio::spawn(async move {
            let mut heard = Vec::new();
            loop {
                match events.recv().await {
                    Ok(ServerMessage::Status { message }) => {
                        heard.push((tokio::time::Instant::now(), message));
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return heard,
                }
            }
        });
        let mut progress = MediaProgress::start(&stream);
        let started = tokio::time::Instant::now();

        let (permit, replay) = tokio::join!(queue_media(&slots, &progress), async {
            tokio::time::sleep(OTHER_JOB / 2).await;
            let (replay, _) = stream.join().await;
            tokio::time::sleep(OTHER_JOB / 2).await;
            drop(other);
            replay
        });

        assert!(
            started.elapsed() >= OTHER_JOB,
            "the queued job ran alongside the other after {:?}",
            started.elapsed()
        );
        progress.stop().await;
        drop(permit);
        drop(stream);
        let heard = listener.await.expect("the listener ends with the stream");
        assert_eq!(
            heard.first().map(|(_, message)| message.as_str()),
            Some(MEDIA_QUEUED),
            "the wait has to say why nothing is happening yet: {heard:?}"
        );
        let mut previous = started;
        for (at, message) in &heard {
            let quiet = at.duration_since(previous);
            assert!(
                quiet <= MEDIA_HEARTBEAT,
                "the console heard nothing for {quiet:?} while the slot was taken"
            );
            assert!(
                message.starts_with(MEDIA_QUEUED),
                "a status during the wait has to say it is waiting: {message}"
            );
            previous = *at;
        }
        let tail = (started + OTHER_JOB).saturating_duration_since(previous);
        assert!(
            tail <= MEDIA_HEARTBEAT,
            "the console heard nothing for the last {tail:?} of the wait"
        );
        assert!(
            matches!(
                replay.as_slice(),
                [ServerMessage::Status { message }] if message.starts_with(MEDIA_QUEUED)
            ),
            "a console that reconnects during the wait has to see it: {replay:?}"
        );
    }

    #[tokio::test]
    async fn a_free_slot_starts_without_saying_it_waited() {
        let slots = Semaphore::new(1);
        let stream = ChatStream::of(Uuid::new_v4());
        let mut events = stream.events.subscribe();
        let mut progress = MediaProgress::start(&stream);

        let permit = queue_media(&slots, &progress).await;
        progress.stop().await;

        drop(permit);
        assert!(
            events.try_recv().is_err(),
            "nothing was ahead of the job, so there was nothing to wait for"
        );
    }

    #[test]
    fn elapsed_labels_read_in_seconds_then_whole_minutes() {
        assert_eq!(elapsed_label(Duration::from_secs(30)), "30 s");
        assert_eq!(elapsed_label(Duration::from_secs(59)), "59 s");
        assert_eq!(elapsed_label(Duration::from_secs(60)), "1 min");
        assert_eq!(elapsed_label(Duration::from_secs(7_259)), "120 min");
    }

    #[tokio::test(start_paused = true)]
    async fn connection_cleanup_removes_only_an_idle_chat_entry() {
        let chat_id = Uuid::new_v4();
        let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS_PER_CHAT));
        CHAT_CONNECTIONS.insert(chat_id, semaphore.clone());
        drop(ConnectionCleanupGuard {
            chat_id,
            semaphore: semaphore.clone(),
        });
        tokio::time::sleep(Duration::from_millis(75)).await;
        assert!(!CHAT_CONNECTIONS.contains_key(&chat_id));

        let active_chat = Uuid::new_v4();
        let active = Arc::new(Semaphore::new(MAX_CONNECTIONS_PER_CHAT));
        let permit = active.clone().acquire_owned().await.unwrap();
        CHAT_CONNECTIONS.insert(active_chat, active.clone());
        drop(ConnectionCleanupGuard {
            chat_id: active_chat,
            semaphore: active,
        });
        tokio::time::sleep(Duration::from_millis(75)).await;
        assert!(CHAT_CONNECTIONS.contains_key(&active_chat));
        drop(permit);
        CHAT_CONNECTIONS.remove(&active_chat);
    }

    #[test]
    fn generation_deadlines_accept_normal_timeouts_and_reject_overflow() {
        assert!(generation_deadline(Duration::from_secs(30)).is_ok());
        assert!(generation_deadline(Duration::MAX).is_err());
    }

    #[test]
    fn queued_generation_cannot_replace_the_active_approval_gate() {
        let chat = Uuid::new_v4();
        let active = crate::agent::ApprovalPolicy::required(crate::agent::ApprovalGate::new());
        crate::agent::ApprovalPolicy::register(chat, active.clone());
        let queued = Generation::new(chat);
        crate::agent::ApprovalPolicy::set_chat_auto(chat, true);
        assert!(active.is_auto());
        drop(queued);
        assert!(active.is_auto());
        crate::agent::ApprovalPolicy::unregister(chat, &active);
    }

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
            ClientMessage::Send {
                content, metadata, ..
            } => {
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
            ClientMessage::Send {
                content, metadata, ..
            } => {
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
    fn a_stored_file_and_its_attachment_always_agree() {
        // Whatever ComfyUI names the format, the extension the artifact is
        // stored under and the media type announced for it come from one entry,
        // so a URL can never contradict the attachment beside it.
        for (returned, fallback, extension, mime) in [
            ("image/png", MediaType::PNG, "png", "image/png"),
            ("image/jpeg", MediaType::PNG, "jpg", "image/jpeg"),
            ("image/webp", MediaType::PNG, "webp", "image/webp"),
            ("video/webm", MediaType::WEBM, "webm", "video/webm"),
            ("video/mp4", MediaType::WEBM, "mp4", "video/mp4"),
            ("audio/flac", MediaType::FLAC, "flac", "audio/flac"),
        ] {
            let media = stored_media(returned, fallback);
            assert_eq!(media.extension, extension, "{returned}");
            assert_eq!(media.mime, mime, "{returned}");
            let url = format!("/api/artifacts/w/c/m/file.{}", media.extension);
            let attachment = generated_media_attachment(&url, media.mime, 0)
                .expect("a stored artifact is always attachable");
            assert_eq!(attachment.mime, media.mime, "{returned}");
            assert!(attachment.name.ends_with(media.extension), "{returned}");
        }
    }

    #[test]
    fn a_format_outside_the_job_lane_falls_back_rather_than_mislabelling() {
        // A video job that somehow reports a picture must not store a .png the
        // player will then be handed as a clip.
        assert_eq!(stored_media("image/png", MediaType::WEBM), MediaType::WEBM);
        assert_eq!(stored_media("video/webm", MediaType::FLAC), MediaType::FLAC);
        assert_eq!(stored_media("audio/flac", MediaType::PNG), MediaType::PNG);
        // An unknown format falls back within its own lane.
        assert_eq!(
            stored_media("video/x-matroska", MediaType::WEBM),
            MediaType::WEBM
        );
        assert_eq!(stored_media("", MediaType::PNG), MediaType::PNG);
    }

    #[test]
    fn every_job_reports_an_unreachable_comfyui_the_same_way() {
        let error = abnegate_comfy::Error::Configuration("prompt is empty or too long");
        for subject in [
            "Image generation",
            "Video generation",
            "Audio generation",
            "Upscaling",
        ] {
            let message = comfy_failure(subject, &error);
            assert!(message.starts_with(subject), "{message}");
            assert!(message.contains("prompt is empty or too long"), "{message}");
        }
        assert_eq!(
            comfy_failure("Upscaling", &abnegate_comfy::Error::Disabled),
            "Upscaling failed: ComfyUI is disabled"
        );
    }

    #[tokio::test]
    async fn comfy_connection_and_timeout_failures_have_actionable_messages() {
        let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed_address = closed.local_addr().unwrap();
        drop(closed);
        let connection = reqwest::get(format!("http://{closed_address}"))
            .await
            .expect_err("the listener was closed before the request");
        assert_eq!(
            comfy_failure("Image generation", &abnegate_comfy::Error::Http(connection)),
            "Image generation failed: cannot reach ComfyUI. Start the image service and try again."
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let blocker = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let timeout = reqwest::Client::builder()
            .timeout(Duration::from_millis(20))
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .expect_err("the accepted request never receives an HTTP response");
        assert_eq!(
            comfy_failure("Video generation", &abnegate_comfy::Error::Http(timeout)),
            "Video generation failed: ComfyUI did not respond in time. Check the image service and try again."
        );
        blocker.abort();
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
        assert!(generated_image_attachment("data:image/png;base64", 0).is_none());
        assert!(generated_media_attachment("ftp://example.test/a.png", "image/png", 0).is_none());
        assert!(
            generated_media_attachment(
                &format!(
                    "data:image/png;base64,{}",
                    "x".repeat(MAX_GENERATED_IMAGE_URL_LENGTH)
                ),
                "image/png",
                0
            )
            .is_none()
        );
        assert!(image_metadata(&[]).is_none());
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
    fn test_generated_video_attachment_ignores_non_media_mime() {
        let attachment = generated_media_attachment(
            "/api/artifacts/ws/chat/msg/clip.webm",
            "application/octet-stream",
            0,
        )
        .expect("valid video");
        assert_eq!(attachment.name, "generated-video-1.webm");
        assert_eq!(attachment.mime, "video/webm");
    }

    #[test]
    fn generated_audio_attachment_keeps_audio_mime() {
        let attachment = generated_media_attachment("/api/artifacts/w/c/m/x.flac", "audio/flac", 0)
            .expect("valid audio");
        assert_eq!(attachment.mime, "audio/flac");
        assert_eq!(attachment.name, "generated-audio-1.flac");

        let inline = generated_media_attachment("data:audio/flac;base64,abc", "", 0)
            .expect("inline audio must not be rejected as non-media");
        assert_eq!(inline.mime, "audio/flac");
    }

    #[test]
    fn generated_audio_attachment_infers_mime_from_extension() {
        let attachment =
            generated_media_attachment("/api/artifacts/w/c/m/x.flac", "", 0).expect("valid audio");
        assert_eq!(attachment.mime, "audio/flac");
        assert_eq!(attachment.name, "generated-audio-1.flac");
    }

    /// A tool may hand back an external URL with any casing. Inferring the type
    /// case-sensitively rendered those clips in an `<img>` element.
    #[test]
    fn extension_inference_ignores_url_casing() {
        for url in [
            "https://example.test/clip.FLAC",
            "https://example.test/clip.Flac",
        ] {
            let attachment =
                generated_media_attachment(url, "", 0).unwrap_or_else(|| panic!("{url} is audio"));
            assert_eq!(attachment.mime, "audio/flac", "{url}");
            assert_eq!(attachment.name, "generated-audio-1.flac", "{url}");
        }

        let video = generated_media_attachment("https://example.test/clip.WEBM", "", 0)
            .expect("uppercase video");
        assert_eq!(video.mime, "video/webm");
    }

    #[test]
    fn opus_attachments_are_ogg_not_the_rtp_payload_type() {
        let inferred =
            generated_media_attachment("/api/artifacts/w/c/m/x.opus", "", 0).expect("valid audio");
        assert_eq!(
            inferred.mime, "audio/ogg",
            "browsers return \"\" from canPlayType(\"audio/opus\") and nosniff blocks recovery"
        );
        assert_eq!(inferred.name, "generated-audio-1.opus");

        let reported = generated_media_attachment("/api/artifacts/w/c/m/x.opus", "audio/opus", 0)
            .expect("valid audio");
        assert_eq!(
            reported.mime, "audio/ogg",
            "metadata must agree with the Content-Type the artifact route serves"
        );
    }

    #[test]
    fn unknown_media_falls_back_within_its_own_lane() {
        let audio = generated_media_attachment("/api/artifacts/w/c/m/x.flac", "audio/basic", 0)
            .expect("valid audio");
        assert_eq!(audio.mime, "audio/basic");
        assert_eq!(audio.name, "generated-audio-1.flac");

        let video = generated_media_attachment("/api/artifacts/w/c/m/x.webm", "video/quicktime", 0)
            .expect("valid video");
        assert_eq!(video.name, "generated-video-1.webm");

        let image = generated_media_attachment("/api/artifacts/w/c/m/x.png", "image/heic", 0)
            .expect("valid image");
        assert_eq!(image.name, "generated-image-1.png");
    }

    #[test]
    fn test_server_message_message_start_serialize() {
        let msg = ServerMessage::MessageStart {
            message_id: Uuid::new_v4(),
            role: "assistant".to_string(),
            resumed: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"message_start\""));
        assert!(json.contains("\"role\":\"assistant\""));
        assert!(
            !json.contains("resumed"),
            "a fresh start stays on the wire it had: {json}"
        );
    }

    #[test]
    fn resumed_start_tells_the_client_to_revive_the_message() {
        let msg = ServerMessage::MessageStart {
            message_id: Uuid::new_v4(),
            role: "assistant".to_string(),
            resumed: true,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"resumed\":true"), "{json}");
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
    fn test_server_message_reasoning_serialize() {
        let msg = ServerMessage::Reasoning {
            content: "The capital is Paris.".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"reasoning\""));
        assert!(json.contains("\"content\":\"The capital is Paris.\""));
    }

    #[test]
    fn test_server_message_tool_call_serialize() {
        let msg = ServerMessage::ToolCall {
            message_id: Uuid::new_v4(),
            tool_call_id: "call_abc".to_string(),
            name: "search_knowledge".to_string(),
            arguments: r#"{"query":"deploys"}"#.to_string(),
            at: 0,
            reasoning: Some("Need workspace deploy docs.".to_string()),
            reasoning_at: None,
            reason: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"tool_call\""));
        assert!(json.contains("\"tool_call_id\":\"call_abc\""));
        assert!(json.contains("\"name\":\"search_knowledge\""));
        assert!(json.contains("deploys"));
        assert!(json.contains("Need workspace deploy docs."));
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
            refusal: None,
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
            at: None,
            reasoning: None,
            reasoning_at: None,
            reason: None,
            preview: None,
            questions: Vec::new(),
            job: None,
            waiting: None,
            refusal: None,
        }];
        let metadata = serde_json::json!({ "tool_calls": records });

        assert_eq!(metadata["tool_calls"][0]["name"], "list_tasks");
        assert_eq!(metadata["tool_calls"][0]["success"], true);
        assert_eq!(metadata["tool_calls"][0]["duration_ms"], 7);
        assert!(metadata["tool_calls"][0].get("reason").is_none());
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
            at: None,
            reasoning: Some("Inspect the workspace first.".to_string()),
            reasoning_at: None,
            reason: Some("The user asked which tests are failing.".to_string()),
            preview: None,
            questions: Vec::new(),
            job: None,
            waiting: None,
            refusal: None,
        }];

        let merged = merge_metadata(images, &records, &[], &[], None, &[], None)
            .expect("both sides produce metadata");
        assert_eq!(merged["attachments"][0]["name"], "generated-image-1.png");
        assert_eq!(merged["tool_calls"][0]["name"], "run_shell");
        assert_eq!(
            merged["tool_calls"][0]["reasoning"],
            "Inspect the workspace first."
        );
        assert_eq!(
            merged["tool_calls"][0]["reason"],
            "The user asked which tests are failing."
        );
    }

    #[test]
    fn test_merge_metadata_keeps_citations_with_source_and_revision() {
        let citations = vec![crate::agent::Citation {
            kind: crate::agent::CitationKind::GithubBuild,
            title: "repository main@aaaaaaa".into(),
            url: "https://github.com/owner/repository/commit/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            identifier: None,
            revision: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
            observed_at: "2026-09-05T00:00:00+00:00".into(),
            complete: false,
            provenance: crate::agent::verification::Provenance::ServerExecution,
            outcome: crate::agent::CitationOutcome::Incomplete,
            note: Some("Observed CI only".into()),
        }];
        let merged = merge_metadata(None, &[], &citations, &[], None, &[], None)
            .expect("citations produce metadata");
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
            reason: None,
        }];
        let merged = merge_metadata(None, &[], &[], &receipts, None, &[], None)
            .expect("receipts produce metadata");
        assert_eq!(merged["action_receipts"][0]["action"], "create_task");
        assert_eq!(merged["action_receipts"][0]["href"], "/tasks?id=task-1");
    }

    #[test]
    fn test_merge_metadata_is_none_when_the_turn_produced_neither() {
        assert!(merge_metadata(None, &[], &[], &[], None, &[], None).is_none());
    }

    /// The trace is workspace-readable and memory is one person's, so a memory
    /// call leaves its name, outcome and timing and nothing it carried — in the
    /// record that rides the message and in the frame that goes out live. The
    /// receipts fix covered neither, and the outcome line is the result's first
    /// line, which for a read is the stored content itself.
    #[test]
    fn a_memory_call_leaves_no_private_data_in_the_trace() {
        use crate::agent::memory::{MEMORY_READ, MEMORY_WRITE, TRACE_DETAIL};
        let private = [
            "Deploy window",
            "When we ship",
            "Thursdays",
            "never Fridays",
            "ship on Thursdays",
        ];

        let (record, frame) = trace_started(
            Uuid::new_v4(),
            "call_1".to_string(),
            MEMORY_WRITE.to_string(),
            r#"{"category":"fact","name":"Deploy window","description":"When we ship.","content":"Thursdays, never Fridays.","reason":"They ship on Thursdays, so remembering it."}"#,
            0,
            Some(Thought {
                text: "I should remember this.".to_string(),
                at: 0,
            }),
        );
        let record_json = serde_json::to_string(&record).expect("a record serialises");
        let frame_json = serde_json::to_string(&frame).expect("a frame serialises");
        for leaked in private {
            assert!(
                !record_json.contains(leaked),
                "the record carries {leaked:?}: {record_json}"
            );
            assert!(
                !frame_json.contains(leaked),
                "the live frame carries {leaked:?}: {frame_json}"
            );
        }
        assert!(
            record_json.contains(r#"\"category\":\"fact\""#),
            "the kind survives, as on a receipt: {record_json}"
        );
        assert_eq!(
            record.name, MEMORY_WRITE,
            "the name is what the badge and the receipt key on"
        );
        assert_eq!(
            record.reason, None,
            "the stated reason is one more field the model wrote"
        );

        // A read's outcome line would be the entry's first line.
        let shown =
            crate::agent::memory::trace_detail(MEMORY_READ, "Thursdays, never Fridays. (2 lines)");
        assert_eq!(shown, TRACE_DETAIL);

        // Every other tool's trace is exactly what it was.
        let (record, frame) = trace_started(
            Uuid::new_v4(),
            "call_2".to_string(),
            "read_file".to_string(),
            r#"{"path":"src/main.rs","reason":"To see what main does."}"#,
            0,
            None,
        );
        assert!(
            serde_json::to_string(&record)
                .unwrap()
                .contains("src/main.rs")
        );
        assert_eq!(record.reason.as_deref(), Some("To see what main does."));
        assert!(
            serde_json::to_string(&frame)
                .unwrap()
                .contains("To see what main does.")
        );
        assert_eq!(
            crate::agent::memory::trace_detail("read_file", "fn main() {} (12 lines)"),
            "fn main() {} (12 lines)"
        );
    }

    fn call(name: &str, success: bool) -> ToolCallRecord {
        ToolCallRecord {
            id: "call_1".to_string(),
            name: name.to_string(),
            arguments: "{}".to_string(),
            success,
            detail: "ok".to_string(),
            duration_ms: 1,
            at: None,
            reasoning: None,
            reasoning_at: None,
            reason: None,
            preview: None,
            questions: Vec::new(),
            job: None,
            waiting: None,
            refusal: None,
        }
    }

    /// The badge is derived from the turn's own calls, so it is a claim the
    /// server made rather than one the model made about itself.
    #[test]
    fn test_merge_metadata_flags_a_turn_that_read_memory() {
        let images = image_metadata(&[ChatImageAttachment {
            name: "generated-image-1.png".to_string(),
            mime: "image/png".to_string(),
            url: "https://example.test/one.png".to_string(),
        }]);
        let records = vec![
            call("read_file", true),
            call(crate::agent::memory::MEMORY_READ, true),
        ];

        let merged = merge_metadata(images, &records, &[], &[], None, &[], None)
            .expect("the turn had calls");

        assert_eq!(merged[MEMORY_USED_KEY], true);
        assert_eq!(merged["attachments"][0]["name"], "generated-image-1.png");
        assert_eq!(merged["tool_calls"].as_array().expect("the trace").len(), 2);
    }

    /// A read that failed read nothing, a write is not a read, and a turn with
    /// no calls has nothing to derive the badge from. The key is absent in all
    /// three rather than present and false, which is what the console's
    /// optional boolean expects.
    #[test]
    fn test_merge_metadata_leaves_the_flag_off_without_a_successful_read() {
        let failed = merge_metadata(
            None,
            &[call(crate::agent::memory::MEMORY_READ, false)],
            &[],
            &[],
            None,
            &[],
            None,
        )
        .expect("the turn had calls");
        assert!(failed.get(MEMORY_USED_KEY).is_none(), "{failed}");

        let wrote = merge_metadata(
            None,
            &[call(crate::agent::memory::MEMORY_WRITE, true)],
            &[],
            &[],
            None,
            &[],
            None,
        )
        .expect("the turn had calls");
        assert!(wrote.get(MEMORY_USED_KEY).is_none(), "{wrote}");

        let quiet = merge_metadata(
            None,
            &[],
            &[],
            &[],
            Some(&Thinking {
                text: "Thinking.".to_string(),
                at: 0,
            }),
            &[],
            None,
        )
        .expect("reasoning");
        assert!(quiet.get(MEMORY_USED_KEY).is_none(), "{quiet}");
    }

    #[test]
    fn test_merge_metadata_keeps_reasoning() {
        let merged = merge_metadata(
            None,
            &[],
            &[],
            &[],
            Some(&Thinking {
                text: "The capital is Paris.".to_string(),
                at: 0,
            }),
            &[],
            None,
        )
        .expect("reasoning");
        assert_eq!(merged["reasoning"], "The capital is Paris.");
        assert_eq!(merged["reasoning_at"], 0);

        let merged = merge_metadata(
            Some(serde_json::json!("invalid image metadata")),
            &[],
            &[],
            &[],
            Some(&Thinking {
                text: "Recovered".to_string(),
                at: 0,
            }),
            &[],
            None,
        )
        .expect("reasoning replaces malformed metadata");
        assert_eq!(
            merged,
            serde_json::json!({"reasoning": "Recovered", "reasoning_at": 0})
        );
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
                reason: None,
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
        assert_eq!(AUTH_RECHECK_INTERVAL, Duration::from_secs(10));
        assert_eq!(WS_IDLE_TIMEOUT_SECS, 300);
        assert_eq!(WS_PING_INTERVAL_SECS, 30);
        assert_eq!(MAX_CONSECUTIVE_ERRORS, 5);
        assert_eq!(MAX_CONNECTIONS_PER_CHAT, 5);
        assert_eq!(MAX_MESSAGES_PER_MINUTE, 20);
        assert_eq!(MAX_MESSAGE_LENGTH, 100_000);
        assert_eq!(MAX_CONTEXT_RESULTS, 10);
        assert_eq!(MAX_CONTEXT_IN_PROMPT, 5);
        assert_eq!(MAX_RESPONSE_LENGTH, 100_000);
        assert_eq!(MAX_GENERATED_IMAGES, 8);
        assert_eq!(STATUS_CONNECTED, "connected");
    }

    #[test]
    fn retrieved_context_wraps_untrusted_source_lines() {
        let block = session::prompt::retrieved(&[
            "- [knowledge] Notes (knowledge://1): should_skip_blob".to_string(),
        ]);
        assert!(block.contains("<retrieved_context>"));
        assert!(block.contains("untrusted source data"));
        assert!(block.contains("knowledge://1"));
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
            ServerMessage::Audio {
                message_id: Uuid::new_v4(),
                attachment: ChatImageAttachment {
                    name: "generated-audio-1.flac".to_string(),
                    mime: "audio/flac".to_string(),
                    url: "/api/artifacts/w/c/m/x.flac".to_string(),
                },
            },
        ];

        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            // Just verify it can be serialized without panicking
            assert!(!json.is_empty());
            if matches!(msg, ServerMessage::Audio { .. }) {
                assert!(json.contains("\"type\":\"audio\""), "{json}");
            }
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
        for invalid in [
            serde_json::json!({"role":"user","content":"x"}),
            serde_json::json!({"id":Uuid::new_v4(),"content":"x"}),
            serde_json::json!({"id":Uuid::new_v4(),"role":3,"content":"x"}),
            serde_json::json!({"id":Uuid::new_v4(),"role":"user"}),
            serde_json::json!({"id":Uuid::new_v4(),"role":"user","content":false}),
        ] {
            assert!(saved_action(&invalid).is_none(), "{invalid}");
        }
        let without_metadata = saved_action(&serde_json::json!({
            "id": Uuid::new_v4(),
            "role": "user",
            "content": "x",
            "metadata": null
        }))
        .unwrap();
        assert!(matches!(
            without_metadata,
            ServerMessage::MessageSaved { metadata: None, .. }
        ));
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
            arguments: r#"{"path":"x","reason":"Persist the config the user dictated."}"#.into(),
            reason: Some("Persist the config the user dictated.".into()),
            preview: Some("Write 12 characters to x, replacing whatever is there.".into()),
        })
        .unwrap();
        assert_eq!(json["type"], "tool_approval_required");
        assert_eq!(json["tool_call_id"], "call_1");
        assert_eq!(json["name"], "write_file");
        assert_eq!(json["reason"], "Persist the config the user dictated.");
        assert_eq!(
            json["preview"],
            "Write 12 characters to x, replacing whatever is there."
        );
    }

    #[test]
    fn an_approval_request_without_a_reason_omits_the_field() {
        let json = serde_json::to_value(ServerMessage::ToolApprovalRequired {
            message_id: Uuid::nil(),
            tool_call_id: "call_1".into(),
            name: "run_shell".into(),
            arguments: r#"{"command":"ls"}"#.into(),
            reason: None,
            preview: None,
        })
        .unwrap();
        assert_eq!(json["type"], "tool_approval_required");
        assert!(json.get("reason").is_none());
        assert!(json.get("preview").is_none());
    }

    #[test]
    fn a_started_call_records_where_in_the_answer_it_ran_on_the_frame_and_the_record() {
        let (record, frame) = trace_started(
            Uuid::nil(),
            "call_1".to_string(),
            "read_file".to_string(),
            r#"{"path":"src/main.rs"}"#,
            13,
            Some(Thought {
                text: "Read main first.".to_string(),
                at: 5,
            }),
        );

        assert_eq!(record.at, Some(13), "a reload draws the call where it ran");
        assert_eq!(
            record.reasoning_at,
            Some(5),
            "and its thinking where it began"
        );
        let json = serde_json::to_value(frame).unwrap();
        assert_eq!(json["at"], 13, "the live trace draws the call where it ran");
        assert_eq!(json["reasoning_at"], 5);
    }

    #[test]
    fn tool_call_carries_the_stated_reason_to_the_console() {
        let json = serde_json::to_value(ServerMessage::ToolCall {
            message_id: Uuid::nil(),
            tool_call_id: "call_1".into(),
            name: "create_pull_request".into(),
            arguments: r#"{"title":"Fix the export","reason":"The user asked me to open it."}"#
                .into(),
            at: 0,
            reasoning: None,
            reasoning_at: None,
            reason: Some("The user asked me to open it.".into()),
        })
        .unwrap();
        assert_eq!(json["type"], "tool_call");
        assert_eq!(json["name"], "create_pull_request");
        assert_eq!(json["reason"], "The user asked me to open it.");
        assert!(json.get("reasoning").is_none());
    }

    #[test]
    fn the_job_and_wait_frames_carry_the_tags_and_keys_the_console_reads() {
        let message_id = Uuid::new_v4();
        let started = serde_json::to_value(ServerMessage::JobStarted {
            message_id,
            tool_call_id: "call_1".into(),
            job: spawned(),
        })
        .unwrap();
        assert_eq!(started["type"], "job_started");
        assert_eq!(started["message_id"], message_id.to_string());
        assert_eq!(started["tool_call_id"], "call_1");
        assert_eq!(started["job"]["id"], JOB_ID);
        assert_eq!(started["job"]["pid"], 48213);
        assert_eq!(started["job"]["log_path"], JOB_LOG);

        let exited = serde_json::to_value(ServerMessage::JobExited {
            message_id,
            job: job::JobExit {
                id: JOB_ID.into(),
                exit_code: Some(1),
            },
        })
        .unwrap();
        assert_eq!(exited["type"], "job_exited");
        assert_eq!(exited["message_id"], message_id.to_string());
        assert_eq!(exited["job"]["id"], JOB_ID);
        assert_eq!(exited["job"]["exit_code"], 1);

        let opened = serde_json::to_value(ServerMessage::WaitStarted {
            message_id,
            tool_call_id: "call_2".into(),
            waiting: waiting(),
        })
        .unwrap();
        assert_eq!(opened["type"], "wait_started");
        assert_eq!(opened["message_id"], message_id.to_string());
        assert_eq!(opened["tool_call_id"], "call_2");
        assert_eq!(opened["waiting"]["kind"], "job");
        assert_eq!(opened["waiting"]["id"], JOB_ID);
        assert_eq!(opened["waiting"]["deadline"], "2026-09-13T00:05:00+00:00");
        assert!(
            opened["waiting"].get("reference").is_none(),
            "a job wait has no ref to report: {opened}"
        );

        let exit = wait::job_exited(JOB_ID, 0, Duration::from_secs(214));
        let settled = serde_json::to_value(ServerMessage::WaitSettled {
            message_id,
            settled: WaitSettled {
                tool_call_id: "call_2".into(),
                outcome: exit.text,
                verdict: exit.verdict,
            },
        })
        .unwrap();
        assert_eq!(settled["type"], "wait_settled");
        assert_eq!(settled["message_id"], message_id.to_string());
        assert_eq!(settled["settled"]["tool_call_id"], "call_2");
        assert_eq!(
            settled["settled"]["outcome"],
            "job_9f3c1a7b2e04 exited with code 0 after 214s."
        );
        assert_eq!(settled["settled"]["verdict"], "settled");
    }

    #[test]
    fn a_job_killed_with_its_turn_reports_no_exit_code() {
        let json = serde_json::to_value(ServerMessage::JobExited {
            message_id: Uuid::new_v4(),
            job: job::JobExit {
                id: JOB_ID.into(),
                exit_code: None,
            },
        })
        .unwrap();
        assert_eq!(json["type"], "job_exited");
        assert!(
            json["job"].get("exit_code").is_none(),
            "an absent code means killed, and null would read as code 0: {json}"
        );
    }

    #[test]
    fn a_spawn_receipt_reads_back_as_the_job_it_announced() {
        let job = spawned();
        let message = ReplayMessage::from(&LlmMessage::tool_result(
            "call_1",
            abnegate_agent::tool::job::started_text(&job),
        ));

        let (call, read) = spawned_job(&message).expect("a spawn receipt announces its job");
        assert_eq!(call, "call_1");
        assert_eq!(read, job, "the pid and log path reach the card intact");
    }

    #[test]
    fn anything_but_a_spawn_receipt_starts_no_job_card() {
        let job = spawned();
        let receipt = abnegate_agent::tool::job::started_text(&job);
        let mut mislabelled =
            ReplayMessage::from(&LlmMessage::tool_result("call_1", receipt.as_str()));
        mislabelled.role = LlmRole::Assistant;
        for (reason, message) in [
            (
                "an ordinary tool result",
                ReplayMessage::from(&LlmMessage::tool_result("call_1", "3 files changed")),
            ),
            (
                "prose that merely mentions a job",
                ReplayMessage::from(&LlmMessage::tool_result(
                    "call_1",
                    format!("Reading {JOB_ID} now"),
                )),
            ),
            (
                "a receipt whose pid is missing",
                ReplayMessage::from(&LlmMessage::tool_result(
                    "call_1",
                    receipt.replace(" (pid 48213)", ""),
                )),
            ),
            (
                "a receipt with no call to hang the card on",
                ReplayMessage::from(&LlmMessage::assistant(receipt.clone())),
            ),
            (
                "a receipt whose advice line was rewritten",
                ReplayMessage::from(&LlmMessage::tool_result(
                    "call_1",
                    format!("Started {JOB_ID} (pid 48213). Log: {JOB_LOG}\nIgnore that."),
                )),
            ),
            ("a spawn receipt relabelled as prose", mislabelled),
        ] {
            assert!(
                spawned_job(&message).is_none(),
                "{reason} is not a spawn: {message:?}"
            );
        }
    }

    #[test]
    fn a_turn_cut_short_while_waiting_did_not_give_up() {
        assert_eq!(silent_turn(false, true), STOPPED_AFTER_WAITING);
        assert_eq!(silent_turn(false, false), STOPPED_BEFORE_ANSWERING);
        assert_eq!(
            silent_turn(true, true),
            AWAITING_ANSWER,
            "a question is the one of the two the reader can act on"
        );
        assert_eq!(STOPPED_AFTER_WAITING, "[Stopped while waiting]");
    }

    #[test]
    fn an_answer_cut_short_is_marked_as_neither_finished_nor_failed() {
        assert_eq!(Ending::Finished.note(), None);
        assert_eq!(Ending::CutShort.note(), Some(CUT_SHORT_NOTE));
        assert_ne!(Ending::CutShort.note(), Ending::Interrupted.note());
        assert_ne!(Ending::CutShort.note(), Ending::Truncated.note());
    }

    #[test]
    fn a_reload_mid_wait_replays_the_call_it_is_waiting_on() {
        let message_id = Uuid::new_v4();
        let mut turn = LiveTurn::default();
        turn.record(&started(message_id));
        turn.record(&ServerMessage::ToolCall {
            message_id,
            tool_call_id: "call_2".to_string(),
            name: wait::WAIT_FOR.to_string(),
            arguments: "{}".to_string(),
            at: 0,
            reasoning: None,
            reasoning_at: None,
            reason: None,
        });
        turn.record(&ServerMessage::WaitStarted {
            message_id,
            tool_call_id: "call_2".to_string(),
            waiting: waiting(),
        });

        let replay = turn.replay();
        assert_eq!(
            frame_kinds(&replay),
            vec!["message_start", "tool_call", "wait_started"],
            "a reader who reloads mid-wait still sees what is pending: {replay:?}"
        );
        assert!(
            matches!(
                replay.last(),
                Some(ServerMessage::WaitStarted { tool_call_id, waiting, .. })
                    if tool_call_id == "call_2" && waiting.id == JOB_ID
            ),
            "{replay:?}"
        );
    }

    #[test]
    fn a_reload_after_a_settle_replays_how_the_wait_ended() {
        let message_id = Uuid::new_v4();
        let mut turn = LiveTurn::default();
        turn.record(&started(message_id));
        turn.record(&ServerMessage::WaitStarted {
            message_id,
            tool_call_id: "call_2".to_string(),
            waiting: waiting(),
        });
        turn.record(&ServerMessage::JobExited {
            message_id,
            job: job::JobExit {
                id: JOB_ID.into(),
                exit_code: Some(0),
            },
        });
        let exit = wait::job_exited(JOB_ID, 0, Duration::from_secs(214));
        turn.record(&ServerMessage::WaitSettled {
            message_id,
            settled: WaitSettled {
                tool_call_id: "call_2".to_string(),
                outcome: exit.text,
                verdict: exit.verdict,
            },
        });
        // The second run continues the same message, so nothing clears the log.
        turn.record(&chunk("The build passed", 0));

        assert_eq!(
            frame_kinds(&turn.replay()),
            vec![
                "message_start",
                "wait_started",
                "job_exited",
                "wait_settled",
                "chunk"
            ],
            "a second message_start would have cleared everything before it"
        );
    }

    /// The console draws a settled wait on the verdict and nothing else, so
    /// every outcome has to be built with the one its meaning requires. Each
    /// builder is listed here: one added without a verdict of its own reaches
    /// the card as an ordinary settle, which is how the outcome that says out
    /// loud "this is not a pass" came to be drawn as a finished wait.
    #[test]
    fn every_outcome_carries_the_verdict_its_meaning_requires() {
        use crate::agent::wait::Verdict;

        let silent = wait::checks_unknown("main", Duration::from_secs(120));
        let unreadable = wait::checks_unreadable(
            "main",
            "GitHub request failed or timed out.",
            Duration::from_secs(120),
        );
        let ran_out = wait::timed_out(JOB_ID, Duration::from_secs(300));

        for (outcome, verdict) in [
            (
                wait::job_exited(JOB_ID, 0, Duration::from_secs(1)),
                Verdict::Settled,
            ),
            (
                wait::job_killed(JOB_ID, Duration::from_secs(900)),
                Verdict::Settled,
            ),
            (
                wait::task_run_completed(Uuid::new_v4(), Duration::from_secs(1)),
                Verdict::Settled,
            ),
            (
                wait::task_run_failed(Uuid::new_v4(), Duration::from_secs(1), "the runner died"),
                Verdict::Settled,
            ),
            (
                wait::checks_settled("main", "abc1234", "success", Duration::from_secs(1)),
                Verdict::Settled,
            ),
            (silent.clone(), Verdict::Silent),
            (unreadable.clone(), Verdict::Unreadable),
            (ran_out.clone(), Verdict::TimedOut),
        ] {
            assert_eq!(outcome.verdict, verdict, "{}", outcome.text);
        }

        // Said a second way, because the table above and the builders it reads
        // can be changed together: whatever the spelling, none of the three
        // that report nothing having passed may arrive as the verdict the card
        // draws neutrally.
        for outcome in [silent, unreadable, ran_out] {
            assert_ne!(
                outcome.verdict,
                Verdict::Settled,
                "an outcome that says it is not a pass is never drawn as one: {}",
                outcome.text
            );
        }
    }

    fn cancellation() -> (broadcast::Sender<()>, broadcast::Receiver<()>) {
        broadcast::channel(1)
    }

    fn unreachable_deadline() -> tokio::time::Instant {
        tokio::time::Instant::now() + Duration::from_secs(3600)
    }

    #[tokio::test]
    async fn a_cancel_during_a_wait_stops_it() {
        let (sender, mut cancel) = cancellation();
        sender.send(()).unwrap();

        let end = settle_wait(
            std::future::pending(),
            std::future::pending(),
            &mut cancel,
            unreachable_deadline(),
        )
        .await;
        assert_eq!(
            end,
            WaitEnd::Cancelled,
            "a reconnected client's stop is the only escape from an orphaned wait"
        );
    }

    #[tokio::test]
    async fn a_wait_the_turn_has_no_time_left_for_ends_the_turn() {
        let (_sender, mut cancel) = cancellation();

        let end = settle_wait(
            std::future::pending(),
            std::future::pending(),
            &mut cancel,
            tokio::time::Instant::now(),
        )
        .await;
        assert_eq!(
            end,
            WaitEnd::Expired,
            "a wait clamped by the turn's deadline times out rather than hanging"
        );
    }

    #[tokio::test]
    async fn a_lost_lease_during_a_wait_outranks_a_cancel() {
        let (sender, mut cancel) = cancellation();
        sender.send(()).unwrap();

        let end = settle_wait(
            std::future::pending(),
            std::future::ready(()),
            &mut cancel,
            tokio::time::Instant::now(),
        )
        .await;
        assert_eq!(end, WaitEnd::Lost);
    }

    #[tokio::test]
    async fn an_outcome_landing_with_everything_else_is_still_an_outcome() {
        let (sender, mut cancel) = cancellation();
        sender.send(()).unwrap();
        let outcome = wait::job_exited(JOB_ID, 0, Duration::from_secs(214));

        let end = settle_wait(
            std::future::ready(outcome.clone()),
            std::future::ready(()),
            &mut cancel,
            tokio::time::Instant::now(),
        )
        .await;
        assert_eq!(
            end,
            WaitEnd::Settled(outcome),
            "a settle in the same tick as the deadline is not a timeout"
        );
    }

    #[test]
    fn the_outcome_the_resumed_turn_reads_is_the_one_that_was_stored() {
        let mut replay = session::RunContext::from_messages(vec![LlmMessage::user("build it")]);
        let carried = replay.entries.len();
        let mut waits = Vec::new();
        let outcome = wait::job_exited(JOB_ID, 0, Duration::from_secs(214)).text;

        let stored = wait::resume_with_outcome(
            &mut replay,
            &Waited::new("call_2", waiting()),
            outcome.clone(),
            &mut waits,
        );

        let added = &replay.entries[carried..];
        assert_eq!(
            added.iter().map(|entry| &entry.id).collect::<Vec<_>>(),
            stored.iter().map(|entry| &entry.id).collect::<Vec<_>>(),
            "what the store is handed and what the resumed run reads are one pair"
        );
        assert!(
            added
                .iter()
                .any(|entry| entry.message.content.as_deref() == Some(outcome.as_str())),
            "the resumed run reads the outcome, not just the receipt: {added:?}"
        );
    }

    async fn background(session: ToolSession, cwd: &std::path::Path, script: &str) -> JobStarted {
        let context = abnegate_agent::ToolContext::default()
            .within(cwd)
            .with_session(session)
            .with_application(zone_core::application::application())
            .with_environment(abnegate_agent::tool::EnvironmentPolicy::empty())
            .with_proxy(abnegate_agent::tool::Proxy::default());
        Jobs::spawn(
            &abnegate_agent::tool::job::JobCommand::shell(script),
            &context,
        )
        .await
        .unwrap()
    }

    async fn settles(session: ToolSession, id: &str) {
        for _ in 0..500 {
            if Jobs::read(session, id, 0, 0)
                .await
                .is_ok_and(|tail| tail.state.settled())
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the job never reached a state the registry calls settled");
    }

    #[tokio::test]
    async fn a_finished_job_reports_its_code_and_one_killed_with_the_turn_does_not() {
        let chat_id = Uuid::new_v4();
        let cwd = tempfile::tempdir().unwrap();
        let session = ToolSession::Chat(chat_id);
        let finished = background(session, cwd.path(), "exit 3").await;
        settles(session, &finished.id).await;

        assert_eq!(
            job_exit(&finished.id, job_state(chat_id, &finished.id).await).exit_code,
            Some(3),
            "a job that finished on its own reports what it exited with"
        );
        assert_eq!(
            Jobs::kill_session(session).await,
            1,
            "the turn's teardown claims the job it started"
        );
        assert!(
            job_exit(&finished.id, job_state(chat_id, &finished.id).await)
                .exit_code
                .is_none(),
            "a job the turn took with it is gone, not exited"
        );
    }

    #[tokio::test]
    async fn a_timed_out_job_wait_leaves_the_job_for_the_teardown_to_report() {
        let chat_id = Uuid::new_v4();
        let cwd = tempfile::tempdir().unwrap();
        let session = ToolSession::Chat(chat_id);
        let running = background(session, cwd.path(), "sleep 30").await;
        let mut tracked = vec![running.id.clone()];

        assert_eq!(
            job_wait_exit(chat_id, &running.id, &mut tracked).await,
            None,
            "a wait that ran out of time has not seen the job end, so nothing is announced"
        );
        assert_eq!(
            tracked,
            std::slice::from_ref(&running.id),
            "the job the wait gave up on is still the turn's to report"
        );

        assert_eq!(
            job_exits(chat_id, &tracked).await,
            [job::JobExit {
                id: running.id.clone(),
                exit_code: None,
            }],
            "the teardown reports the real end, exactly once, for the job still running"
        );
        assert_eq!(
            Jobs::kill_session(session).await,
            1,
            "the job outlived the wait and the turn is what ends it"
        );
    }

    #[tokio::test]
    async fn a_settled_job_wait_reports_the_code_and_takes_the_job_off_the_turn() {
        let chat_id = Uuid::new_v4();
        let cwd = tempfile::tempdir().unwrap();
        let session = ToolSession::Chat(chat_id);
        let finished = background(session, cwd.path(), "exit 5").await;
        settles(session, &finished.id).await;
        let mut tracked = vec![finished.id.clone()];

        assert_eq!(
            job_wait_exit(chat_id, &finished.id, &mut tracked).await,
            Some(job::JobExit {
                id: finished.id.clone(),
                exit_code: Some(5),
            }),
            "a wait the job's own exit settled reports the code it exited with"
        );
        assert!(
            tracked.is_empty(),
            "the wait announced the end, so the teardown must not announce it again"
        );
        assert!(
            job_exits(chat_id, &tracked).await.is_empty(),
            "the teardown has nothing left to report"
        );
        Jobs::kill_session(session).await;
    }

    const SOURCE_URI: &str = "https://example.test/changelog";
    const SOURCE_TITLE: &str = "Example changelog";
    const UNRETRIEVED: &str = "web:abc123";
    const FIRST_OBSERVED: &str = "2026-09-05T00:00:00+00:00";

    fn held(uri: &str, title: &str) -> db::chat_sources::Source {
        held_as(agent::identifier::Kind::Web, uri, title)
    }

    fn held_as(kind: agent::identifier::Kind, key: &str, title: &str) -> db::chat_sources::Source {
        let first_observed_at = chrono::DateTime::parse_from_rfc3339(FIRST_OBSERVED)
            .expect("the fixture observation time is rfc3339")
            .with_timezone(&chrono::Utc);
        db::chat_sources::Source {
            chat_id: Uuid::nil(),
            identifier: agent::identifier::mint(kind, key),
            kind,
            key: key.to_string(),
            uri: key.to_string(),
            title: title.to_string(),
            first_observed_at,
            last_observed_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn an_unknown_marker_yields_no_citation() {
        let reply = format!(
            "The release shipped on Tuesday {}.",
            agent::identifier::render(UNRETRIEVED)
        );

        let identifiers = cited_identifiers(&reply);
        assert_eq!(identifiers, [UNRETRIEVED]);

        let cited = cited_sources(&identifiers, &[]);

        assert!(
            cited.citations.is_empty(),
            "a marker the registry never held became a citation, so a fabricated attribution \
             reads as evidence"
        );
        assert_eq!(cited.resolved, 0);
        assert_eq!(cited.unresolved, [UNRETRIEVED]);
    }

    #[tokio::test]
    async fn an_unresolved_marker_leaves_the_reply_exactly_as_the_model_wrote_it() {
        let state = AppState::for_tests();
        let marker = agent::identifier::render(UNRETRIEVED);
        let reply = format!("The release shipped on Tuesday {marker}.");
        let mut citations = Vec::new();

        merge_cited_sources(&state, Uuid::new_v4(), &reply, &mut citations).await;

        assert!(
            citations.is_empty(),
            "a marker the registry never held became a citation: {citations:?}"
        );
        assert_eq!(
            reply,
            format!("The release shipped on Tuesday {marker}."),
            "the reply was rewritten; an unresolved marker must stay visible so the reader \
             can see there is nothing behind the claim"
        );
        assert_eq!(
            cited_identifiers(&reply),
            [UNRETRIEVED],
            "the unresolved marker no longer scans out of the reply, so stripping it left a \
             confident sentence with nothing to check"
        );
    }

    #[test]
    fn a_stored_identifier_resolves_by_its_row_and_never_by_rehashing_its_uri() {
        let mut source = held(SOURCE_URI, SOURCE_TITLE);
        source.identifier =
            agent::identifier::mint(agent::identifier::Kind::Web, "https://example.test/other");
        assert_ne!(
            source.identifier,
            agent::identifier::mint(agent::identifier::Kind::Web, SOURCE_URI),
            "the fixture identifier hashes to its own uri, so this test cannot tell a lookup \
             from a recompute"
        );

        let reply = format!(
            "The notes say so {}.",
            agent::identifier::render(&source.identifier)
        );
        let identifiers = cited_identifiers(&reply);

        let cited = cited_sources(&identifiers, std::slice::from_ref(&source));

        assert_eq!(
            cited.resolved, 1,
            "an identifier the registry holds stopped resolving once it no longer matched a \
             fresh digest of its uri, so resolution is recomputing instead of looking up"
        );
        assert!(cited.unresolved.is_empty());
        assert_eq!(cited.citations.len(), 1, "{:?}", cited.citations);
        assert_eq!(cited.citations[0].url, SOURCE_URI);
        assert_eq!(
            cited.citations[0].identifier.as_deref(),
            Some(&*source.identifier)
        );
    }

    #[test]
    fn a_source_cited_twice_in_one_reply_produces_one_citation() {
        let source = held(SOURCE_URI, SOURCE_TITLE);
        let marker = agent::identifier::render(&source.identifier);
        let reply = format!("It shipped {marker}, and the notes agree {marker}.");

        let identifiers = cited_identifiers(&reply);
        assert_eq!(
            identifiers,
            [source.identifier.as_str()],
            "one source cited twice scanned as two, so the same page is about to be cited twice"
        );

        let cited = cited_sources(&identifiers, std::slice::from_ref(&source));
        let mut citations = Vec::new();
        agent::citations::merge(&mut citations, cited.citations);

        assert_eq!(citations.len(), 1, "{citations:?}");
        assert_eq!(
            citations[0].identifier.as_deref(),
            Some(&*source.identifier)
        );
        assert_eq!(citations[0].url, SOURCE_URI);
        assert_eq!(citations[0].title, SOURCE_TITLE);
        assert_eq!(citations[0].kind, agent::CitationKind::Web);
        assert_eq!(
            citations[0].observed_at,
            source.first_observed_at.to_rfc3339(),
            "a cited source must carry when it was first observed, not when it was cited"
        );
        assert!(
            !citations[0].passing(),
            "a retrieved page is something the server saw, not something it verified"
        );
        assert!(cited.unresolved.is_empty());
        assert_eq!(cited.resolved, 1);
    }

    #[test]
    fn a_reply_that_cites_nothing_resolves_nothing() {
        assert!(cited_identifiers("No markers here, and [web:zz] is not one.").is_empty());
    }

    #[test]
    fn every_registry_kind_a_reply_can_cite_renders_as_a_citation() {
        for (kind, expected) in [
            (agent::identifier::Kind::Web, agent::CitationKind::Web),
            (
                agent::identifier::Kind::Doc,
                agent::CitationKind::WorkspaceDocument,
            ),
            (
                agent::identifier::Kind::Kb,
                agent::CitationKind::KnowledgePassage,
            ),
        ] {
            assert_eq!(
                citation_kind(kind),
                Some(expected),
                "a cited {kind} source must reach the console, or its marker reads as fabricated"
            );
        }
    }

    #[test]
    fn a_registry_kind_the_citation_shape_cannot_express_is_not_guessed_at() {
        assert_eq!(citation_kind(agent::identifier::Kind::Chat), None);
    }

    #[test]
    fn a_cited_document_and_passage_reach_the_console_carrying_their_identifiers() {
        let document = held_as(
            agent::identifier::Kind::Doc,
            "workspace://documents/release-notes",
            "Release notes",
        );
        let passage = held_as(
            agent::identifier::Kind::Kb,
            "knowledge:7f2b",
            "Deployment runbook",
        );
        let identifiers = vec![document.identifier.clone(), passage.identifier.clone()];

        let cited = cited_sources(&identifiers, &[document.clone(), passage.clone()]);

        assert_eq!(cited.resolved, 2);
        assert!(cited.unresolved.is_empty());
        assert_eq!(
            cited
                .citations
                .iter()
                .map(|citation| (citation.kind.clone(), citation.identifier.clone()))
                .collect::<Vec<_>>(),
            vec![
                (
                    agent::CitationKind::WorkspaceDocument,
                    Some(document.identifier)
                ),
                (
                    agent::CitationKind::KnowledgePassage,
                    Some(passage.identifier)
                ),
            ],
            "a document or passage the registry resolved produced no citation, so the console \
             renders the reply's own marker as a fabrication"
        );
    }

    /// Zone's tools reach a spawned coding agent over MCP (see `crate::mcp`),
    /// and which of the agent's own tools survive beside them is
    /// `agent_sandboxed` alone.
    #[test]
    fn only_a_sandboxed_chat_withholds_the_agents_own_tools() {
        assert_eq!(
            builtin_tools(true),
            BuiltinTools::Withheld,
            "a sandboxed chat must not leave the agent tools zone cannot gate"
        );
        assert_eq!(
            builtin_tools(false),
            BuiltinTools::Granted,
            "an unsandboxed chat keeps the agent's own file and shell tools"
        );
    }
}
