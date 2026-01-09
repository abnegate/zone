//! WebSocket handler for context gathering progress
//!
//! Streams real-time progress updates for context gathering operations.
//!
//! Protocol:
//! 1. Client connects to /ws/context/:gathering_id
//! 2. Client sends JWT token as first message for authentication
//! 3. Server streams GatheringEvent updates as JSON messages
//! 4. Connection closes when gathering completes or on error

use axum::{
    body::Bytes,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use chrono::NaiveDateTime;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::auth::validate_token;
use crate::db::{context_gatherings, gathering_events, workspace_members};
use crate::state::AppState;

/// WebSocket polling interval in milliseconds
const WS_POLL_INTERVAL_MS: u64 = 200;

/// Authentication timeout in seconds
const WS_AUTH_TIMEOUT_SECS: u64 = 30;

/// Re-check authorization every ~10 seconds (50 poll cycles at 200ms)
const AUTH_RECHECK_INTERVAL: u32 = 50;

/// Maximum number of events to fetch per poll
const WS_EVENT_BATCH_SIZE: i64 = 100;

/// WebSocket idle timeout in seconds (5 minutes)
const WS_IDLE_TIMEOUT_SECS: u64 = 300;

/// WebSocket ping interval in seconds
const WS_PING_INTERVAL_SECS: u64 = 30;

/// Maximum consecutive database errors before closing connection
const MAX_CONSECUTIVE_ERRORS: u32 = 5;

/// Maximum concurrent connections per gathering
const MAX_CONNECTIONS_PER_GATHERING: usize = 10;

/// Gathering status constants
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";
const STATUS_CONNECTED: &str = "connected";

/// Global connection limiter per gathering
static GATHERING_CONNECTIONS: Lazy<DashMap<Uuid, Arc<Semaphore>>> = Lazy::new(DashMap::new);

/// Client message for authentication
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Authenticate with JWT
    Auth { token: String },
}

/// Server event message sent to clients
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// Initial connection status
    Init { gathering_id: Uuid, status: String },
    /// Gathering event
    Event {
        event_type: String,
        payload: serde_json::Value,
        created_at: String,
    },
    /// Terminal status (completed or failed)
    Terminal { status: String, gathering_id: Uuid },
    /// Error message
    Error { message: String },
}

/// WebSocket upgrade handler for context gathering progress
pub async fn handle_context_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(gathering_id): Path<Uuid>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, gathering_id))
}

