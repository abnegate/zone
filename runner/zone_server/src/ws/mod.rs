//! WebSocket handlers
//!
//! Provides real-time streaming for:
//! - Task run progress and logs
//! - Model pulling progress

mod task_run;

pub use task_run::handle_task_ws;
