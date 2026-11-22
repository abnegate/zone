//! WebSocket handlers
//!
//! Provides real-time streaming for:
//! - Task run progress and logs
//! - Model pulling progress
//! - Context gathering progress
//! - Chat AI responses

pub mod chat;
pub mod context;
pub mod pull;
pub mod task_run;

use axum::extract::ws::{CloseFrame, Message, close_code};

pub use chat::handle_chat_ws;
pub use context::handle_context_ws;
pub use pull::handle_pull_ws;
pub use task_run::handle_task_ws;

const RESTARTING: &str = "The server is restarting";

/// The close frame a socket gets when the server stops: 1012, Service
/// Restart, which tells its client to reconnect, and reach another replica.
pub fn restarting() -> Message {
    Message::Close(Some(CloseFrame {
        code: close_code::RESTART,
        reason: RESTARTING.into(),
    }))
}
