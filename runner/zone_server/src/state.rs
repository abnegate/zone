//! Application state

use sqlx::PgPool;
use std::sync::Arc;

use crate::cache::Cache;
use crate::config::Config;

/// Shared application state
///
/// This state is cloneable and cheap to share across handlers.
/// It implements `FromRef<AppState>` automatically via the Clone trait.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<AppStateInner>,
}

struct AppStateInner {
    pub config: Config,
    pub db: PgPool,
    pub cache: Option<Cache>,
}

impl AppState {
    /// Create a new application state
    pub fn new(config: Config, db: PgPool, cache: Option<Cache>) -> Self {
        Self {
            inner: Arc::new(AppStateInner { config, db, cache }),
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// Get the database pool
    pub fn db(&self) -> &PgPool {
        &self.inner.db
    }

    /// Get the cache (if available)
    pub fn cache(&self) -> Option<&Cache> {
        self.inner.cache.as_ref()
    }
}
