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
use uuid::Uuid;

use crate::auth::validate_access_token;
use crate::db::{self, sessions};
use crate::pull::{ComfyPull, Event, Pull, PullRegistry, PullStart};
use crate::state::AppState;
use zone_comfy::recipe::RecipeCatalog;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const PING_INTERVAL: Duration = Duration::from_secs(15);

type Sender = SplitSink<WebSocket, Message>;

#[derive(Clone, Copy)]
struct Authorization {
    expires_at: i64,
    session_id: Uuid,
    user_id: Uuid,
}

impl Authorization {
    async fn is_current(self, state: &AppState) -> db::DbResult<bool> {
        if chrono::Utc::now().timestamp() >= self.expires_at {
            return Ok(false);
        }

        sessions::is_active_user_session(state.db(), self.session_id, self.user_id).await
    }
}

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
    let access = match tokio::time::timeout(HANDSHAKE_TIMEOUT, receiver.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<Authentication>(&text) {
            Ok(Authentication::Auth { token }) => {
                validate_access_token(&token, state.config().jwt_secret()).ok()
            }
            Err(_) => None,
        },
        _ => None,
    };
    let authorization = access.and_then(|access| {
        Some(Authorization {
            expires_at: access.claims.exp,
            session_id: access.session_id?,
            user_id: access.claims.user_id().ok()?,
        })
    });
    let Some(authorization) = authorization else {
        let _ = emit_handshake(
            &mut sender,
            Handshake::Error {
                message: "Authentication failed".to_string(),
            },
        )
        .await;
        let _ = sender.close().await;
        return;
    };
    if !authorization.is_current(&state).await.unwrap_or(false) {
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
        pull_start(&state, request),
        &state,
        authorization,
    )
    .await;
}

fn pull_start(state: &AppState, request: Pull) -> PullStart {
    let comfy = is_comfy_pull(&request).then(|| {
        let catalog = RecipeCatalog::load(Some(state.config().comfyui.workflow_path.as_path()))
            .or_else(|_| RecipeCatalog::packaged())
            .ok();
        let recipe_id = request.recipe_id.clone().or_else(|| {
            request.hf_base.as_deref().and_then(|base| {
                catalog
                    .as_ref()
                    .and_then(|catalog| catalog.adapter_recipe_for_base(base))
                    .map(|recipe| recipe.id.clone())
            })
        });
        ComfyPull {
            models_dir: state.config().comfyui.models_dir.clone(),
            recipe_id,
            hf_base: request.hf_base.clone(),
            hub_origin: crate::routes::models::huggingface_hub_origin(
                &state.config().huggingface_models_url,
            ),
        }
    });
    PullStart {
        model: request.model,
        ollama_host: state.config().ollama_host.clone(),
        comfy,
    }
}

fn is_comfy_pull(request: &Pull) -> bool {
    request
        .runtime
        .as_deref()
        .is_some_and(|runtime| runtime.eq_ignore_ascii_case("comfy"))
        || request.model.contains(".safetensors")
}

async fn subscribe(
    sender: &mut Sender,
    receiver: &mut futures::stream::SplitStream<WebSocket>,
    registry: &PullRegistry,
    request: PullStart,
    state: &AppState,
    authorization: Authorization,
) {
    let mut subscription = registry.start(request);
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
                if !authorization.is_current(state).await.unwrap_or(false) {
                    let _ = emit_event(
                        sender,
                        &Event::Error {
                            message: "Access revoked".to_string(),
                        },
                    )
                    .await;
                    let _ = sender.close().await;
                    return;
                }
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
