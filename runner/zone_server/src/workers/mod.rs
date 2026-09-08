//! Background workers for async task processing
//!
//! This module contains workers that run background tasks such as:
//! - Context gathering from sources
//! - Embedding generation
//! - Automatic source indexing
//! - Task execution
//! - PR creation on task completion, and repair of a branch that stopped merging
//! - Knowledge refresh from web URLs
//! - Promotion of repeated answers to standing instructions
//! - Reception sync: how each change was received once people reviewed it
//! - Learning conventions and strategies from finished runs
//! - Cleanup tasks

pub mod conflict;
pub mod embeddings;
pub mod evaluation;
pub mod gathering;
pub mod indexing;
pub mod knowledge_refresh;
pub mod learning;
pub mod pr;
pub mod promotion;
pub mod reception;
pub mod reminders;
pub mod source_resync;
pub mod task;
pub mod titles;
