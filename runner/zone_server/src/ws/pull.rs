//! Authenticated Ollama model downloads with streaming progress.

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::auth::validate_token;
use crate::state::AppState;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

type Sender = SplitSink<WebSocket, Message>;

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Authentication {
    Auth { token: String },
}

#[derive(Deserialize, Serialize)]
struct Pull {
    model: String,
}

#[derive(Deserialize)]
struct Progress {
    status: Option<String>,
    error: Option<String>,
    total: Option<u64>,
    completed: Option<u64>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    Authenticated,
    Step {
        status: String,
    },
    Progress {
        percent: f64,
    },
    Complete {
        success: bool,
        message: &'static str,
    },
    Error {
        message: String,
    },
}

async fn send(sender: &mut Sender, event: Event) -> Result<(), String> {
    let text = serde_json::to_string(&event).map_err(|error| error.to_string())?;
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
        let _ = send(
            &mut sender,
            Event::Error {
                message: "Authentication failed".to_string(),
            },
        )
        .await;
        let _ = sender.close().await;
        return;
    }
    if send(&mut sender, Event::Authenticated).await.is_err() {
        return;
    }

    let request = match tokio::time::timeout(HANDSHAKE_TIMEOUT, receiver.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Pull>(&text).ok(),
        _ => None,
    };
    let Some(request) = request.filter(|request| !request.model.trim().is_empty()) else {
        let _ = send(
            &mut sender,
            Event::Error {
                message: "A model name is required".to_string(),
            },
        )
        .await;
        let _ = sender.close().await;
        return;
    };

    let result = {
        let download = download(&mut sender, &state.config().ollama_host, request);
        tokio::pin!(download);
        loop {
            tokio::select! {
                result = &mut download => break result,
                message = receiver.next() => {
                    match message {
                        Some(Ok(Message::Ping(_) | Message::Pong(_))) => {},
                        // Dropping the download future cancels the upstream request too.
                        _ => return,
                    }
                }
            }
        }
    };
    if let Err(message) = result {
        let _ = send(&mut sender, Event::Error { message }).await;
    }
    let _ = sender.close().await;
}

async fn download(sender: &mut Sender, host: &str, request: Pull) -> Result<(), String> {
    // Large models can take hours; only connection establishment has a timeout.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("Could not connect to Ollama: {error}"))?;
    let response = client
        .post(format!("{}/api/pull", host.trim_end_matches('/')))
        .json(&serde_json::json!({ "model": request.model, "stream": true }))
        .send()
        .await
        .map_err(|error| format!("Could not connect to Ollama: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .map_err(|error| format!("Could not read Ollama error: {error}"))?;
        let message = serde_json::from_str::<Progress>(&body)
            .ok()
            .and_then(|progress| progress.error)
            .filter(|error| !error.trim().is_empty())
            .unwrap_or_else(|| {
                if body.trim().is_empty() {
                    format!("Ollama returned HTTP {status}")
                } else {
                    body
                }
            });
        return Err(message);
    }

    let mut stream = response.bytes_stream();
    let mut pending = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("Model download interrupted: {error}"))?;
        pending.extend_from_slice(&chunk);
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            if process(sender, &line).await? {
                return Ok(());
            }
        }
    }
    if !pending.is_empty() && process(sender, &pending).await? {
        return Ok(());
    }
    Err("Model download ended before installation completed".to_string())
}

async fn process(sender: &mut Sender, line: &[u8]) -> Result<bool, String> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(false);
    }
    let progress: Progress = serde_json::from_slice(line)
        .map_err(|_| "Ollama returned invalid download progress".to_string())?;
    if let Some(error) = progress.error {
        return Err(error);
    }
    if let Some(status) = progress.status {
        if status == "success" {
            send(
                sender,
                Event::Complete {
                    success: true,
                    message: "Model installed successfully",
                },
            )
            .await?;
            return Ok(true);
        }
        send(sender, Event::Step { status }).await?;
    }
    if let (Some(total), Some(completed)) = (progress.total, progress.completed)
        && total > 0
    {
        send(
            sender,
            Event::Progress {
                percent: (completed as f64 / total as f64 * 100.0).clamp(0.0, 100.0),
            },
        )
        .await?;
    }
    Ok(false)
}
