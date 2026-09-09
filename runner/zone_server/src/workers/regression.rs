//! Item 25: whether a fix that shipped actually held.
//!
//! Zone knows when a task finished and it syncs that outcome outwards, but
//! nothing has ever gone back a week later to ask whether the thing stayed
//! fixed. This module does, on a schedule.
//!
//! # The bar
//!
//! The whole design is about what *not* to report. A regression check earns its
//! place the first time it catches something real and loses it the second time
//! it cries wolf, because after that the notification is closed unread and a
//! genuine regression goes with it. So there are three verdicts, and only
//! [`Verdict::Regressed`] is delivered. [`Verdict::Suspected`] exists for
//! everything that looks wrong without having earned an interruption: it is
//! counted in `zone_agent_regression_checks_total` and never sent.
//!
//! Reaching `Regressed` through [`RecurrenceChecker`] takes all four of:
//!
//! 1. the failure being one the code can cause at all — a rate limit, a reset
//!    connection or a deadline is excluded outright;
//! 2. at least three failures in the same category the fix addressed;
//! 3. those failures spread over at least two occasions more than ten minutes
//!    apart, so one retry storm is one event;
//! 4. and them making up at least half the runs since the fix settled.
//!
//! [`ReopenChecker`] answers from the other direction — a person moved the task
//! back out of completion — so its bar is attribution rather than volume: not
//! within the hour (a status correction), not after the watch window (a new
//! problem on an old task), and back into work rather than back into review.
//!
//! [`checker`], [`recurrence`] and [`reopen`] are pure over in-memory rows.
//! [`worker`] is the only part that reads a database, sends anything, or
//! schedules.

pub mod checker;
pub mod recurrence;
pub mod reopen;
pub mod worker;

pub use checker::{
    Evidence, FixSubject, RegressionChecker, RegressionError, RegressionPolicy, RegressionResult,
    Verdict, is_attributable,
};
pub use recurrence::RecurrenceChecker;
pub use reopen::ReopenChecker;
pub use worker::{Alerted, RegressionSettings, check_all, checkers, scan_workspace, subjects};
