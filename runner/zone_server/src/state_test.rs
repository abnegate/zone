//! Tests for AppState with zone_context services

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use zone_context::adapters::{AdapterRegistry, FilesystemAdapter, GitHubAdapter, TextAdapter};
    use zone_context::context::ContextService;
    use zone_context::embeddings::providers::MockEmbeddingService;

    use crate::cache::Cache;
    use crate::config::Config;
    use crate::state::AppState;

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
        }
    }

    #[tokio::test]
    async fn test_adapter_registry_initialization() {
        // Given: Empty adapter registry
        let mut registry = AdapterRegistry::new();

        // When: Registering text, filesystem, and github adapters
        registry.register(TextAdapter::new());
        registry.register(FilesystemAdapter::new());
        registry.register(GitHubAdapter::new(None));

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

    #[sqlx::test]
    async fn test_appstate_construction_with_services(pool: sqlx::PgPool) {
        // Given: All required components
        let config = create_test_config();

        // Create adapter registry with registered adapters
        let mut registry = AdapterRegistry::new();
        registry.register(TextAdapter::new());
        registry.register(FilesystemAdapter::new());
        registry.register(GitHubAdapter::new(None));
        let adapter_registry = Arc::new(registry);

        // Create mock embedding service
        let embedding_service: Arc<dyn zone_context::embeddings::EmbeddingService> =
            Arc::new(MockEmbeddingService::new(768));

        // Create context service
        let context_service = Arc::new(ContextService::new(
            pool.clone(),
            adapter_registry.clone(),
            embedding_service.clone(),
        ));

        // When: Creating AppState with all services
        let state = AppState::new_with_services(
            config,
            pool,
            None, // No cache for this test
            adapter_registry.clone(),
            embedding_service.clone(),
            context_service.clone(),
        );

        // Then: Should have all services accessible
        assert!(state.adapter_registry().is_some());
        assert!(state.embedding_service().is_some());
        assert!(state.context_service().is_some());

        let registry_ref = state.adapter_registry().unwrap();
        assert!(registry_ref.has_adapter("text"));
        assert!(registry_ref.has_adapter("filesystem"));
        assert!(registry_ref.has_adapter("github"));

        let embedding_ref = state.embedding_service().unwrap();
        assert_eq!(embedding_ref.dimension(), 768);

        let _context_ref = state.context_service().unwrap();
    }

    #[sqlx::test]
    async fn test_appstate_without_services(pool: sqlx::PgPool) {
        // Given: Only basic components (no zone_context services)
        let config = create_test_config();

        // When: Creating AppState without services
        let state = AppState::new(config, pool, None);

        // Then: Service accessors should return None
        assert!(state.adapter_registry().is_none());
        assert!(state.embedding_service().is_none());
        assert!(state.context_service().is_none());
    }
}
