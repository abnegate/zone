//! Item 22: the digest nobody has to go looking for.
//!
//! Every other worker in zone is reactive — something happened, so something
//! runs. Nothing ever says, unprompted, how the week went. This does: one
//! message per workspace per cadence, built from the same numbers
//! [`crate::workers::analytics`] publishes and the same verdicts
//! [`crate::workers::regression`] reaches.
//!
//! Two decisions are worth knowing about.
//!
//! **A missed slot loses nothing.** [`schedule::due`] compares the last delivery
//! to the last slot that passed rather than the clock to an hour, so a process
//! that was down over a slot notices on the way back up. Several missed slots
//! produce one digest covering everything since the last delivery, and it says
//! how many it folded in — a queue of catch-up digests is noise, and trimming
//! the window to keep it tidy would drop the days nobody saw.
//!
//! **A dead channel is not a failed report.** Delivery goes through
//! [`zone_notify::Fanout`], which returns a report rather than a result, so a
//! workspace with a stale Discord webhook still gets its digest on Slack and the
//! slot is recorded rather than retried into a duplicate.
//!
//! [`schedule`] and [`digest`] are pure. [`worker`] is the only part that reads
//! a database, sends anything, or schedules.

pub mod digest;
pub mod schedule;
pub mod worker;

pub use digest::{Digest, generate};
pub use schedule::{Cadence, Due, Schedule, due};
pub use worker::{Ledger, ReportEnvironment, ReportSettings, build, deliver, owed, spawn};
