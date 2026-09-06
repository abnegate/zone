//! LLM client module
//!
//! Provides an OpenAI-compatible client for chat completions with tool use.

mod client;
pub(crate) mod history;
mod reasoning;
mod types;

pub use client::{LlmClient, LlmConfig, LlmError, RequestOptions};
pub use reasoning::{Effort, ReasoningEffort, classify};
pub use types::*;
