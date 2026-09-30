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

use crate::db::sync_config::{SyncDirection, SyncEventType, UnknownSyncValue};
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

/// A verified webhook delivery: an issue event to apply, or one to acknowledge and skip
#[derive(Debug, Clone)]
pub enum Delivery {
    Issue(WebhookEvent),
    Ignored(IgnoredDelivery),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", content = "event", rename_all = "snake_case")]
pub enum IgnoredDelivery {
    Ping,
    NotAnIssue(String),
}

impl IgnoredDelivery {
    pub fn message(&self) -> String {
        match self {
            Self::Ping => "Ping acknowledged".to_string(),
            Self::NotAnIssue(event) => {
                format!("Ignored {event} delivery; only issue events are synced")
            }
        }
    }
}

/// Webhook event from external system
#[derive(Debug, Clone)]
pub struct WebhookEvent {
    pub event_type: SyncEventType,
    /// External issue ID
    pub external_id: String,
    /// The issue's page in the external system
    pub url: Option<String>,
    pub origin: IssueOrigin,
    /// Event payload
    pub payload: WebhookPayload,
}

/// Where an issue lives and who opened it, as far as its delivery says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueOrigin {
    GitHub {
        /// The repository's `owner/name`
        repository: Option<String>,
        author_association: github::AuthorAssociation,
    },
    Linear {
        project_id: Option<String>,
    },
}

impl IssueOrigin {
    /// Whether an issue from here may become a new task under `settings`: a
    /// GitHub issue in the configured repository opened by someone with write
    /// access to it, or a Linear issue in the configured project.
    pub fn may_become_task(&self, settings: &Settings) -> bool {
        match self {
            Self::GitHub {
                repository,
                author_association,
            } => {
                let configured = settings
                    .external_repo_url
                    .as_deref()
                    .and_then(github::repository_name);
                author_association.can_write()
                    && matches!(
                        (repository, configured),
                        (Some(repository), Some(configured))
                            if repository.to_lowercase() == configured
                    )
            }
            Self::Linear { project_id } => matches!(
                (project_id.as_deref(), settings.external_project_id.as_deref()),
                (Some(project_id), Some(configured))
                    if !configured.trim().is_empty()
                        && project_id.trim().eq_ignore_ascii_case(configured.trim())
            ),
        }
    }
}

/// What a `sync_configs.config` holds: which way items move and where from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub direction: Option<String>,
    pub external_repo_url: Option<String>,
    pub external_project_id: Option<String>,
}

impl Settings {
    /// The settings a stored configuration spells; fields it lacks, or holds
    /// as anything but text, read as unset.
    pub fn from_config(config: &serde_json::Value) -> Self {
        let field = |name: &str| {
            config
                .get(name)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        Self {
            direction: field("direction"),
            external_repo_url: field("external_repo_url"),
            external_project_id: field("external_project_id"),
        }
    }

    /// The configured direction, bidirectional when none is stored.
    pub fn direction(&self) -> Result<SyncDirection, UnknownSyncValue> {
        self.direction
            .as_deref()
            .map_or(Ok(SyncDirection::Bidirectional), str::parse)
    }
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

    /// Verify a delivery's signature, then classify it as an issue event or one to ignore
    fn parse_webhook(&self, headers: &HeaderMap, body: &[u8], secret: &str)
    -> SyncResult<Delivery>;
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
    fn an_ignored_delivery_serializes_its_reason_and_event() {
        assert_eq!(
            serde_json::to_value(IgnoredDelivery::Ping).unwrap(),
            serde_json::json!({ "reason": "ping" })
        );
        assert_eq!(
            serde_json::to_value(IgnoredDelivery::NotAnIssue("Comment".to_string())).unwrap(),
            serde_json::json!({ "reason": "not_an_issue", "event": "Comment" })
        );
    }

    fn github_settings(url: &str) -> Settings {
        Settings::from_config(&serde_json::json!({
            "direction": "inbound",
            "external_repo_url": url,
            "external_project_id": null
        }))
    }

    fn github_origin(
        repository: Option<&str>,
        author_association: github::AuthorAssociation,
    ) -> IssueOrigin {
        IssueOrigin::GitHub {
            repository: repository.map(str::to_string),
            author_association,
        }
    }

    #[test]
    fn settings_read_the_stored_direction_and_targets() {
        let settings = github_settings("https://github.com/acme/widgets");

        assert_eq!(settings.direction(), Ok(SyncDirection::Inbound));
        assert_eq!(
            settings.external_repo_url.as_deref(),
            Some("https://github.com/acme/widgets")
        );
        assert_eq!(settings.external_project_id, None);
    }

    #[test]
    fn settings_without_a_direction_are_bidirectional() {
        let settings = Settings::from_config(&serde_json::json!({ "owner": "acme" }));

        assert_eq!(settings.direction(), Ok(SyncDirection::Bidirectional));
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn settings_with_an_unknown_direction_say_so() {
        let settings = Settings::from_config(&serde_json::json!({ "direction": "sideways" }));

        assert!(settings.direction().is_err());
    }

    #[test]
    fn a_github_issue_becomes_a_task_only_from_a_writer_in_the_configured_repository() {
        let settings = github_settings("https://github.com/Acme/Widgets.git");
        let writer = github::AuthorAssociation::Member;

        assert!(github_origin(Some("acme/widgets"), writer).may_become_task(&settings));
        assert!(github_origin(Some("ACME/Widgets"), writer).may_become_task(&settings));
        assert!(!github_origin(Some("acme/gadgets"), writer).may_become_task(&settings));
        assert!(!github_origin(None, writer).may_become_task(&settings));
        assert!(
            !github_origin(Some("acme/widgets"), github::AuthorAssociation::Contributor)
                .may_become_task(&settings)
        );
        assert!(
            !github_origin(Some("acme/widgets"), writer).may_become_task(&Settings::default()),
            "a sync without a repository creates nothing"
        );
    }

    #[test]
    fn a_linear_issue_becomes_a_task_only_in_the_configured_project() {
        let settings =
            Settings::from_config(&serde_json::json!({ "external_project_id": "Project-1" }));
        let origin = |project_id: Option<&str>| IssueOrigin::Linear {
            project_id: project_id.map(str::to_string),
        };

        assert!(origin(Some("project-1")).may_become_task(&settings));
        assert!(!origin(Some("project-2")).may_become_task(&settings));
        assert!(!origin(None).may_become_task(&settings));
        assert!(!origin(Some("")).may_become_task(&Settings::from_config(
            &serde_json::json!({ "external_project_id": "" })
        )));
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
