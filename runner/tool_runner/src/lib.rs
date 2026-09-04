//! Tool Runner Core Library
//!
//! This library provides the core functionality for the Zone tool runner,
//! which executes commands in workspaces with streaming output, timeouts,
//! and cancellation support.
//!
//! # Architecture
//!
//! The library is organized into several modules:
//!
//! - [`protocol`]: Message types and NDJSON codec for communication
//! - [`executor`]: Command execution engine with streaming output
//! - [`job`]: Job state management and registry
//! - [`error`]: Error types for the library
//!
//! # Usage
//!
//! The typical usage pattern is:
//!
//! 1. Create a [`CommandExecutor`] with desired configuration
//! 2. Register jobs with a [`JobRegistry`]
//! 3. Spawn commands using [`CommandExecutor::spawn`]
//! 4. Stream output through channels
//! 5. Handle cancellation and timeouts
//!
//! # Protocol
//!
//! Communication uses newline-delimited JSON (NDJSON) over stdin/stdout.
//! See the [`protocol`] module for message types.
//!
//! [`CommandExecutor`]: executor::CommandExecutor
//! [`JobRegistry`]: job::JobRegistry

pub mod error;
pub mod executor;
pub mod job;
pub mod protocol;
pub mod proxy;

// Re-export commonly used types
pub use error::{DaemonError, ExecutorError, JobError, ProtocolError};
pub use executor::{CommandExecutor, ExecutorConfig, JobHandle};
pub use job::{JobRegistry, JobState};
pub use protocol::{
    Capability, ErrorCode, InboundMessage, LogLevel, NdjsonCodec, OutboundMessage, PROTOCOL_VERSION,
};
pub use proxy::Proxy;
