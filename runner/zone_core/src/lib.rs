//! Zone Core - Shared agent logic for Zone
//!
//! This crate provides the core functionality shared between the Zone server
//! and CLI, including:
//! - Domain types (User, Organization, Project, Task, Chat, Source)
//! - LLM client (OpenAI-compatible API)
//! - Zone's tools, MCP servers and local tool context on top of abnegate-agent
//! - The VPN and offline gate on the public web

pub mod application;
pub mod commands;
pub mod error;
pub mod llm;
pub mod local;
pub mod servers;
pub mod types;
#[cfg(any(test, feature = "test-support"))]
pub mod variables;
pub mod vpn;
pub mod walk;

pub use error::CoreError;
pub use llm::LlmClient;
pub use types::*;
