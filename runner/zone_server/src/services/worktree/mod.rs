//! Removing the worktrees chat agents leave behind in a workspace's folders.
//!
//! The chat prompt tells an agent to start work in `git worktree add .worktrees/<branch>`, in
//! whichever mounted folder it is working in, and nothing an agent does afterwards removes it.
//! The housekeeping sweep does: a worktree under a folder's `.worktrees/` goes once it is
//! unlocked, has been left alone for a day, holds nothing uncommitted or untracked, and its
//! HEAD has reached the remote's default branch, merged or squashed. Its branch is kept, and a
//! worktree anywhere else is never touched.

mod entry;
mod git;
mod merge;
mod repository;
mod sweep;
mod tally;
mod verdict;

pub use entry::Entry;
pub use repository::{AREA, Repository};
pub use sweep::{IDLE, SWEEP_INTERVAL_SECONDS, clean, sweep};
pub use tally::Tally;
pub use verdict::Verdict;
