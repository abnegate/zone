//! LLM client module
//!
//! Completions come from an OpenAI-compatible endpoint through
//! [`abnegate_llm`], or from a coding agent CLI on this host, behind one
//! [`LlmClient`].

mod client;
pub mod provider;
mod reasoning;
pub mod window;

pub use abnegate_llm::{
    Budget, ChatRequest, ChatResponse, ChatStreamChunk, Choice, ContentPart, Dialect, Effort,
    Error as LlmError, FunctionCall, FunctionDefinition, GeneratedImage, ImageUrl, LlmConfig,
    Message, RequestOptions, Role, SpecificFunction, StreamChoice, StreamDelta,
    StreamFunctionCall, StreamToolCall, TEMPLATE_STOPS, ToolCall, ToolChoice, ToolDefinition,
    Trust, Usage, finish_reason, history, reasoning::classify, wire_arguments,
};
pub use client::{ChatStream, LlmBackend, LlmClient, READ_TIMEOUT, TIMEOUT};
pub use provider::{
    AgentEvent, AgentKind, AgentStream, BuiltinTools, CliProvider, CliSettings, CodexSandbox,
    Completion, CompletionProvider, CompletionRequest, Credential, HttpProvider, LIMIT_WORDINGS,
    Limit, ProviderError, ProviderKind, Router, SelectionStrategy, Session, Toolset, Weighted,
    Window,
};
pub use reasoning::ReasoningEffort;
