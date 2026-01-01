//! Command execution engine.
//!
//! This module provides the core functionality for spawning and managing
//! command execution with streaming output, timeouts, and cancellation.

mod command;
mod limits;
mod process_group;

pub use command::{CommandExecutor, JobHandle, OutputKind, StdinHandle};
pub use limits::{ExecutorConfig, OutputLimiter, DEFAULT_BUFFER_SIZE, DEFAULT_MAX_OUTPUT_BYTES, DEFAULT_TIMEOUT_MS, GRACE_PERIOD};
pub use process_group::ProcessGroup;
