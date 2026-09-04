//! Database layer
//!
//! This module provides database access using sqlx with PostgreSQL.
//! Queries use compile-time checked SQL via sqlx macros.

pub mod actions;
pub mod ai_settings;
pub mod audit;
pub mod chats;
pub mod context_gatherings;
pub mod email_verification;
pub mod gathering_events;
pub mod invitations;
pub mod knowledge;
pub mod message_embeddings;
pub mod organization_members;
pub mod organizations;
pub mod password_reset;
pub mod plans;
pub mod projects;
pub mod refresh_tokens;
pub mod reminders;
pub mod sessions;
pub mod sources;
pub mod subscriptions;
pub mod sync_config;
pub mod tasks;
pub mod usage;
pub mod users;
pub mod workspace_members;
pub mod workspace_themes;
pub mod workspaces;

use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

/// Database connection pool
#[derive(Clone)]
pub struct DbPool(PgPool);

impl DbPool {
    /// Create a new database connection pool
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(30))
            .connect(database_url)
            .await?;

        Ok(Self(pool))
    }

    /// Get a reference to the underlying pool
    pub fn inner(&self) -> &PgPool {
        &self.0
    }
}

impl std::ops::Deref for DbPool {
    type Target = PgPool;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Result type for database operations
pub type DbResult<T> = Result<T, sqlx::Error>;
