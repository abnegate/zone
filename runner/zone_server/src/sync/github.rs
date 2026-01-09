//! GitHub Issues synchronization provider

use async_trait::async_trait;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;

use super::{
    ExternalIssue, IssueState, SyncConfig, SyncError, SyncProvider, SyncResult, WebhookEvent,
    WebhookPayload,
};
use crate::db::tasks::TaskRow;

/// GitHub-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubConfig {
    /// Repository owner
    pub owner: String,
    /// Repository name
    pub repo: String,
    /// GitHub API token
    pub token: String,
    /// Optional: Custom field mappings
    #[serde(default)]
    pub field_mappings: HashMap<String, String>,
}

/// GitHub issue response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitHubIssue {
    number: i64,
    html_url: String,
    state: String,
    title: String,
    body: Option<String>,
}

/// GitHub webhook event
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitHubWebhookPayload {
    action: String,
    issue: GitHubIssue,
}

/// GitHub sync provider
#[derive(Debug, Clone)]
pub struct GitHubSyncProvider {
    client: reqwest::Client,
    base_url: String,
}

impl GitHubSyncProvider {
    /// Create a new GitHub sync provider
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://api.github.com".to_string(),
        }
    }

    /// Create with custom base URL (for testing)
    #[cfg(test)]
    pub fn with_base_url(base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
        }
    }

    /// Parse GitHub config from sync config
    fn parse_config(&self, config: &SyncConfig) -> SyncResult<GitHubConfig> {
        serde_json::from_value(config.config.clone())
            .map_err(|e| SyncError::InvalidConfig(format!("Invalid GitHub config: {}", e)))
    }

    /// Verify HMAC-SHA256 signature for GitHub webhooks
    fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
        use hmac::{Hmac, Mac};
        use subtle::ConstantTimeEq;
        type HmacSha256 = Hmac<Sha256>;

        // GitHub sends signature as "sha256=<hex>"
        if !signature.starts_with("sha256=") {
            return false;
        }

        let expected_sig = &signature[7..]; // Skip "sha256=" prefix

        // Compute HMAC-SHA256
        let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
            Ok(m) => m,
            Err(_) => return false,
        };

        mac.update(body);
        let result = mac.finalize();
        let computed_sig = hex::encode(result.into_bytes());

        // Constant-time comparison to prevent timing attacks
        computed_sig
            .as_bytes()
            .ct_eq(expected_sig.as_bytes())
            .into()
    }

    /// Map Zone task status to GitHub issue state
    fn map_task_status_to_github_state(status: &str) -> &str {
        match status {
            "complete" | "blocked" => "closed",
            _ => "open",
        }
    }

    /// Map GitHub issue state to Zone IssueState
    fn map_github_state_to_issue_state(state: &str) -> IssueState {
        match state {
            "closed" => IssueState::Closed,
            "open" => IssueState::Open,
            _ => IssueState::Open,
        }
    }
}

impl Default for GitHubSyncProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SyncProvider for GitHubSyncProvider {
    fn provider_name(&self) -> &str {
        "github"
    }

