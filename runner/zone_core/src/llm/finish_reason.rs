//! The OpenAI-compatible words a completion gives for why it ended.

/// A turn that ended normally.
///
/// Claude reports `success` and codex `completed`. The agent loop takes a
/// reply as final only on `stop`, so a turn carrying the agent's own word
/// would never be accepted and would run to the iteration limit instead.
pub const STOP: &str = "stop";

/// A reply cut off by its token limit.
pub const LENGTH: &str = "length";
