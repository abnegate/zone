//! LLM client module
//!
//! Provides an OpenAI-compatible client for chat completions with tool use,
//! and a provider abstraction that puts a coding agent CLI behind the same
//! handle.

mod client;
mod dialect;
pub mod finish_reason;
pub(crate) mod history;
pub mod metadata;
pub mod provider;
mod reasoning;
mod types;

pub use client::{ChatStream, LlmBackend, LlmClient, LlmConfig, LlmError, RequestOptions, Trust};
pub use dialect::{Budget, Dialect, TEMPLATE_STOPS};
pub use provider::{
    AgentEvent, AgentKind, AgentStream, BuiltinTools, CliProvider, CliSettings, CodexSandbox,
    Completion, CompletionProvider, CompletionRequest, Credential, HttpProvider, LIMIT_WORDINGS,
    Limit, ProviderError, ProviderKind, Router, SelectionStrategy, Session, Toolset, Weighted,
    Window,
};
pub use reasoning::{Effort, ReasoningEffort, classify};
pub use types::*;