    async fn create_issue(&self, config: &SyncConfig, task: &TaskRow) -> SyncResult<ExternalIssue> {
        let gh_config = self.parse_config(config)?;

        // Build issue body from task description and acceptance criteria
        let mut body = task.description.clone();
        if let Some(ref criteria) = task.acceptance_criteria {
            body.push_str("\n\n## Acceptance Criteria\n\n");
            body.push_str(criteria);
        }

        // Create issue payload
        let payload = serde_json::json!({
            "title": task.title,
            "body": body,
        });

        // Make API request
        let url = format!(
            "{}/repos/{}/{}/issues",
            self.base_url, gh_config.owner, gh_config.repo
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", gh_config.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "zone-sync")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(SyncError::ExternalApiError(format!(
                "GitHub API error {}: {}",
                status, error_text
            )));
        }

        let issue: GitHubIssue = response.json().await?;

        Ok(ExternalIssue {
            external_id: issue.number.to_string(),
            url: issue.html_url,
            state: Self::map_github_state_to_issue_state(&issue.state),
            metadata: HashMap::new(),
        })
    }

    async fn update_issue(
        &self,
        config: &SyncConfig,
        task: &TaskRow,
        external_id: &str,
    ) -> SyncResult<()> {
        let gh_config = self.parse_config(config)?;

        // Build issue body
        let mut body = task.description.clone();
        if let Some(ref criteria) = task.acceptance_criteria {
            body.push_str("\n\n## Acceptance Criteria\n\n");
            body.push_str(criteria);
        }

        // Determine GitHub state from task status
        let state = Self::map_task_status_to_github_state(&task.status);

        // Update issue payload
        let payload = serde_json::json!({
            "title": task.title,
            "body": body,
            "state": state,
        });

        // Make API request
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            self.base_url, gh_config.owner, gh_config.repo, external_id
        );

        let response = self
            .client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", gh_config.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "zone-sync")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(SyncError::ExternalApiError(format!(
                "GitHub API error {}: {}",
                status, error_text
            )));
        }

        Ok(())
    }

    async fn close_issue(&self, config: &SyncConfig, external_id: &str) -> SyncResult<()> {
        let gh_config = self.parse_config(config)?;

        // Close issue payload
        let payload = serde_json::json!({
            "state": "closed",
        });

        // Make API request
        let url = format!(
            "{}/repos/{}/{}/issues/{}",
            self.base_url, gh_config.owner, gh_config.repo, external_id
        );

        let response = self
            .client
            .patch(&url)
            .header("Authorization", format!("Bearer {}", gh_config.token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "zone-sync")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(SyncError::ExternalApiError(format!(
                "GitHub API error {}: {}",
                status, error_text
            )));
        }

        Ok(())
    }

    fn parse_webhook(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        secret: &str,
    ) -> SyncResult<WebhookEvent> {
        // Verify signature
        let signature = headers
            .get("X-Hub-Signature-256")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                SyncError::WebhookVerificationFailed(
                    "Missing X-Hub-Signature-256 header".to_string(),
                )
            })?;

        if !Self::verify_signature(secret, body, signature) {
            return Err(SyncError::WebhookVerificationFailed(
                "Invalid signature".to_string(),
            ));
        }

        // Parse payload
        let payload: GitHubWebhookPayload = serde_json::from_slice(body).map_err(|e| {
            SyncError::InvalidWebhookPayload(format!("Failed to parse JSON: {}", e))
        })?;

        // Map event type
        let event_type = match payload.action.as_str() {
            "opened" => "issue_created",
            "edited" => "issue_updated",
            "closed" => "issue_closed",
            "reopened" => "issue_reopened",
            _ => &payload.action,
        };

        Ok(WebhookEvent {
            event_type: event_type.to_string(),
            external_id: payload.issue.number.to_string(),
            payload: WebhookPayload {
                title: Some(payload.issue.title.clone()),
                description: payload.issue.body.clone(),
                state: Some(Self::map_github_state_to_issue_state(&payload.issue.state)),
                raw: Some(serde_json::to_value(&payload).unwrap_or_default()),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_provider_name() {
        let provider = GitHubSyncProvider::new();
        assert_eq!(provider.provider_name(), "github");
    }

    #[test]
    fn test_verify_signature_valid() {
        let secret = "my-secret";
        let body = b"test payload";

        // Compute expected signature
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        let sig = format!("sha256={}", hex::encode(result.into_bytes()));

        assert!(GitHubSyncProvider::verify_signature(secret, body, &sig));
    }

    #[test]
    fn test_verify_signature_invalid() {
        let secret = "my-secret";
        let body = b"test payload";
        let invalid_sig = "sha256=invalid";

        assert!(!GitHubSyncProvider::verify_signature(
            secret,
            body,
            invalid_sig
        ));
    }

    #[test]
    fn test_verify_signature_wrong_secret() {
        let secret = "my-secret";
        let body = b"test payload";

        // Compute signature with different secret
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(b"wrong-secret").unwrap();
        mac.update(body);
        let result = mac.finalize();
        let sig = format!("sha256={}", hex::encode(result.into_bytes()));

        assert!(!GitHubSyncProvider::verify_signature(secret, body, &sig));
    }

    #[test]
    fn test_verify_signature_missing_prefix() {
        let secret = "my-secret";
        let body = b"test payload";
        let sig_no_prefix = "abcdef1234567890";

        assert!(!GitHubSyncProvider::verify_signature(
            secret,
            body,
            sig_no_prefix
        ));
    }

    #[test]
    fn test_map_task_status_to_github_state() {
        assert_eq!(
            GitHubSyncProvider::map_task_status_to_github_state("complete"),
            "closed"
        );
        assert_eq!(
            GitHubSyncProvider::map_task_status_to_github_state("blocked"),
            "closed"
        );
        assert_eq!(
            GitHubSyncProvider::map_task_status_to_github_state("created"),
            "open"
        );
        assert_eq!(
            GitHubSyncProvider::map_task_status_to_github_state("in_progress"),
            "open"
        );
    }

    #[test]
    fn test_map_github_state_to_issue_state() {
        assert_eq!(
            GitHubSyncProvider::map_github_state_to_issue_state("open"),
            IssueState::Open
        );
        assert_eq!(
            GitHubSyncProvider::map_github_state_to_issue_state("closed"),
            IssueState::Closed
        );
    }

    #[test]
    fn test_parse_webhook_missing_signature() {
        let provider = GitHubSyncProvider::new();
        let headers = HeaderMap::new();
        let body = b"{}";
        let secret = "test-secret";

        let result = provider.parse_webhook(&headers, body, secret);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(SyncError::WebhookVerificationFailed(_))
        ));
    }

    #[test]
    fn test_parse_webhook_invalid_json() {
        let provider = GitHubSyncProvider::new();
        let body = b"not valid json";
        let secret = "test-secret";

        // Compute valid signature
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        let sig = format!("sha256={}", hex::encode(result.into_bytes()));

        let mut headers = HeaderMap::new();
        headers.insert("X-Hub-Signature-256", sig.parse().unwrap());

        let result = provider.parse_webhook(&headers, body, secret);
        assert!(result.is_err());
        assert!(matches!(result, Err(SyncError::InvalidWebhookPayload(_))));
    }

    #[test]
    fn test_parse_config_valid() {
        let provider = GitHubSyncProvider::new();
        let config = SyncConfig {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            provider: "github".to_string(),
            enabled: true,
            config: serde_json::json!({
                "owner": "test-owner",
                "repo": "test-repo",
                "token": "ghp_test123",
            }),
            webhook_secret_encrypted: None,
        };

        let gh_config = provider.parse_config(&config).unwrap();
        assert_eq!(gh_config.owner, "test-owner");
        assert_eq!(gh_config.repo, "test-repo");
        assert_eq!(gh_config.token, "ghp_test123");
    }

    #[test]
    fn test_parse_config_invalid() {
        let provider = GitHubSyncProvider::new();
        let config = SyncConfig {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            provider: "github".to_string(),
            enabled: true,
            config: serde_json::json!({
                "invalid": "config"
            }),
            webhook_secret_encrypted: None,
        };

        let result = provider.parse_config(&config);
        assert!(result.is_err());
        assert!(matches!(result, Err(SyncError::InvalidConfig(_))));
    }
}
