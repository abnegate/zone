//! Authenticated subscriptions to background Ollama model downloads.

use axum::{
    body::Bytes,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::{Instant, MissedTickBehavior, interval_at};

use crate::auth::validate_token;
use crate::pull::{Event, Pull, PullRegistry};
use crate::state::AppState;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const PING_INTERVAL: Duration = Duration::from_secs(15);

type Sender = SplitSink<WebSocket, Message>;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Authentication {
    Auth { token: String },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Handshake {
    Authenticated,
    Error { message: String },
}

async fn emit_handshake(sender: &mut Sender, event: Handshake) -> Result<(), String> {
    let text = serde_json::to_string(&event).map_err(|error| error.to_string())?;
    sender
        .send(Message::Text(text.into()))
        .await
        .map_err(|error| error.to_string())
}

async fn emit_event(sender: &mut Sender, event: &Event) -> Result<(), String> {
    let text = serde_json::to_string(event).map_err(|error| error.to_string())?;
    sender
        .send(Message::Text(text.into()))
        .await
        .map_err(|error| error.to_string())
}

pub async fn handle_pull_ws(
    socket: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    socket.on_upgrade(move |socket| handle(socket, state))
}

async fn handle(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let authenticated = match tokio::time::timeout(HANDSHAKE_TIMEOUT, receiver.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => {
            match serde_json::from_str::<Authentication>(&text) {
                Ok(Authentication::Auth { token }) => {
                    validate_token(&token, state.config().jwt_secret()).is_ok_and(|claims| {
                        // Refresh tokens carry no email and must not authorize a download.
                        !claims.email.is_empty() && claims.user_id().is_ok()
                    })
                }
                Err(_) => false,
            }
        }
        _ => false,
    };
    if !authenticated {
        let _ = emit_handshake(
            &mut sender,
            Handshake::Error {
                message: "Authentication failed".to_string(),
            },
        )
        .await;
        let _ = sender.close().await;
        return;
    }
    if emit_handshake(&mut sender, Handshake::Authenticated)
        .await
        .is_err()
    {
        return;
    }

    let request = match tokio::time::timeout(HANDSHAKE_TIMEOUT, receiver.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Pull>(&text).ok(),
        _ => None,
    };
    let Some(request) = request.filter(|request| !request.model.trim().is_empty()) else {
        let _ = emit_handshake(
            &mut sender,
            Handshake::Error {
                message: "A model name is required".to_string(),
            },
        )
        .await;
        let _ = sender.close().await;
        return;
    };

    let registry = state.pull_registry();
    if request.cancel {
        registry.cancel(&request.model);
        let _ = emit_event(
            &mut sender,
            &Event::Error {
                message: "Installation cancelled".to_string(),
            },
        )
        .await;
        let _ = sender.close().await;
        return;
    }

    subscribe(
        &mut sender,
        &mut receiver,
        registry,
        state.config().ollama_host.clone(),
        request.model,
    )
    .await;
}

async fn subscribe(
    sender: &mut Sender,
    receiver: &mut futures::stream::SplitStream<WebSocket>,
    registry: &PullRegistry,
    host: String,
    model: String,
) {
    let mut subscription = registry.start_or_attach(host, model.clone());
    for event in subscription.replay() {
        if emit_event(sender, &event).await.is_err() {
            return;
        }
        if event.is_terminal() {
            let _ = sender.close().await;
            return;
        }
    }

    let mut ping = interval_at(Instant::now() + PING_INTERVAL, PING_INTERVAL);
    ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            event = subscription.next() => {
                let Some(event) = event else {
                    let _ = sender.close().await;
                    return;
                };
                let terminal = event.is_terminal();
                if emit_event(sender, &event).await.is_err() {
                    return;
                }
                if terminal {
                    let _ = sender.close().await;
                    return;
                }
            }
            _ = ping.tick() => {
                if sender.send(Message::Ping(Bytes::new())).await.is_err() {
                    return;
                }
            }
            message = receiver.next() => {
                match message {
                    Some(Ok(Message::Ping(payload))) => {
                        if sender.send(Message::Pong(payload)).await.is_err() {
                            return;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(pull) = serde_json::from_str::<Pull>(&text)
                            && pull.cancel
                        {
                            registry.cancel(&pull.model);
                        }
                    }
                    Some(Ok(Message::Pong(_) | Message::Binary(_))) => {}
                    // Detaching leaves the background job running.
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                }
            }
        }
    }
}
