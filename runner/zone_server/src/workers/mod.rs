//! Background workers for async task processing
//!
//! This module contains workers that run background tasks such as:
//! - Context gathering from sources
//! - Agent analytics: success rates, failure kinds and completion times
//! - Embedding generation
//! - Automatic source indexing
//! - Task execution
//! - PR creation on task completion, and repair of a branch that stopped merging
//! - Knowledge refresh from web URLs
//! - Promotion of repeated answers to standing instructions
//! - Reception sync: how each change was received once people reviewed it
//! - Learning conventions and strategies from finished runs
//! - Watching whether a shipped fix regressed
//! - Scheduled digests of all of the above
//! - Cleanup tasks

pub mod analytics;
pub mod conflict;
pub mod embeddings;
pub mod evaluation;
pub mod gathering;
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