/// Handle the WebSocket connection
async fn handle_socket(socket: WebSocket, state: AppState, gathering_id: Uuid) {
    let (mut sender, mut receiver) = socket.split();

    // CRITICAL-3: Rate limiting - enforce max connections per gathering
    let semaphore = GATHERING_CONNECTIONS
        .entry(gathering_id)
        .or_insert_with(|| Arc::new(Semaphore::new(MAX_CONNECTIONS_PER_GATHERING)))
        .clone();

    let _permit = match semaphore.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            tracing::warn!(
                "Too many connections for gathering {}, rejecting",
                gathering_id
            );
            let error_msg = ServerMessage::Error {
                message: "Too many connections".to_string(),
            };
            let _ = sender
                .send(Message::Text(
                    serde_json::to_string(&error_msg).unwrap().into(),
                ))
                .await;
            let _ = sender.close().await;
            return;
        }
    };

    // Wait for auth message
    let claims = match tokio::time::timeout(
        std::time::Duration::from_secs(WS_AUTH_TIMEOUT_SECS),
        receiver.next(),
    )
    .await
    {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<ClientMessage>(&text) {
            Ok(ClientMessage::Auth { token }) => {
                match validate_token(&token, state.config().jwt_secret()) {
                    Ok(claims) => claims,
                    Err(e) => {
                        // CRITICAL-1: Don't leak JWT configuration details to client
                        tracing::warn!(
                            "Authentication failed for gathering {}: {}",
                            gathering_id,
                            e
                        );
                        let error_msg = ServerMessage::Error {
                            message: "Authentication failed".to_string(),
                        };
                        let _ = sender
                            .send(Message::Text(
                                serde_json::to_string(&error_msg).unwrap().into(),
                            ))
                            .await;
                        let _ = sender.close().await;
                        return;
                    }
                }
            }
            Err(_) => {
                let error_msg = ServerMessage::Error {
                    message: "Invalid message format".to_string(),
                };
                let _ = sender
                    .send(Message::Text(
                        serde_json::to_string(&error_msg).unwrap().into(),
                    ))
                    .await;
                let _ = sender.close().await;
                return;
            }
        },
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) => return,
        _ => {
            let error_msg = ServerMessage::Error {
                message: "Authentication timeout or error".to_string(),
            };
            let _ = sender
                .send(Message::Text(
                    serde_json::to_string(&error_msg).unwrap().into(),
                ))
                .await;
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
            let _ = sender
                .send(Message::Text(
                    serde_json::to_string(&error_msg).unwrap().into(),
                ))
                .await;
            let _ = sender.close().await;
            return;
        }
    };

    // Verify gathering ownership
    let gathering = match context_gatherings::get_gathering(state.db(), gathering_id).await {
        Ok(Some(g)) => g,
        Ok(None) => {
            let error_msg = ServerMessage::Error {
                message: "Gathering not found".to_string(),
            };
            let _ = sender
                .send(Message::Text(
                    serde_json::to_string(&error_msg).unwrap().into(),
                ))
                .await;
            let _ = sender.close().await;
            return;
        }
        Err(e) => {
            tracing::error!("Database error fetching gathering: {}", e);
            let error_msg = ServerMessage::Error {
                message: "Internal server error".to_string(),
            };
            let _ = sender
                .send(Message::Text(
                    serde_json::to_string(&error_msg).unwrap().into(),
                ))
                .await;
            let _ = sender.close().await;
            return;
        }
    };

    // Extract workspace_id and verify user has access
    let workspace_id = match gathering.workspace_id {
        Some(ws_id) => ws_id,
        None => {
            tracing::warn!("Gathering {} has no workspace_id", gathering_id);
            let error_msg = ServerMessage::Error {
                message: "Invalid gathering configuration".to_string(),
            };
            let _ = sender
                .send(Message::Text(
                    serde_json::to_string(&error_msg).unwrap().into(),
                ))
                .await;
            let _ = sender.close().await;
            return;
        }
    };

    // Verify user is a member of the workspace
    match workspace_members::is_member(state.db(), user_id, workspace_id).await {
        Ok(true) => {
            // MINOR-6: Log successful connection
            tracing::info!(
                "User {} connected to gathering {} in workspace {}",
                user_id,
                gathering_id,
                workspace_id
            );
        }
        Ok(false) => {
            tracing::warn!(
                "User {} attempted to access gathering {} without permission",
                user_id,
                gathering_id
            );
            let error_msg = ServerMessage::Error {
                message: "Access denied".to_string(),
            };
            let _ = sender
                .send(Message::Text(
                    serde_json::to_string(&error_msg).unwrap().into(),
                ))
                .await;
            let _ = sender.close().await;
            return;
        }
        Err(e) => {
            tracing::error!("Database error checking workspace membership: {}", e);
            let error_msg = ServerMessage::Error {
                message: "Internal server error".to_string(),
            };
            let _ = sender
                .send(Message::Text(
                    serde_json::to_string(&error_msg).unwrap().into(),
                ))
                .await;
            let _ = sender.close().await;
            return;
        }
    }

    // Send initial status
    let init_msg = ServerMessage::Init {
        gathering_id,
        status: STATUS_CONNECTED.to_string(),
    };

    if sender
        .send(Message::Text(
            serde_json::to_string(&init_msg).unwrap().into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    // Track last event time for polling
    let mut last_event_time: Option<NaiveDateTime> = None;
    let mut interval = tokio::time::interval(Duration::from_millis(WS_POLL_INTERVAL_MS));

    // CRITICAL-2: Periodic authorization re-check counter
    let mut auth_check_counter = 0;

    // MAJOR-3: Track consecutive database errors
    let mut consecutive_db_errors = 0;

    // MAJOR-4: Track last client activity and setup ping interval
    let mut last_client_activity = Instant::now();
    let mut ping_interval = tokio::time::interval(Duration::from_secs(WS_PING_INTERVAL_SECS));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                // CRITICAL-2: Periodically re-verify membership
                auth_check_counter += 1;
                if auth_check_counter >= AUTH_RECHECK_INTERVAL {
                    auth_check_counter = 0;
                    match workspace_members::is_member(state.db(), user_id, workspace_id).await {
                        Ok(false) => {
                            tracing::warn!(
                                "User {} lost access to workspace {} during gathering {}",
                                user_id,
                                workspace_id,
                                gathering_id
                            );
                            let error_msg = ServerMessage::Error {
                                message: "Access revoked".to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_msg).unwrap().into(),
                                ))
                                .await;
                            let _ = sender.close().await;
                            return;
                        }
                        Err(e) => {
                            tracing::error!("Error re-checking workspace membership: {}", e);
                            // Don't close on transient errors, but count them
                            consecutive_db_errors += 1;
                            if consecutive_db_errors >= MAX_CONSECUTIVE_ERRORS {
                                let error_msg = ServerMessage::Error {
                                    message: "Connection unstable, please reconnect".to_string(),
                                };
                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&error_msg).unwrap().into(),
                                    ))
                                    .await;
                                let _ = sender.close().await;
                                return;
                            }
                            continue;
                        }
                        Ok(true) => {
                            // Still authorized, continue
                        }
                    }
                }

                // Poll for new events - MINOR-2: use constant for batch size
                match gathering_events::get_events_since(
                    state.db(),
                    gathering_id,
                    last_event_time,
                    Some(WS_EVENT_BATCH_SIZE),
                )
                .await
                {
                    Ok(events) => {
                        // Reset error counter on success
                        consecutive_db_errors = 0;

                        // Stream events to client
                        for event in events {
                            let event_msg = ServerMessage::Event {
                                event_type: event.event_type.clone(),
                                payload: event.payload.clone(),
                                // MINOR-3: Add timezone suffix
                                created_at: format!("{}Z", event.created_at.format("%Y-%m-%dT%H:%M:%S")),
                            };

                            if sender
                                .send(Message::Text(
                                    serde_json::to_string(&event_msg).unwrap().into(),
                                ))
                                .await
                                .is_err()
                            {
                                return;
                            }

                            // Update last event time
                            last_event_time = Some(event.created_at);
                        }

                        // Check if gathering has reached terminal state - MINOR-5: use constants
                        match context_gatherings::get_gathering(state.db(), gathering_id).await {
                            Ok(Some(g)) if g.status == STATUS_COMPLETED || g.status == STATUS_FAILED => {
                                // Send terminal message
                                let terminal_msg = ServerMessage::Terminal {
                                    status: g.status.clone(),
                                    gathering_id,
                                };

                                let _ = sender
                                    .send(Message::Text(
                                        serde_json::to_string(&terminal_msg).unwrap().into(),
                                    ))
                                    .await;

                                let _ = sender.close().await;
                                return;
                            }
                            Err(e) => {
                                tracing::error!("Error checking gathering status: {}", e);
                                // MAJOR-3: Track consecutive errors
                                consecutive_db_errors += 1;
                                if consecutive_db_errors >= MAX_CONSECUTIVE_ERRORS {
                                    let error_msg = ServerMessage::Error {
                                        message: "Connection unstable, please reconnect".to_string(),
                                    };
                                    let _ = sender
                                        .send(Message::Text(
                                            serde_json::to_string(&error_msg).unwrap().into(),
                                        ))
                                        .await;
                                    let _ = sender.close().await;
                                    return;
                                }
                            }
                            _ => {
                                // Gathering still in progress
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error fetching gathering events: {}", e);
                        // MAJOR-3: Track consecutive errors
                        consecutive_db_errors += 1;
                        if consecutive_db_errors >= MAX_CONSECUTIVE_ERRORS {
                            let error_msg = ServerMessage::Error {
                                message: "Connection unstable, please reconnect".to_string(),
                            };
                            let _ = sender
                                .send(Message::Text(
                                    serde_json::to_string(&error_msg).unwrap().into(),
                                ))
                                .await;
                            let _ = sender.close().await;
                            return;
                        }
                    }
                }
            }

            // MAJOR-4: Send periodic pings and check for idle timeout
            _ = ping_interval.tick() => {
                if last_client_activity.elapsed() > Duration::from_secs(WS_IDLE_TIMEOUT_SECS) {
                    tracing::info!(
                        "Closing idle WebSocket connection for gathering {}",
                        gathering_id
                    );
                    let _ = sender.close().await;
                    return;
                }
                if sender.send(Message::Ping(Bytes::new())).await.is_err() {
                    return;
                }
            }

            // Handle client messages (ping/pong, close)
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Ping(data))) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
                            return;
                        }
                        // MAJOR-4: Update activity timestamp
                        last_client_activity = Instant::now();
                    }
                    Some(Ok(Message::Pong(_))) => {
                        // MAJOR-4: Update activity timestamp on pong
                        last_client_activity = Instant::now();
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_deserialize() {
        let json = r#"{"type": "auth", "token": "test-token"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Auth { token } => assert_eq!(token, "test-token"),
        }
    }

    #[test]
    fn test_server_message_serialize() {
        let init_msg = ServerMessage::Init {
            gathering_id: Uuid::new_v4(),
            status: STATUS_CONNECTED.to_string(),
        };
        let json = serde_json::to_string(&init_msg).unwrap();
        assert!(json.contains("\"type\":\"init\""));
        assert!(json.contains("\"status\":\"connected\""));

        let event_msg = ServerMessage::Event {
            event_type: "Progress".to_string(),
            payload: serde_json::json!({"step": 1}),
            created_at: "2024-01-01T00:00:00".to_string(),
        };
        let json = serde_json::to_string(&event_msg).unwrap();
        assert!(json.contains("\"type\":\"event\""));
        assert!(json.contains("\"event_type\":\"Progress\""));

        let terminal_msg = ServerMessage::Terminal {
            status: STATUS_COMPLETED.to_string(),
            gathering_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&terminal_msg).unwrap();
        assert!(json.contains("\"type\":\"terminal\""));
        assert!(json.contains("\"status\":\"completed\""));

        let error_msg = ServerMessage::Error {
            message: "Test error".to_string(),
        };
        let json = serde_json::to_string(&error_msg).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"message\":\"Test error\""));
    }
}
