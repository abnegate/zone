//! Task run WebSocket handler
//!
//! Streams real-time progress and logs for task runs.
//!
//! Protocol:
//! 1. Client connects to /ws/tasks/runs/:run_id
//! 2. Client sends JWT token as first message for authentication
//! 3. Server streams progress updates as JSON messages
//! 4. Connection closes when task completes or on error

use axum::{
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::auth::{AccessClaims, validate_access_token};
use crate::db::{self, sessions, tasks, workspace_members};
use crate::state::AppState;

const AUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Progress message sent to clients
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProgressMessage {
    /// Initial task run state
    Init {
        run_id: Uuid,
        task_id: Uuid,
        status: String,
    },
    /// Status changed
    StatusUpdate {
        status: String,
        current_phase: Option<String>,
        progress_percent: Option<i32>,
    },
    /// New log entry
    Log {
        id: Uuid,
        phase: String,
        agent_type: String,
        log_level: String,
        message: String,
    },
    /// Task completed successfully
    Completed { status: String },
    /// Task failed
    Failed { error: String },
    /// Error message
    Error { message: String },
}

impl ProgressMessage {
    /// Convert to a WebSocket text message
    fn to_ws_message(&self) -> Message {
        Message::Text(serde_json::to_string(self).unwrap().into())
    }
}

/// Client message for authentication
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Authenticate with JWT
    Auth { token: String },
}

/// Global task progress broadcaster
///
/// In production, this would be backed by Redis pub/sub for horizontal scaling
pub struct TaskProgressBroadcaster {
    senders: dashmap::DashMap<Uuid, broadcast::Sender<ProgressMessage>>,
}

impl TaskProgressBroadcaster {
    pub fn new() -> Self {
        Self {
            senders: dashmap::DashMap::new(),
        }
    }

    /// Get or create a broadcast channel for a task run
    pub fn get_sender(&self, run_id: Uuid) -> broadcast::Sender<ProgressMessage> {
        self.senders
            .entry(run_id)
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(100);
                tx
            })
            .clone()
    }

    /// Subscribe to a task run's progress
    pub fn subscribe(&self, run_id: Uuid) -> broadcast::Receiver<ProgressMessage> {
        self.get_sender(run_id).subscribe()
    }

    /// Broadcast a message to all subscribers of a task run
    pub fn broadcast(&self, run_id: Uuid, message: ProgressMessage) {
        if let Some(sender) = self.senders.get(&run_id) {
            let _ = sender.send(message);
        }
    }

    /// Remove a broadcast channel when no longer needed
    pub fn remove(&self, run_id: Uuid) {
        self.senders.remove(&run_id);
    }
}

impl Default for TaskProgressBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}

/// WebSocket upgrade handler for task run progress
pub async fn handle_task_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, run_id))
}

/// Whether the bound identity may watch this run.
///
/// Validating the token proves who is calling and nothing about what they may
/// see. Without this step the run id is the only credential, and any account
/// can stream another tenant's agent logs -- which carry command output, file
/// contents and diffs from their repository.
#[derive(Debug, Clone, Copy)]
struct Authorization {
    expires_at: i64,
    session_id: Uuid,
    user_id: Uuid,
    workspace_id: Uuid,
}

impl Authorization {
    async fn is_current(self, state: &AppState) -> db::DbResult<bool> {
        if chrono::Utc::now().timestamp() >= self.expires_at {
            return Ok(false);
        }

        let (session, member) = tokio::try_join!(
            sessions::is_active_user_session(state.db(), self.session_id, self.user_id),
            workspace_members::is_member(state.db(), self.user_id, self.workspace_id),
        )?;

        Ok(session && member)
    }
}

async fn authorize(
    state: &AppState,
    access: &AccessClaims,
    run_id: Uuid,
) -> db::DbResult<Option<Authorization>> {
    let Ok(user_id) = access.claims.user_id() else {
        return Ok(None);
    };
    let Some(session_id) = access.session_id else {
        return Ok(None);
    };

    let Some(run) = tasks::get_task_run(state.db(), run_id).await? else {
        return Ok(None);
    };

    let Some(task) = tasks::get_task(state.db(), run.task_id).await? else {
        return Ok(None);
    };

    let authorization = Authorization {
        expires_at: access.claims.exp,
        session_id,
        user_id,
        workspace_id: task.workspace_id,
    };
    authorization
        .is_current(state)
        .await
        .map(|current| current.then_some(authorization))
}

async fn revalidate(
    sender: &mut SplitSink<WebSocket, Message>,
    state: &AppState,
    authorization: Authorization,
    run_id: Uuid,
) -> bool {
    let message = match authorization.is_current(state).await {
        Ok(true) => return true,
        Ok(false) => "Authorization expired or revoked",
        Err(error) => {
            tracing::error!(%run_id, %error, "Failed to revalidate task stream authorization");
            "Authorization unavailable"
        }
    };
    let error = ProgressMessage::Error {
        message: message.to_string(),
    };
    let _ = sender.send(error.to_ws_message()).await;
    let _ = sender.close().await;
    false
}

