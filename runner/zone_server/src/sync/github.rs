//! GitHub Issues synchronization provider

use async_trait::async_trait;
use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;

use super::{
    Delivery, ExternalIssue, IgnoredDelivery, IssueState, SyncConfig, SyncError, SyncProvider,
    SyncResult, WebhookEvent, WebhookPayload,
};
use crate::db::sync_config::SyncEventType;
use crate::db::tasks::TaskRow;

pub const PROVIDER_NAME: &str = "github";

const SIGNATURE_HEADER: &str = "X-Hub-Signature-256";
const EVENT_HEADER: &str = "X-GitHub-Event";
const ISSUES_EVENT: &str = "issues";
const PING_EVENT: &str = "ping";

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
        use hmac::{Hmac, KeyInit, Mac};
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

    fn map_action_to_event_type(action: &str) -> SyncEventType {
        match action {
            "opened" => SyncEventType::Create,
            "closed" | "deleted" => SyncEventType::Close,
            _ => SyncEventType::Update,
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
        PROVIDER_NAME
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
    ) -> SyncResult<Delivery> {
        let signature = headers
            .get(SIGNATURE_HEADER)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                SyncError::WebhookVerificationFailed(format!("Missing {SIGNATURE_HEADER} header"))
            })?;

        if !Self::verify_signature(secret, body, signature) {
            return Err(SyncError::WebhookVerificationFailed(
                "Invalid signature".to_string(),
            ));
        }

        let event = headers
            .get(EVENT_HEADER)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                SyncError::InvalidWebhookPayload(format!("Missing {EVENT_HEADER} header"))
            })?;
        match event {
            ISSUES_EVENT => {}
            PING_EVENT => return Ok(Delivery::Ignored(IgnoredDelivery::Ping)),
            other => {
                return Ok(Delivery::Ignored(IgnoredDelivery::NotAnIssue(
                    other.to_string(),
                )));
            }
        }

        let payload: GitHubWebhookPayload = serde_json::from_slice(body).map_err(|e| {
            SyncError::InvalidWebhookPayload(format!("Failed to parse JSON: {}", e))
        })?;

        Ok(Delivery::Issue(WebhookEvent {
            event_type: Self::map_action_to_event_type(&payload.action),
            external_id: payload.issue.number.to_string(),
            payload: WebhookPayload {
                title: Some(payload.issue.title.clone()),
                description: payload.issue.body.clone(),
                state: Some(Self::map_github_state_to_issue_state(&payload.issue.state)),
                raw: Some(serde_json::to_value(&payload).unwrap_or_default()),
            },
        }))
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
        use hmac::{Hmac, KeyInit, Mac};
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
        use hmac::{Hmac, KeyInit, Mac};
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

    const SECRET: &str = "my-secret";

    fn signed_headers(body: &[u8], event: Option<&str>) -> HeaderMap {
        use hmac::{Hmac, KeyInit, Mac};
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(SECRET.as_bytes()).unwrap();
        mac.update(body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        let mut headers = HeaderMap::new();
        headers.insert(SIGNATURE_HEADER, signature.parse().unwrap());
        if let Some(event) = event {
            headers.insert(EVENT_HEADER, event.parse().unwrap());
        }
        headers
    }

    fn parse_signed(event: Option<&str>, body: &serde_json::Value) -> SyncResult<Delivery> {
        let body = serde_json::to_vec(body).unwrap();
        GitHubSyncProvider::new().parse_webhook(&signed_headers(&body, event), &body, SECRET)
    }

    fn ping() -> serde_json::Value {
        serde_json::json!({ "zen": "Keep it logically awesome.", "hook_id": 1 })
    }

    fn parse_signed_action(action: &str) -> SyncEventType {
        let body = serde_json::json!({
            "action": action,
            "issue": {
                "number": 123,
                "html_url": "https://github.com/owner/repo/issues/123",
                "state": "open",
                "title": "Title",
                "body": null
            }
        });

        match parse_signed(Some(ISSUES_EVENT), &body).expect("a signed webhook parses") {
            Delivery::Issue(event) => event.event_type,
            Delivery::Ignored(reason) => panic!("an issues delivery was ignored: {reason:?}"),
        }
    }

    #[test]
    fn a_signed_ping_is_acknowledged_as_a_ping() {
        let delivery = parse_signed(Some(PING_EVENT), &ping()).expect("a signed ping parses");

        assert!(matches!(delivery, Delivery::Ignored(IgnoredDelivery::Ping)));
    }

    #[test]
    fn an_unsigned_ping_fails_verification() {
        let body = serde_json::to_vec(&ping()).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(EVENT_HEADER, PING_EVENT.parse().unwrap());

        let result = GitHubSyncProvider::new().parse_webhook(&headers, &body, SECRET);

        assert!(matches!(
            result,
            Err(SyncError::WebhookVerificationFailed(_))
        ));
    }

    #[test]
    fn a_signed_event_other_than_issues_is_ignored_as_not_an_issue() {
        let body = serde_json::json!({ "action": "opened", "pull_request": { "number": 7 } });

        let delivery = parse_signed(Some("pull_request"), &body).expect("a signed event parses");

        assert!(matches!(
            delivery,
            Delivery::Ignored(IgnoredDelivery::NotAnIssue(event)) if event == "pull_request"
        ));
    }

    #[test]
    fn a_signed_delivery_without_an_event_header_is_an_invalid_payload() {
        let result = parse_signed(None, &ping());

        assert!(matches!(result, Err(SyncError::InvalidWebhookPayload(_))));
    }

    #[test]
    fn webhook_actions_map_to_sync_event_types() {
        assert_eq!(parse_signed_action("opened"), SyncEventType::Create);
        assert_eq!(parse_signed_action("closed"), SyncEventType::Close);
        assert_eq!(parse_signed_action("deleted"), SyncEventType::Close);
        for action in ["edited", "reopened", "labeled", "assigned"] {
            assert_eq!(
                parse_signed_action(action),
                SyncEventType::Update,
                "{action}"
            );
        }
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
        let body = b"not valid json";
        let headers = signed_headers(body, Some(ISSUES_EVENT));

        let result = GitHubSyncProvider::new().parse_webhook(&headers, body, SECRET);

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
