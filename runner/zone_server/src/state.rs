//! Application state

use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::{OnceCell, Semaphore};
use zone_context::adapters::AdapterRegistry;
use zone_context::context::ContextService;
use zone_context::embeddings::EmbeddingService;
use zone_core::mcp::McpHub;

use crate::cache::Cache;
use crate::config::Config;
use crate::pull::PullRegistry;
use crate::sync::SyncRegistry;
use crate::utils::rate_limit::{RateLimitConfig, RateLimiter};
use crate::ws::task_run::{self, TaskProgressBroadcaster};
use zone_email::EmailService;

/// Maximum concurrent indexing operations
const MAX_CONCURRENT_INDEX: usize = 3;
/// One, because a training upload holds its whole body in memory and a second
/// run would be waiting on the same ComfyUI and the same GPU regardless.
const MAX_CONCURRENT_TRAIN: usize = 1;

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
    pub pull_registry: PullRegistry,
    /// Derived encryption key (32 bytes) for AES-256-GCM
    pub encryption_key: [u8; 32],
    /// Semaphore for limiting concurrent indexing operations
    pub index_semaphore: Arc<Semaphore>,
    /// Training uploads carry their images and clips inline as base64, so a
    /// handler holds the encoded body and the bytes it decodes out of it at
    /// once. One at a time bounds the peak to a single request's worth, and
    /// costs nothing real: a second run would contend for the same ComfyUI.
    pub train_semaphore: Arc<Semaphore>,
    /// Process-wide MCP hub. Connected once, shared across chat turns.
    pub mcp: OnceCell<McpHub>,
    /// Terminal frames for task runs, for sockets and waits to subscribe to.
    /// The process's one instance, which the writers that finish a run also
    /// publish to, so a state built per test observes the same runs.
    pub task_progress: Arc<TaskProgressBroadcaster>,
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
                pull_registry: PullRegistry::new(),
                encryption_key,
                index_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_INDEX)),
                train_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_TRAIN)),
                mcp: OnceCell::new(),
                task_progress: task_run::progress(),
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
                pull_registry: PullRegistry::new(),
                encryption_key,
                index_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_INDEX)),
                train_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_TRAIN)),
                mcp: OnceCell::new(),
                task_progress: task_run::progress(),
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
                pull_registry: PullRegistry::new(),
                encryption_key,
                index_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_INDEX)),
                train_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_TRAIN)),
                mcp: OnceCell::new(),
                task_progress: task_run::progress(),
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

    /// Permits for training uploads, which are held for the whole request.
    pub fn train_semaphore(&self) -> &Arc<Semaphore> {
        &self.inner.train_semaphore
    }

    /// Get the sync registry
    pub fn sync_registry(&self) -> &SyncRegistry {
        &self.inner.sync_registry
    }

    /// Get the background model pull registry
    pub fn pull_registry(&self) -> &PullRegistry {
        &self.inner.pull_registry
    }

    /// MCP servers for this process. Connected once on first chat or task use.
    pub async fn mcp_hub(&self) -> &McpHub {
        self.inner.mcp.get_or_init(McpHub::connect_from_env).await
    }

    /// Terminal frames for task runs. A waiter subscribes here; the writers
    /// that finish a run publish here.
    pub fn task_progress(&self) -> &Arc<TaskProgressBroadcaster> {
        &self.inner.task_progress
    }

    pub fn existing_mcp(&self) -> Option<&McpHub> {
        self.inner.mcp.get()
    }

    /// Install an empty hub so tests never spawn MCP children.
    pub fn disable_mcp(&self) {
        let _ = self.inner.mcp.set(McpHub::new());
    }
}

#[cfg(test)]
impl AppState {
    /// State for unit tests that never reach the database.
    ///
    /// The pool is lazy, so nothing connects unless a test actually queries.
    pub fn for_tests() -> Self {
        let db =
            PgPool::connect_lazy("postgres://localhost/test").expect("a lazy pool needs no server");
        let state = Self::new(test_config(), db, None);
        state.disable_mcp();
        state
    }
}

