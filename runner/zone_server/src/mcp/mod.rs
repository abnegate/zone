//! Serving zone's own tools to a spawned coding agent over MCP.
//!
//! A coding agent driven as a completion provider runs its own tool loop and
//! cannot be handed zone's schemas over a completions API, so it reaches them
//! the way it reaches any other MCP server: over this endpoint. Nothing about
//! the tools changes by arriving here. They are the chat turn's own registry,
//! and a call runs under the chat's own [`crate::agent::ApprovalPolicy`], so a
//! call the reader would have been asked about in chat is put to them here
//! too, on the same card, answered through the same gate.
//!
//! What the agent still holds of its own -- its file tools, its shell -- is a
//! different question, decided by [`zone_core::llm::BuiltinTools`] when the
//! child is spawned. Those tools never reach this endpoint and zone never sees
//! them.

mod endpoint;
mod protocol;
mod turn;

#[cfg(test)]
pub(crate) mod testing;

pub use endpoint::{BODY_LIMIT, PATH, endpoint, serve};
pub use turn::{Lease, Turn};
