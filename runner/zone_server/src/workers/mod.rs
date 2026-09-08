//! Background workers for async task processing
//!
//! This module contains workers that run background tasks such as:
//! - Context gathering from sources
//! - Embedding generation
//! - Automatic source indexing
//! - Task execution
//! - PR creation on task completion
//! - Knowledge refresh from web URLs
//! - Promotion of repeated answers to standing instructions
//! - Learning conventions and strategies from finished runs
//! - Cleanup tasks

pub mod embeddings;
pub mod evaluation;
pub mod gathering;
pub mod indexing;
pub mod knowledge_refresh;
pub mod learning;
pub mod pr;
pub mod promotion;
pub mod reminders;
pub mod source_resync;
pub mod task;
pub mod titles;
