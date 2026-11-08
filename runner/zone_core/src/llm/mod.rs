//! LLM client module
//!
//! Completions come from an OpenAI-compatible endpoint through
//! [`abnegate_llm`], or from a coding agent CLI on this host through
//! [`abnegate_agent_cli`], behind one [`LlmClient`].

mod backend;
pub mod cli;
mod client;
pub mod window;

pub use backend::LlmBackend;
pub use cli::{BuiltinTools, Observer, SignIn, Toolset};
pub use client::{ChatStream, LlmClient, READ_TIMEOUT, TIMEOUT};
