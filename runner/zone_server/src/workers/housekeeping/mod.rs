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
//!   [`abnegate_notify::Fanout`] maps a panicking channel back to its notifier.
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
//! Every sweep has a [`Scope`]. Replica sweeps run on every replica because the
//! state they keep is the replica's own. Cluster sweeps run only on the one
//! replica whose [`Leadership`] holds [`leader::LEADER_KEY`], checked at each
//! dispatch and aborted on demotion.
//!
//! A pass aborted mid-way can be repeated by the next leader, never alongside
//! it: the old leader stops within `freshness` of its last answered ping, and a
//! new one waits longer than that before its first sweep. The two sweeps that
//! notify claim in Postgres before they send, scheduled reports in
//! `report_deliveries` and regression alerts in `regression_alerts`, so a repeat
//! sends nothing twice. Every other cluster sweep tolerates a duplicate pass:
//! each recomputes what is due from stored state on every pass, so a repeat is
//! the same as one replica sweeping again early, which a restart already does.

pub mod catchup;
pub mod jitter;
pub mod job;
pub mod leader;
pub mod outcome;
pub mod periodic;
pub mod registry;
pub mod report;
pub mod schedule;
pub mod scope;
pub mod slot;
pub mod sweep;
pub mod warmup;
pub mod worker;

pub use catchup::Catchup;
pub use jitter::Jitter;
pub use job::{Failure, Job, Sweeping};
pub use leader::{LEADER_KEY, Leadership, Tenure, Timing, UnsafeTiming};
pub use outcome::Outcome;
pub use periodic::{Periodic, periodic};
pub use registry::Registry;
pub use report::Report;
pub use schedule::Schedule;
pub use scope::Scope;
pub use slot::Slot;
pub use sweep::Sweep;
pub use warmup::Warmup;
pub use worker::{Worker, spawn};
