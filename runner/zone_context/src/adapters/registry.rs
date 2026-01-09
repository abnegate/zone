//! Adapter registry for managing source adapters

use std::collections::HashMap;
use std::sync::Arc;

use super::{AdapterRef, SourceAdapter};
use crate::error::{ContextError, Result};
use zone_core::{Source, SourceType};

/// Registry for source adapters
///
/// Maintains a mapping of source types to their adapter implementations.
#[derive(Default)]
pub struct AdapterRegistry {
    adapters: HashMap<String, AdapterRef>,
}

impl AdapterRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with all default adapters registered
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(super::FilesystemAdapter::new());
        registry.register(super::GitHubAdapter::new());
        registry.register(super::GitLabAdapter::new());
        registry.register(super::NotionAdapter::new());
        registry.register(super::TextAdapter::new());
        registry.register(super::WebAdapter::new());
        registry
    }

    /// Register an adapter
    pub fn register(&mut self, adapter: impl SourceAdapter + 'static) {
        let source_type = adapter.source_type().to_string();
        self.adapters.insert(source_type, Arc::new(adapter));
    }

    /// Register an adapter reference
    pub fn register_ref(&mut self, adapter: AdapterRef) {
        let source_type = adapter.source_type().to_string();
        self.adapters.insert(source_type, adapter);
    }

    /// Get an adapter by source type string
    pub fn get(&self, source_type: &str) -> Option<AdapterRef> {
        self.adapters.get(source_type).cloned()
    }

    /// Get an adapter for a source
    pub fn get_for_source(&self, source: &Source) -> Result<AdapterRef> {
        let type_str = source_type_to_string(&source.source_type);
        self.get(&type_str).ok_or_else(|| {
            ContextError::UnsupportedSourceType(format!(
                "No adapter registered for source type: {}",
                type_str
            ))
        })
    }

    /// Check if an adapter is registered for a source type
    pub fn has_adapter(&self, source_type: &str) -> bool {
        self.adapters.contains_key(source_type)
    }

    /// Get all registered source types
    pub fn registered_types(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }

    /// Get adapter count
    pub fn len(&self) -> usize {
        self.adapters.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.adapters.is_empty()
    }
}

/// Convert SourceType enum to string identifier
fn source_type_to_string(source_type: &SourceType) -> String {
    match source_type {
        SourceType::Filesystem => "filesystem".to_string(),
        SourceType::GitHub => "github".to_string(),
        SourceType::GitLab => "gitlab".to_string(),
        SourceType::GoogleCalendar => "ical".to_string(),
        SourceType::GoogleMail => "imap".to_string(),
        SourceType::Notion => "notion".to_string(),
        SourceType::Slack => "slack".to_string(),
        SourceType::Web => "web".to_string(),
        SourceType::Text => "text".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{FetchConfig, FetchResult, FetchStrategy};
    use async_trait::async_trait;

    // Mock adapter for testing
    struct MockAdapter {
        adapter_type: String,
    }

    #[async_trait]
    impl SourceAdapter for MockAdapter {
        fn source_type(&self) -> &str {
            &self.adapter_type
        }

        async fn verify(&self, _source: &Source) -> Result<()> {
            Ok(())
        }

        async fn estimate_tokens(&self, _source: &Source) -> Result<usize> {
            Ok(1000)
        }

        async fn fetch(
            &self,
            source: &Source,
            _config: &FetchConfig,
            _strategy: FetchStrategy,
            _progress: &dyn crate::adapters::ProgressCallback,
        ) -> Result<FetchResult> {
            Ok(FetchResult::new(source.id, false))
        }
    }

    fn create_test_source(source_type: SourceType) -> Source {
        Source {
            id: uuid::Uuid::new_v4(),
            name: "Test".to_string(),
            source_type,
            category: source_type.category(),
            config: serde_json::json!({}),
            is_active: true,
            last_synced_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_registry_new() {
        let registry = AdapterRegistry::new();
        assert!(registry.is_empty());
    }

    #[test]
    fn test_registry_with_defaults() {
        let registry = AdapterRegistry::with_defaults();
        assert!(!registry.is_empty());
        assert!(registry.has_adapter("filesystem"));
        assert!(registry.has_adapter("github"));
        assert!(registry.has_adapter("gitlab"));
        assert!(registry.has_adapter("notion"));
        assert!(registry.has_adapter("text"));
        assert!(registry.has_adapter("web"));
        assert_eq!(registry.len(), 6);
    }

    #[test]
    fn test_registry_register() {
        let mut registry = AdapterRegistry::new();
        registry.register(MockAdapter {
            adapter_type: "test".to_string(),
        });

        assert!(!registry.is_empty());
        assert_eq!(registry.len(), 1);
        assert!(registry.has_adapter("test"));
    }

    #[test]
    fn test_registry_get() {
        let mut registry = AdapterRegistry::new();
        registry.register(MockAdapter {
            adapter_type: "test".to_string(),
        });

        let adapter = registry.get("test");
        assert!(adapter.is_some());
        assert_eq!(adapter.unwrap().source_type(), "test");

        let missing = registry.get("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_registry_get_for_source() {
        let mut registry = AdapterRegistry::new();
        registry.register(MockAdapter {
            adapter_type: "filesystem".to_string(),
        });

        let source = create_test_source(SourceType::Filesystem);
        let adapter = registry.get_for_source(&source);
        assert!(adapter.is_ok());

        let github_source = create_test_source(SourceType::GitHub);
        let missing = registry.get_for_source(&github_source);
        assert!(missing.is_err());
    }

    #[test]
    fn test_registry_registered_types() {
        let mut registry = AdapterRegistry::new();
        registry.register(MockAdapter {
            adapter_type: "type1".to_string(),
        });
        registry.register(MockAdapter {
            adapter_type: "type2".to_string(),
        });

        let types = registry.registered_types();
        assert_eq!(types.len(), 2);
        assert!(types.contains(&"type1".to_string()));
        assert!(types.contains(&"type2".to_string()));
    }

    #[test]
    fn test_source_type_to_string() {
        assert_eq!(source_type_to_string(&SourceType::Filesystem), "filesystem");
        assert_eq!(source_type_to_string(&SourceType::GitHub), "github");
        assert_eq!(source_type_to_string(&SourceType::GitLab), "gitlab");
        assert_eq!(source_type_to_string(&SourceType::Text), "text");
        assert_eq!(source_type_to_string(&SourceType::Web), "web");
        assert_eq!(source_type_to_string(&SourceType::Slack), "slack");
    }
}
