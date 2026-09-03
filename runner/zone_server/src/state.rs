//! Application state

use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Semaphore;
use zone_context::adapters::AdapterRegistry;
use zone_context::context::ContextService;
use zone_context::embeddings::EmbeddingService;

use crate::cache::Cache;
use crate::config::Config;
use crate::services::email::EmailService;
use crate::sync::SyncRegistry;
use crate::utils::rate_limit::{RateLimitConfig, RateLimiter};

/// Maximum concurrent indexing operations
const MAX_CONCURRENT_INDEX: usize = 3;

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
    pub adapter_registry: Option<Arc<AdapterRegistry>>,
    pub embedding_service: Option<Arc<dyn EmbeddingService>>,
    pub context_service: Option<Arc<ContextService>>,
    pub email_service: Option<Arc<EmailService>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub sync_registry: SyncRegistry,
    /// Derived encryption key (32 bytes) for AES-256-GCM
    pub encryption_key: [u8; 32],
    /// Semaphore for limiting concurrent indexing operations
    pub index_semaphore: Arc<Semaphore>,
}

impl AppState {
    /// Create a new application state without zone_context services
    pub fn new(config: Config, db: PgPool, cache: Option<Cache>) -> Self {
        // Derive encryption key from config
        let encryption_key = crate::crypto::derive_key(config.encryption_key())
            .expect("Encryption key should be valid (validated in Config::from_env)");

        // Create rate limiter with default config (10 requests per minute)
        let rate_limiter = Arc::new(RateLimiter::new(RateLimitConfig::default()));

        // Spawn background cleanup task to prevent unbounded memory growth
        let rate_limiter_clone = rate_limiter.clone();
        tokio::spawn(async move {
            use std::time::Duration;
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 min
            loop {
                interval.tick().await;
                rate_limiter_clone.cleanup();
                tracing::debug!("Rate limiter cleanup completed");
            }
        });

        Self {
            inner: Arc::new(AppStateInner {
                config,
                db,
                cache,
                adapter_registry: None,
                embedding_service: None,
                context_service: None,
                email_service: None,
                rate_limiter,
                sync_registry: SyncRegistry::new(),
                encryption_key,
                index_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_INDEX)),
            }),
        }
    }

    /// Create a new application state with zone_context services
    pub fn new_with_services(
        config: Config,
        db: PgPool,
        cache: Option<Cache>,
        adapter_registry: Arc<AdapterRegistry>,
        embedding_service: Arc<dyn EmbeddingService>,
        context_service: Arc<ContextService>,
    ) -> Self {
        // Derive encryption key from config
        let encryption_key = crate::crypto::derive_key(config.encryption_key())
            .expect("Encryption key should be valid (validated in Config::from_env)");

        // Create rate limiter with default config (10 requests per minute)
        let rate_limiter = Arc::new(RateLimiter::new(RateLimitConfig::default()));

        // Spawn background cleanup task to prevent unbounded memory growth
        let rate_limiter_clone = rate_limiter.clone();
        tokio::spawn(async move {
            use std::time::Duration;
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 min
            loop {
                interval.tick().await;
                rate_limiter_clone.cleanup();
                tracing::debug!("Rate limiter cleanup completed");
            }
        });

        Self {
            inner: Arc::new(AppStateInner {
                config,
                db,
                cache,
                adapter_registry: Some(adapter_registry),
                embedding_service: Some(embedding_service),
                context_service: Some(context_service),
                email_service: None,
                rate_limiter,
                sync_registry: SyncRegistry::new(),
                encryption_key,
                index_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_INDEX)),
            }),
        }
    }

    /// Create a new application state with all services including email
    pub fn new_with_all_services(
        config: Config,
        db: PgPool,
        cache: Option<Cache>,
        adapter_registry: Arc<AdapterRegistry>,
        embedding_service: Arc<dyn EmbeddingService>,
        context_service: Arc<ContextService>,
        email_service: Option<Arc<EmailService>>,
    ) -> Self {
        // Derive encryption key from config
        let encryption_key = crate::crypto::derive_key(config.encryption_key())
            .expect("Encryption key should be valid (validated in Config::from_env)");

        // Create rate limiter with default config (10 requests per minute)
        let rate_limiter = Arc::new(RateLimiter::new(RateLimitConfig::default()));

        // Spawn background cleanup task to prevent unbounded memory growth
        let rate_limiter_clone = rate_limiter.clone();
        tokio::spawn(async move {
            use std::time::Duration;
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 min
            loop {
                interval.tick().await;
                rate_limiter_clone.cleanup();
                tracing::debug!("Rate limiter cleanup completed");
            }
        });

        Self {
            inner: Arc::new(AppStateInner {
                config,
                db,
                cache,
                adapter_registry: Some(adapter_registry),
                embedding_service: Some(embedding_service),
                context_service: Some(context_service),
                email_service,
                rate_limiter,
                sync_registry: SyncRegistry::new(),
                encryption_key,
                index_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_INDEX)),
            }),
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

    /// Get the adapter registry (if available)
    pub fn adapter_registry(&self) -> Option<&Arc<AdapterRegistry>> {
        self.inner.adapter_registry.as_ref()
    }

    /// Get the embedding service (if available)
    pub fn embedding_service(&self) -> Option<&Arc<dyn EmbeddingService>> {
        self.inner.embedding_service.as_ref()
    }

    /// Get the context service (if available)
    pub fn context_service(&self) -> Option<&Arc<ContextService>> {
        self.inner.context_service.as_ref()
    }

    /// Get the encryption key for source credentials
    pub fn encryption_key(&self) -> &[u8; 32] {
        &self.inner.encryption_key
    }

    /// Get the email service (if available)
    pub fn email_service(&self) -> Option<&Arc<EmailService>> {
        self.inner.email_service.as_ref()
    }

    /// Get the rate limiter
    pub fn rate_limiter(&self) -> &Arc<RateLimiter> {
        &self.inner.rate_limiter
    }

    /// Get the index semaphore for limiting concurrent indexing operations
    pub fn index_semaphore(&self) -> &Arc<Semaphore> {
        &self.inner.index_semaphore
    }

    /// Get the sync registry
    pub fn sync_registry(&self) -> &SyncRegistry {
        &self.inner.sync_registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zone_context::adapters::{FilesystemAdapter, GitHubAdapter, TextAdapter};

    fn create_test_config() -> Config {
        Config {
            host: "localhost".to_string(),
            port: 8000,
            database_url: "postgres://localhost/test".to_string(),
            redis_url: "redis://localhost:6379".to_string(),
            jwt_secret: "test-secret-key-with-at-least-32-chars".to_string(),
            jwt_access_lifetime: 900,
            jwt_refresh_lifetime: 604800,
            litellm_host: "http://localhost:4000".to_string(),
            litellm_key: "test-key".to_string(),
            ollama_host: "http://localhost:11434".to_string(),
            encryption_key: "12345678901234567890123456789012".to_string(),
            cors_origins: vec!["*".to_string()],
            cors_allow_credentials: false,
            app_base_url: "http://localhost:3000".to_string(),
            web_search: Default::default(),
        }
    }

    #[test]
    fn test_adapter_registry_initialization() {
        // Given: Empty adapter registry
        let mut registry = AdapterRegistry::new();

        // When: Registering text, filesystem, and github adapters
        registry.register(TextAdapter::new());
        registry.register(FilesystemAdapter::new());
        registry.register(GitHubAdapter::new());

        // Then: Should have all three adapters registered
        assert_eq!(registry.len(), 3);
        assert!(registry.has_adapter("text"));
        assert!(registry.has_adapter("filesystem"));
        assert!(registry.has_adapter("github"));

        let types = registry.registered_types();
        assert!(types.contains(&"text".to_string()));
        assert!(types.contains(&"filesystem".to_string()));
        assert!(types.contains(&"github".to_string()));
    }

    // Note: AppState with services requires a real database connection and embedding service.
    // These tests are better suited for integration tests with proper test fixtures.
    // The test below verifies basic AppState construction and service accessor methods.

    #[tokio::test]
    async fn test_appstate_without_services() {
        // Given: Mock configuration
        let config = create_test_config();
        let pool_options = sqlx::postgres::PgPoolOptions::new().max_connections(1);

        if let Ok(pool) = pool_options.connect(&config.database_url).await {
            // When: Creating AppState without services
            let state = AppState::new(config, pool, None);

            // Then: Service accessors should return None
            assert!(state.adapter_registry().is_none());
            assert!(state.embedding_service().is_none());
            assert!(state.context_service().is_none());
        }
    }
}
