//! The one worker that owns how often the server does background work.
//!
//! Every periodic sweep used to bring its own loop. Nine `tokio::spawn` calls in
//! `main.rs` started eight `tokio::time::interval`s and two `sleep` loops, each
//! with a period written as a bare `Duration::from_secs` next to the work, each
//! having picked a missed-tick behaviour by copying whichever worker was
//! written before it, none of them jittered. Nothing said how often zone did
//! background work; you had to read all nine.
//!
//! [`periodic`] is now that answer, and this worker runs it:
//!
//! - **A registry.** [`Periodic`] names every sweep and states its period, its
//!   warm-up and its catch-up as data. Nothing is inherited.
//! - **Jitter.** [`Jitter`] slides each sweep's *first* turn by up to a tenth of
//!   its period, so a fleet started by one orchestrator in one second does not
//!   scan every workspace in lockstep forever after. Only the phase moves; the
//!   period between turns stays exact.
//! - **Overlap prevention.** A job has one pending turn, [`Slot::take`] removes
//!   it, and only settling the finished sweep puts one back. A sweep that runs
//!   ten times its period cannot be started alongside itself.
//! - **Error isolation.** Every sweep is its own [`tokio::task`] in a
//!   [`tokio::task::JoinSet`]. A failure is a `Result` the loop logs; a panic is
//!   a `JoinError` mapped back to its job through the task id, the way
//!   [`zone_notify::Fanout`] maps a panicking channel back to its notifier.
//!   Neither reaches the loop's own stack, and neither costs a sibling its turn.
//!
//! # What did not move
//!
//! [`crate::workers::reminders`] ticks every ten seconds and drains a queue
//! until it is empty. That is dispatch, not a periodic sweep: the timer is a
//! floor on how often it looks for work rather than a cadence for doing it, it
//! already coordinates across instances through database locks, and folding it
//! in would put a ten-second job in a registry whose next fastest sweep is five
//! minutes. It kept its own loop.
//!
//! The reactive workers — task execution, pull requests, embeddings, gathering,
//! indexing, titles, conflict resolution, evaluation and notification — are
//! driven by work arriving, not by a clock, and have nothing to register.
//!
//! # Multi-instance safety
//!
//! Several of these sweeps are still not safe to run on two instances at once,
//! and this worker does not change that. Every instance runs every registered
//! sweep, exactly as it did before. Jitter separates them in time, which reduces
//! collisions but does not prevent them; the protections that exist are the
//! ones the work already carried, such as the `pg_advisory_xact_lock` in
//! [`crate::db::knowledge`].
//!
//! Fixing it properly means per-job leader election on a session-scoped
//! advisory lock over a pinned connection, in the shape
//! `zone_installer::migration::lock` already uses. That is a behaviour change
//! rather than a move, it needs a connection held for the length of a sweep,
//! and it cannot be tested here, so it is deliberately out of scope for this
//! change. The seam for it is [`Sweep`]: a sweep's work is a closure, so
//! wrapping one in a lock is a change to [`periodic`] alone.

pub mod catchup;
pub mod jitter;
pub mod job;
pub mod outcome;
pub mod periodic;
pub mod registry;
pub mod report;
pub mod schedule;
pub mod slot;
pub mod sweep;
pub mod warmup;
pub mod worker;

pub use catchup::Catchup;
pub use jitter::Jitter;
pub use job::{Failure, Job, Sweeping};
pub use outcome::Outcome;
pub use periodic::{Periodic, periodic};
pub use registry::Registry;
pub use report::Report;
pub use schedule::Schedule;
pub use slot::Slot;
pub use sweep::Sweep;
pub use warmup::Warmup;
pub use worker::{Worker, spawn};
