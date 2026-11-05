//! LLM client module
//!
//! Completions come from an OpenAI-compatible endpoint through
//! [`abnegate_llm`], or from a coding agent CLI on this host, behind one
//! [`LlmClient`].

mod client;
pub mod provider;
pub mod window;

pub use client::{ChatStream, LlmBackend, LlmClient, READ_TIMEOUT, TIMEOUT};
pub use provider::{
    AgentEvent, AgentKind, AgentStream, BuiltinTools, CliProvider, CliSettings, CodexSandbox,
    Observer, Output, Session, Toolset,
};