/// Handle the WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState, run_id: Uuid) {
    let (mut sender, mut receiver) = socket.split();

    // Wait for auth message
    let authorization = match tokio::time::timeout(AUTH_TIMEOUT, receiver.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::Auth { token }) => {
                match validate_access_token(&token, state.config().jwt_secret()) {
                    Ok(access) => authorize(&state, &access, run_id).await.ok().flatten(),
                    Err(error) => {
                        let msg = ProgressMessage::Error {
                            message: format!("Authentication failed: {}", error),
                        };
                        let _ = sender.send(msg.to_ws_message()).await;
                        None
                    }
                }
            }
            Err(_) => {
                let msg = ProgressMessage::Error {
                    message: "Invalid message format".to_string(),
                };
                let _ = sender.send(msg.to_ws_message()).await;
                None
            }
        },
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => return,
        _ => {
            let msg = ProgressMessage::Error {
                message: "Authentication timeout or error".to_string(),
            };
            let _ = sender.send(msg.to_ws_message()).await;
            None
        }
    };

    let Some(authorization) = authorization else {
        let _ = sender.close().await;
        return;
    };

    // Verify task run exists and get initial state
    let task_run = match tasks::get_task_run(state.db(), run_id).await {
        Ok(Some(run)) => run,
        Ok(None) => {
            let msg = ProgressMessage::Error {
                message: "Task run not found".to_string(),
            };
            let _ = sender.send(msg.to_ws_message()).await;
            return;
        }
        Err(error) => {
            tracing::error!(%run_id, %error, "Failed to load task run");
            let msg = ProgressMessage::Error {
                message: "Task stream unavailable".to_string(),
            };
            let _ = sender.send(msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
    };

    if !revalidate(&mut sender, &state, authorization, run_id).await {
        return;
    }

    // Send initial state
    let init_msg = ProgressMessage::Init {
        run_id: task_run.id,
        task_id: task_run.task_id,
        status: task_run.status.clone(),
    };

    if sender.send(init_msg.to_ws_message()).await.is_err() {
        return;
    }

    let mut last_log_id: Option<Uuid> = None;
    match tasks::get_task_run_logs(state.db(), run_id).await {
        Ok(logs) => {
            if !revalidate(&mut sender, &state, authorization, run_id).await {
                return;
            }

            for log in logs {
                last_log_id = Some(log.id);
                let log_msg = ProgressMessage::Log {
                    id: log.id,
                    phase: log.phase,
                    agent_type: log.agent_type,
                    log_level: log.log_level,
                    message: log.message,
                };
                if sender.send(log_msg.to_ws_message()).await.is_err() {
                    return;
                }
            }
        }
        Err(error) => {
            tracing::error!(%run_id, %error, "Failed to load task run logs");
            let msg = ProgressMessage::Error {
                message: "Task stream unavailable".to_string(),
            };
            let _ = sender.send(msg.to_ws_message()).await;
            let _ = sender.close().await;
            return;
        }
    }

    // If task is already complete, send completion and close
    if task_run.status == "completed" || task_run.status == "failed" {
        let final_msg = if task_run.status == "completed" {
            ProgressMessage::Completed {
                status: task_run.status,
            }
        } else {
            ProgressMessage::Failed {
                error: task_run
                    .error_message
                    .unwrap_or_else(|| "Unknown error".to_string()),
            }
        };
        let _ = sender.send(final_msg.to_ws_message()).await;
        return;
    }

    // For now, we'll poll the database for updates
    // In production, this would use the TaskProgressBroadcaster with Redis pub/sub
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_status = task_run.status;

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // Check for status updates
                match tasks::get_task_run(state.db(), run_id).await {
                    Ok(Some(run)) => {
                        if !revalidate(&mut sender, &state, authorization, run_id).await {
                            return;
                        }

                        // Send status update if changed
                        if run.status != last_status {
                            last_status = run.status.clone();

                            if run.status == "completed" {
                                let msg = ProgressMessage::Completed {
                                    status: run.status,
                                };
                                let _ = sender.send(msg.to_ws_message()).await;
                                return;
                            } else if run.status == "failed" {
                                let msg = ProgressMessage::Failed {
                                    error: run.error_message.unwrap_or_else(|| "Unknown error".to_string()),
                                };
                                let _ = sender.send(msg.to_ws_message()).await;
                                return;
                            } else {
                                let msg = ProgressMessage::StatusUpdate {
                                    status: run.status,
                                    current_phase: run.current_phase,
                                    progress_percent: run.progress_percent,
                                };
                                if sender.send(msg.to_ws_message()).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        let msg = ProgressMessage::Error {
                            message: "Task run not found".to_string(),
                        };
                        let _ = sender.send(msg.to_ws_message()).await;
                        return;
                    }
                    Err(error) => {
                        tracing::error!(%run_id, %error, "Failed to poll task run status");
                        let msg = ProgressMessage::Error {
                            message: "Task stream unavailable".to_string(),
                        };
                        let _ = sender.send(msg.to_ws_message()).await;
                        let _ = sender.close().await;
                        return;
                    },
                }

                // Check for new logs
                match tasks::get_task_run_logs(state.db(), run_id).await {
                    Ok(logs) => {
                        if !revalidate(&mut sender, &state, authorization, run_id).await {
                            return;
                        }

                        for log in logs {
                            // Skip logs we've already sent
                            if let Some(last_id) = last_log_id
                                && log.id <= last_id {
                                    continue;
                                }

                            last_log_id = Some(log.id);

                            let log_msg = ProgressMessage::Log {
                                id: log.id,
                                phase: log.phase,
                                agent_type: log.agent_type,
                                log_level: log.log_level,
                                message: log.message,
                            };
                            if sender.send(log_msg.to_ws_message()).await.is_err() {
                                return;
                            }
                        }
                    },
                    Err(error) => {
                        tracing::error!(%run_id, %error, "Failed to poll task run logs");
                        let msg = ProgressMessage::Error {
                            message: "Task stream unavailable".to_string(),
                        };
                        let _ = sender.send(msg.to_ws_message()).await;
                        let _ = sender.close().await;
                        return;
                    },
                }
            }

            // Handle client messages (ping/pong, close)
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Ping(data))) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
                            return;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    _ => {}
                }
            }
        }
    }
}
