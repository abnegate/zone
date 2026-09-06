//! Canonical, token-aware conversation projections. Compaction never edits history.
mod compact;
mod estimate;
mod types;

pub use compact::{coverage, prepare, project, validate};
pub use estimate::estimate;
pub use types::*;
