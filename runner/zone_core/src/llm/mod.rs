//! LLM client module
//!
//! Provides an OpenAI-compatible client for chat completions with tool use,
//! and a provider abstraction that puts a coding agent CLI behind the same
//! handle.

mod client;
pub(crate) mod history;
pub mod provider;
mod reasoning;
mod types;

pub use client::{LlmClient, LlmConfig, LlmError, RequestOptions};
pub use provider::{
    AgentKind, CliProvider, CliSettings, Completion, CompletionProvider, CompletionRequest,
    Credential, HttpProvider, ProviderError, ProviderKind, Router, SelectionStrategy, Weighted,
};
pub use reasoning::{Effort, ReasoningEffort, classify};
pub use types::*;
