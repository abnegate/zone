//! Background workers for async task processing
//!
//! This module contains workers that run background tasks such as:
//! - Context gathering from sources
//! - Embedding generation
//! - Automatic source indexing
//! - Task execution
//! - PR creation on task completion
//! - Knowledge refresh from web URLs
//! - Cleanup tasks

pub mod embeddings;
pub mod gathering;
pub mod indexing;
pub mod knowledge_refresh;
pub mod pr;
pub mod reminders;
pub mod task;
pub mod titles;
