//! Linear synchronization provider

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

/// Linear-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearConfig {
    /// Linear API key
    pub api_key: String,
    /// Team ID (for creating issues)
    pub team_id: String,
    /// Optional: Project ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Optional: Custom field mappings
    #[serde(default)]
    pub field_mappings: HashMap<String, String>,
}

/// Linear GraphQL response for issue creation
#[derive(Debug, Clone, Deserialize)]
struct LinearIssueCreateResponse {
    data: LinearIssueCreateData,
}

#[derive(Debug, Clone, Deserialize)]
struct LinearIssueCreateData {
    #[serde(rename = "issueCreate")]
    issue_create: LinearIssueCreateResult,
}

#[derive(Debug, Clone, Deserialize)]
struct LinearIssueCreateResult {
    success: bool,
    issue: Option<LinearIssue>,
}

/// Linear issue
#[derive(Debug, Clone, Deserialize)]
struct LinearIssue {
    id: String,
    identifier: String,
    url: String,
    state: LinearState,
}

/// Linear state
#[derive(Debug, Clone, Deserialize)]
struct LinearState {
    name: String,
    #[serde(rename = "type")]
    state_type: String,
}

/// Linear webhook event
#[derive(Debug, Clone, Deserialize)]
struct LinearWebhookPayload {
    action: String,
    #[serde(rename = "type")]
    event_type: String,
    data: serde_json::Value,
}

/// Linear sync provider
#[derive(Debug, Clone)]
pub struct LinearSyncProvider {
    client: reqwest::Client,
    base_url: String,
}

impl LinearSyncProvider {
    /// Create a new Linear sync provider
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://api.linear.app/graphql".to_string(),
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

    /// Parse Linear config from sync config
    fn parse_config(&self, config: &SyncConfig) -> SyncResult<LinearConfig> {
        serde_json::from_value(config.config.clone())
            .map_err(|e| SyncError::InvalidConfig(format!("Invalid Linear config: {}", e)))
    }

    /// Verify HMAC-SHA256 signature for Linear webhooks
    fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
        use hmac::{Hmac, Mac};
        use subtle::ConstantTimeEq;
        type HmacSha256 = Hmac<Sha256>;

        // Linear sends raw hex signature
        let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
            Ok(m) => m,
            Err(_) => return false,
        };

        mac.update(body);
        let result = mac.finalize();
        let computed_sig = hex::encode(result.into_bytes());

        // Constant-time comparison to prevent timing attacks
        computed_sig.as_bytes().ct_eq(signature.as_bytes()).into()
    }

    /// Map Zone task status to Linear state
    fn map_task_status_to_linear_state(status: &str) -> &str {
        match status {
            "complete" => "completed",
            "in_progress" => "started",
            "blocked" => "canceled",
            _ => "backlog",
        }
    }

    /// Map Linear state to IssueState
    fn map_linear_state_to_issue_state(state_type: &str) -> IssueState {
        match state_type {
            "completed" | "canceled" => IssueState::Closed,
            "started" => IssueState::InProgress,
            _ => IssueState::Open,
        }
    }

    /// Execute a GraphQL query
    async fn execute_graphql(
        &self,
        api_key: &str,
        query: &str,
        variables: serde_json::Value,
    ) -> SyncResult<serde_json::Value> {
        let payload = serde_json::json!({
            "query": query,
            "variables": variables,
        });

        let response = self
            .client
            .post(&self.base_url)
            .header("Authorization", api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(SyncError::ExternalApiError(format!(
                "Linear API error {}: {}",
                status, error_text
            )));
        }

        let result: serde_json::Value = response.json().await?;

        // Check for GraphQL errors
        if let Some(errors) = result.get("errors") {
            return Err(SyncError::ExternalApiError(format!(
                "GraphQL error: {}",
                errors
            )));
        }

        Ok(result)
    }
}

impl Default for LinearSyncProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SyncProvider for LinearSyncProvider {
    fn provider_name(&self) -> &str {
        "linear"
    }

    async fn create_issue(&self, config: &SyncConfig, task: &TaskRow) -> SyncResult<ExternalIssue> {
        let linear_config = self.parse_config(config)?;

        // Build issue description
        let mut description = task.description.clone();
        if let Some(ref criteria) = task.acceptance_criteria {
            description.push_str("\n\n## Acceptance Criteria\n\n");
            description.push_str(criteria);
        }

        // GraphQL mutation to create issue
        let query = r#"
            mutation IssueCreate($teamId: String!, $title: String!, $description: String, $projectId: String) {
                issueCreate(input: {
                    teamId: $teamId
                    title: $title
                    description: $description
                    projectId: $projectId
                }) {
                    success
                    issue {
                        id
                        identifier
                        url
                        state {
                            name
                            type
                        }
                    }
                }
            }
        "#;

        let mut variables = serde_json::json!({
            "teamId": linear_config.team_id,
            "title": task.title,
            "description": description,
        });

        if let Some(ref project_id) = linear_config.project_id {
            variables["projectId"] = serde_json::Value::String(project_id.clone());
        }

        let result = self
            .execute_graphql(&linear_config.api_key, query, variables)
            .await?;

        let response: LinearIssueCreateResponse = serde_json::from_value(result).map_err(|e| {
            SyncError::InvalidWebhookPayload(format!("Failed to parse response: {}", e))
        })?;

        if !response.data.issue_create.success {
            return Err(SyncError::ExternalApiError(
                "Failed to create Linear issue".to_string(),
            ));
        }

        let issue = response.data.issue_create.issue.ok_or_else(|| {
            SyncError::ExternalApiError("Issue not returned in response".to_string())
        })?;

        Ok(ExternalIssue {
            external_id: issue.id,
            url: issue.url,
            state: Self::map_linear_state_to_issue_state(&issue.state.state_type),
            metadata: HashMap::new(),
        })
    }

