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

pub use chat::handle_chat_ws;
pub use context::handle_context_ws;
pub use pull::handle_pull_ws;
pub use task_run::{ProgressMessage, TaskProgressBroadcaster, handle_task_ws};
