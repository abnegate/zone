//! Seeing what a turn's own tools did without carrying it to the reader.

use std::fmt::Debug;

use super::output::Output;
use super::subagent::Subagent;

/// Told of each of an agent's own tool calls once it has finished. What a tool
/// prints never reaches the reader, so a caller that has to act on it is
/// handed it here.
pub trait Observer: Debug + Send + Sync {
    fn observe(&self, output: &Output);

    /// Told of each update to a subagent the agent started, as it arrives. An
    /// observer with no use for subagents ignores them.
    fn track(&self, _subagent: &Subagent) {}
}