    async fn update_issue(
        &self,
        config: &SyncConfig,
        task: &TaskRow,
        external_id: &str,
    ) -> SyncResult<()> {
        let linear_config = self.parse_config(config)?;

        // Build issue description
        let mut description = task.description.clone();
        if let Some(ref criteria) = task.acceptance_criteria {
            description.push_str("\n\n## Acceptance Criteria\n\n");
            description.push_str(criteria);
        }

        // Determine Linear state
        let _state_name = Self::map_task_status_to_linear_state(&task.status);

        // GraphQL mutation to update issue
        let query = r#"
            mutation IssueUpdate($id: String!, $title: String, $description: String, $stateId: String) {
                issueUpdate(id: $id, input: {
                    title: $title
                    description: $description
                    stateId: $stateId
                }) {
                    success
                }
            }
        "#;

        let variables = serde_json::json!({
            "id": external_id,
            "title": task.title,
            "description": description,
            // Note: In production, we'd need to look up the actual state ID by name
            // For now, we'll just update title and description
        });

        let _result = self
            .execute_graphql(&linear_config.api_key, query, variables)
            .await?;

        Ok(())
    }

    async fn close_issue(&self, config: &SyncConfig, external_id: &str) -> SyncResult<()> {
        let linear_config = self.parse_config(config)?;

        // GraphQL mutation to archive/close issue
        let query = r#"
            mutation IssueArchive($id: String!) {
                issueArchive(id: $id) {
                    success
                }
            }
        "#;

        let variables = serde_json::json!({
            "id": external_id,
        });

        let _result = self
            .execute_graphql(&linear_config.api_key, query, variables)
            .await?;

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
            .get("Linear-Signature")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                SyncError::WebhookVerificationFailed("Missing Linear-Signature header".to_string())
            })?;

        if !Self::verify_signature(secret, body, signature) {
            return Err(SyncError::WebhookVerificationFailed(
                "Invalid signature".to_string(),
            ));
        }

        // Parse payload
        let payload: LinearWebhookPayload = serde_json::from_slice(body).map_err(|e| {
            SyncError::InvalidWebhookPayload(format!("Failed to parse JSON: {}", e))
        })?;

        // Extract issue data
        let issue_id = payload
            .data
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SyncError::InvalidWebhookPayload("Missing issue ID".to_string()))?;

        let title = payload
            .data
            .get("title")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let description = payload
            .data
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Parse state
        let state = payload
            .data
            .get("state")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str())
            .map(Self::map_linear_state_to_issue_state);

        // Map event type
        let event_type = match payload.action.as_str() {
            "create" => "issue_created",
            "update" => "issue_updated",
            "remove" => "issue_closed",
            _ => &payload.action,
        };

        Ok(WebhookEvent {
            event_type: event_type.to_string(),
            external_id: issue_id.to_string(),
            payload: WebhookPayload {
                title,
                description,
                state,
                raw: Some(payload.data),
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
        let provider = LinearSyncProvider::new();
        assert_eq!(provider.provider_name(), "linear");
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
        let sig = hex::encode(result.into_bytes());

        assert!(LinearSyncProvider::verify_signature(secret, body, &sig));
    }

    #[test]
    fn test_verify_signature_invalid() {
        let secret = "my-secret";
        let body = b"test payload";
        let invalid_sig = "invalid";

        assert!(!LinearSyncProvider::verify_signature(
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
        let sig = hex::encode(result.into_bytes());

        assert!(!LinearSyncProvider::verify_signature(secret, body, &sig));
    }

    #[test]
    fn test_map_task_status_to_linear_state() {
        assert_eq!(
            LinearSyncProvider::map_task_status_to_linear_state("complete"),
            "completed"
        );
        assert_eq!(
            LinearSyncProvider::map_task_status_to_linear_state("in_progress"),
            "started"
        );
        assert_eq!(
            LinearSyncProvider::map_task_status_to_linear_state("blocked"),
            "canceled"
        );
        assert_eq!(
            LinearSyncProvider::map_task_status_to_linear_state("created"),
            "backlog"
        );
    }

    #[test]
    fn test_map_linear_state_to_issue_state() {
        assert_eq!(
            LinearSyncProvider::map_linear_state_to_issue_state("completed"),
            IssueState::Closed
        );
        assert_eq!(
            LinearSyncProvider::map_linear_state_to_issue_state("canceled"),
            IssueState::Closed
        );
        assert_eq!(
            LinearSyncProvider::map_linear_state_to_issue_state("started"),
            IssueState::InProgress
        );
        assert_eq!(
            LinearSyncProvider::map_linear_state_to_issue_state("backlog"),
            IssueState::Open
        );
    }

    #[test]
    fn test_parse_webhook_missing_signature() {
        let provider = LinearSyncProvider::new();
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
    fn test_parse_config_valid() {
        let provider = LinearSyncProvider::new();
        let config = SyncConfig {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            provider: "linear".to_string(),
            enabled: true,
            config: serde_json::json!({
                "api_key": "lin_api_test123",
                "team_id": "TEAM-123",
                "project_id": "PROJ-456",
            }),
            webhook_secret_encrypted: None,
        };

        let linear_config = provider.parse_config(&config).unwrap();
        assert_eq!(linear_config.api_key, "lin_api_test123");
        assert_eq!(linear_config.team_id, "TEAM-123");
        assert_eq!(linear_config.project_id, Some("PROJ-456".to_string()));
    }

    #[test]
    fn test_parse_config_invalid() {
        let provider = LinearSyncProvider::new();
        let config = SyncConfig {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            provider: "linear".to_string(),
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
