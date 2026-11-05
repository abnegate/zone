//! Seeing what a turn's own tools did without carrying it to the reader.

use std::fmt::Debug;

use super::output::Output;

/// Told of each of an agent's own tool calls once it has finished. What a tool
/// prints never reaches the reader, so a caller that has to act on it is
/// handed it here.
pub trait Observer: Debug + Send + Sync {
    fn observe(&self, output: &Output);
}
