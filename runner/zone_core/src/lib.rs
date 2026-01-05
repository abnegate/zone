//! Zone Core - Shared agent logic for Zone
//!
//! This crate provides the core functionality shared between the Zone server
//! and CLI, including:
//! - Domain types (User, Organization, Project, Task, Chat, Source)
//! - Agent loop (ReAct pattern)
//! - LLM client (OpenAI-compatible API)
//! - Tool registry and implementations
//! - File source adapters (local, GitHub)
//! - Session management

pub mod agent;
pub mod error;
pub mod llm;
pub mod session;
pub mod tools;
pub mod types;

// Re-export commonly used types
pub use agent::{
    Agent, AgentCallback, AgentConfig, AgentError, AgentPhase, AgentState, NoOpCallback,
};
pub use error::CoreError;
pub use llm::{LlmClient, LlmConfig, LlmError};
pub use session::{FileSessionStore, Session, SessionStore, SessionSummary};
pub use tools::{Tool, ToolContext, ToolError, ToolRegistry, ToolResult};
pub use types::*;
