//! Item 26: what the agent has actually been doing.
//!
//! `metrics.rs` reports on the process — requests served, queries run, cache
//! hits. It says nothing about the work: whether a run succeeds, what it fails
//! on, or how long it takes. This module answers those over a window.
//!
//! The success rate and the failure-kind distribution are not computed here.
//! They come from [`crate::workers::learning::attempt`], which already discounts
//! observations by age and counts distinct tasks. A second, independent tally
//! over the same runs would drift from the first, and a dashboard that disagrees
//! with the lessons drawn from the same evidence is worse than no dashboard.
//! What is added is the shape the learning loop has no use for: completion
//! times, movement across the window, and where the failures sat in it.
//!
//! [`period`] and [`summary`] are pure over in-memory rows. [`worker`] is the
//! only part that reads a database or schedules anything.

pub mod period;
pub mod summary;
pub mod worker;

pub use period::{BucketSize, TimePeriod, TimeWindow, last_weekday_before};
pub use summary::{
    AgentAnalytics, AgentRun, CompletionTimes, SeriesPoint, Trend, TrendDirection, summarize,
};
pub use worker::{AnalyticsPolicy, load_runs, run_cycle};
