//! LLM client module
//!
//! Provides an OpenAI-compatible client for chat completions with tool use.

mod client;
mod types;

pub use client::{LlmClient, LlmConfig, LlmError};
pub use types::*;
