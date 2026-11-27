//! A client of a running replica's chat WebSocket, reading frames as the
//! console does.

mod received;

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use uuid::Uuid;

use super::replica::Replica;

pub use received::Received;

pub struct ChatSocket {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    closed: Option<CloseFrame>,
}

impl ChatSocket {
    /// Connects to `chat` on `replica` and authenticates as `token`, without
    /// waiting for `init`.
    pub async fn connect(replica: &Replica, chat: Uuid, token: &str) -> Self {
        let (socket, _) = connect_async(replica.chat_socket(chat))
            .await
            .expect("the chat socket opens");
        let mut socket = Self {
            socket,
            closed: None,
        };
        socket.send(json!({"type": "auth", "token": token})).await;
        socket
    }

    /// Connects and reads up to `init`, failing if it takes longer than `within`.
    pub async fn joined(replica: &Replica, chat: Uuid, token: &str, within: Duration) -> Self {
        let mut socket = Self::connect(replica, chat, token).await;
        let init = socket.next(within).await.expect("init arrives");
        assert_eq!(
            init.kind(),
            "init",
            "the first frame is init: {}",
            init.text
        );
        socket
    }

    pub async fn send(&mut self, frame: Value) {
        self.socket
            .send(Message::Text(frame.to_string().into()))
            .await
            .expect("the frame is sent");
    }

    /// Sends a message under `id`, as the console does.
    pub async fn say(&mut self, id: Uuid, content: &str) {
        self.send(json!({"type": "send", "id": id, "content": content}))
            .await;
    }

    /// The next frame, or None when none arrives within `within` or the
    /// socket closed.
    pub async fn next(&mut self, within: Duration) -> Option<Received> {
        let deadline = tokio::time::Instant::now() + within;
        loop {
            let message = tokio::time::timeout_at(deadline, self.socket.next())
                .await
                .ok()??
                .ok()?;
            match message {
                Message::Text(text) => {
                    let text = text.to_string();
                    let frame = serde_json::from_str(&text).expect("frames are JSON");
                    return Some(Received { text, frame });
                }
                Message::Close(frame) => {
                    self.closed = frame;
                    return None;
                }
                _ => {}
            }
        }
    }

    /// Every frame up to and including the first of kind `kind`.
    pub async fn until(&mut self, kind: &str, within: Duration) -> Vec<Received> {
        let deadline = tokio::time::Instant::now() + within;
        let mut received = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Some(frame) = self.next(left).await else {
                panic!(
                    "no {kind} frame within {within:?}; received {:?}",
                    received
                        .iter()
                        .map(|frame: &Received| &frame.text)
                        .collect::<Vec<_>>()
                );
            };
            let done = frame.kind() == kind;
            received.push(frame);
            if done {
                return received;
            }
        }
    }

    /// Every frame that arrives within `within`.
    pub async fn during(&mut self, within: Duration) -> Vec<Received> {
        let deadline = tokio::time::Instant::now() + within;
        let mut received = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                return received;
            }
            match self.next(left).await {
                Some(frame) => received.push(frame),
                None => return received,
            }
        }
    }

    /// The close frame the server sent, once the socket has closed.
    pub fn closed(&self) -> Option<&CloseFrame> {
        self.closed.as_ref()
    }
}