#[cfg(test)]
pub(crate) fn test_config() -> Config {
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
        gpt4all_models_url: crate::config::DEFAULT_GPT4ALL_MODELS_URL.to_string(),
        huggingface_models_url: crate::config::DEFAULT_HUGGINGFACE_MODELS_URL.to_string(),
        model_search_proxy_url: None,
        encryption_key: "12345678901234567890123456789012".to_string(),
        cors_origins: vec!["*".to_string()],
        cors_allow_credentials: false,
        app_base_url: "http://localhost:3000".to_string(),
        github_api_url: crate::config::DEFAULT_GITHUB_API_URL.to_string(),
        web_search: zone_search::WebSearchConfig::default(),
        comfyui: Default::default(),
        source_index: Default::default(),
        monitoring: Default::default(),
        chat: Default::default(),
        train_upload_limit_mb: 512,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zone_context::adapters::{FilesystemAdapter, GitHubAdapter, TextAdapter};
    use zone_context::embeddings::providers::MockEmbeddingService;
    use zone_email::EmailConfig;

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
            gpt4all_models_url: crate::config::DEFAULT_GPT4ALL_MODELS_URL.to_string(),
            huggingface_models_url: crate::config::DEFAULT_HUGGINGFACE_MODELS_URL.to_string(),
            model_search_proxy_url: None,
            encryption_key: "12345678901234567890123456789012".to_string(),
            cors_origins: vec!["*".to_string()],
            cors_allow_credentials: false,
            app_base_url: "http://localhost:3000".to_string(),
            github_api_url: crate::config::DEFAULT_GITHUB_API_URL.to_string(),
            web_search: Default::default(),
            comfyui: Default::default(),
            source_index: Default::default(),
            monitoring: Default::default(),
            chat: Default::default(),
            train_upload_limit_mb: 512,
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

    fn context_dependencies(
        pool: &PgPool,
    ) -> (
        Arc<AdapterRegistry>,
        Arc<dyn EmbeddingService>,
        Arc<ContextService>,
    ) {
        let mut registry = AdapterRegistry::new();
        registry.register(TextAdapter::new());
        let registry = Arc::new(registry);
        let embedding: Arc<dyn EmbeddingService> = Arc::new(MockEmbeddingService::new(32));
        let context = Arc::new(ContextService::new(
            pool.clone(),
            registry.clone(),
            embedding.clone(),
        ));
        (registry, embedding, context)
    }

    #[tokio::test]
    async fn constructors_and_accessors_preserve_supplied_services() {
        let pool = PgPool::connect_lazy("postgres://localhost/state-services")
            .expect("a lazy pool needs no server");
        let (registry, embedding, context) = context_dependencies(&pool);
        let config = create_test_config();
        let state = AppState::new_with_services(
            config.clone(),
            pool.clone(),
            None,
            registry.clone(),
            embedding.clone(),
            context.clone(),
        );

        assert_eq!(state.config().host, config.host);
        assert!(!state.db().is_closed());
        assert!(state.cache().is_none());
        assert!(Arc::ptr_eq(state.adapter_registry().unwrap(), &registry));
        assert!(Arc::ptr_eq(state.embedding_service().unwrap(), &embedding));
        assert!(Arc::ptr_eq(state.context_service().unwrap(), &context));
        assert!(state.email_service().is_none());
        assert_eq!(
            state.index_semaphore().available_permits(),
            MAX_CONCURRENT_INDEX
        );
        assert_eq!(
            state.train_semaphore().available_permits(),
            MAX_CONCURRENT_TRAIN
        );
        assert_eq!(state.encryption_key().len(), 32);
        let _ = state.rate_limiter();
        let _ = state.sync_registry();
        let _ = state.pull_registry();
        assert!(state.existing_mcp().is_none());
        state.disable_mcp();
        assert!(state.existing_mcp().is_some());
        assert!(std::ptr::eq(
            state.mcp_hub().await,
            state.existing_mcp().unwrap()
        ));

        let email = Arc::new(
            EmailService::new(EmailConfig {
                smtp_host: "localhost".to_string(),
                smtp_port: 2525,
                smtp_user: "zone".to_string(),
                smtp_password: "secret".to_string(),
                from_email: "zone@example.com".to_string(),
                from_name: "Zone".to_string(),
            })
            .expect("valid SMTP settings"),
        );
        let with_email = AppState::new_with_all_services(
            config,
            pool,
            None,
            registry,
            embedding,
            context,
            Some(email.clone()),
        );
        assert!(Arc::ptr_eq(with_email.email_service().unwrap(), &email));
        assert_eq!(with_email.train_semaphore().available_permits(), 1);
    }

    #[tokio::test]
    async fn cache_accessor_exposes_a_connected_cache_when_available() {
        let Ok(cache) = Cache::connect("redis://localhost:6379").await else {
            return;
        };
        let pool = PgPool::connect_lazy("postgres://localhost/state-cache")
            .expect("a lazy pool needs no server");
        let state = AppState::new(create_test_config(), pool, Some(cache));
        assert!(state.cache().is_some());
    }
}
