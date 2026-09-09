//! Background workers for async task processing
//!
//! Work reaches a worker one of three ways.
//!
//! **On a schedule.** [`housekeeping`] owns every periodic sweep: knowledge
//! refresh, source resync, answer promotion, learning, agent analytics,
//! regression watching, scheduled digests and reception sync. Each states its
//! period in [`housekeeping::Periodic`] rather than opening its own timer, and
//! this module's other files hold only the work itself.
//!
//! **On a queue.** [`reminders`] drains persisted reminders as they come due,
//! coordinating across server instances through database locks.
//!
//! **On an event.** Task execution, pull request creation and repair, embedding
//! generation, context gathering, source indexing, title generation, conflict
//! resolution, evaluation and notification all run because something arrived,
//! not because a clock struck.

pub mod analytics;
pub mod conflict;
pub mod embeddings;
pub mod evaluation;
pub mod gathering;
pub mod housekeeping;
pub mod indexing;
pub mod knowledge_refresh;
pub mod learning;
pub mod notify;
pub mod pr;
pub mod promotion;
pub mod reception;
pub mod regression;
pub mod reminders;
pub mod reports;
pub mod source_resync;
pub mod task;
pub mod titles;
