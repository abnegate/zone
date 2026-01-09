//! External issue tracker synchronization
//!
//! This module provides bi-directional sync between Zone tasks and external issue trackers
//! like GitHub Issues and Linear.

pub mod github;
pub mod linear;

use async_trait::async_trait;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

use crate::db::tasks::TaskRow;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("External API error: {0}")]
    ExternalApiError(String),

    #[error("Webhook verification failed: {0}")]
    WebhookVerificationFailed(String),

    #[error("Invalid webhook payload: {0}")]
    InvalidWebhookPayload(String),

    #[error("Sync conflict: {0}")]
    SyncConflict(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Cryptography error: {0}")]
    CryptoError(String),
}

pub type SyncResult<T> = Result<T, SyncError>;

/// Configuration for a sync provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub id: Uuid,
    pub project_id: Uuid,
    pub provider: String,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub webhook_secret_encrypted: Option<String>,
}

/// External issue representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalIssue {
    /// External system's ID (e.g., GitHub issue number, Linear issue ID)
    pub external_id: String,
    /// URL to the issue in the external system
    pub url: String,
    /// Current state/status in external system
    pub state: IssueState,
    /// Issue metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Issue state across different providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueState {
    Open,
    Closed,
    InProgress,
}

/// Webhook event from external system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookEvent {
    /// Type of event (e.g., "issue_created", "issue_updated", "issue_closed")
    pub event_type: String,
    /// External issue ID
    pub external_id: String,
    /// Event payload
    pub payload: WebhookPayload,
}

/// Webhook payload data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
    /// Issue title
    pub title: Option<String>,
    /// Issue description/body
    pub description: Option<String>,
    /// Issue state
    pub state: Option<IssueState>,
    /// Raw event data for debugging
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
}

/// Sync provider trait
#[async_trait]
pub trait SyncProvider: Send + Sync {
    /// Get provider name (e.g., "github", "linear")
    fn provider_name(&self) -> &str;

    /// Create an issue in the external system from a Zone task
    async fn create_issue(&self, config: &SyncConfig, task: &TaskRow) -> SyncResult<ExternalIssue>;

    /// Update an external issue from task changes
    async fn update_issue(
        &self,
        config: &SyncConfig,
        task: &TaskRow,
        external_id: &str,
    ) -> SyncResult<()>;

    /// Close an external issue
    async fn close_issue(&self, config: &SyncConfig, external_id: &str) -> SyncResult<()>;

    /// Parse and verify webhook payload
    /// Returns the parsed event if signature verification passes
    fn parse_webhook(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        secret: &str,
    ) -> SyncResult<WebhookEvent>;
}

/// Registry for sync providers
#[derive(Clone)]
pub struct SyncRegistry {
    providers: Arc<HashMap<String, Arc<dyn SyncProvider>>>,
}

impl SyncRegistry {
    /// Create a new sync registry with all providers
    pub fn new() -> Self {
        let mut providers: HashMap<String, Arc<dyn SyncProvider>> = HashMap::new();

        // Register GitHub provider
        let github = Arc::new(github::GitHubSyncProvider::new());
        providers.insert(github.provider_name().to_string(), github);

        // Register Linear provider
        let linear = Arc::new(linear::LinearSyncProvider::new());
        providers.insert(linear.provider_name().to_string(), linear);

        Self {
            providers: Arc::new(providers),
        }
    }

    /// Get a provider by name
    pub fn get_provider(&self, name: &str) -> SyncResult<Arc<dyn SyncProvider>> {
        self.providers
            .get(name)
            .cloned()
            .ok_or_else(|| SyncError::ProviderNotFound(name.to_string()))
    }

    /// List all available provider names
    pub fn list_providers(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }
}

impl Default for SyncRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_registry_creates_with_providers() {
        let registry = SyncRegistry::new();
        let providers = registry.list_providers();

        assert!(providers.contains(&"github".to_string()));
        assert!(providers.contains(&"linear".to_string()));
        assert_eq!(providers.len(), 2);
    }

    #[test]
    fn test_sync_registry_get_github_provider() {
        let registry = SyncRegistry::new();
        let provider = registry.get_provider("github");

        assert!(provider.is_ok());
        assert_eq!(provider.unwrap().provider_name(), "github");
    }

    #[test]
    fn test_sync_registry_get_linear_provider() {
        let registry = SyncRegistry::new();
        let provider = registry.get_provider("linear");

        assert!(provider.is_ok());
        assert_eq!(provider.unwrap().provider_name(), "linear");
    }

    #[test]
    fn test_sync_registry_unknown_provider() {
        let registry = SyncRegistry::new();
        let result = registry.get_provider("unknown");

        assert!(result.is_err());
        assert!(matches!(result, Err(SyncError::ProviderNotFound(_))));
    }

    #[test]
    fn test_issue_state_serialization() {
        let state = IssueState::Open;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"open\"");

        let state = IssueState::Closed;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"closed\"");
    }

    #[test]
    fn test_issue_state_deserialization() {
        let state: IssueState = serde_json::from_str("\"open\"").unwrap();
        assert_eq!(state, IssueState::Open);

        let state: IssueState = serde_json::from_str("\"closed\"").unwrap();
        assert_eq!(state, IssueState::Closed);
    }
}
