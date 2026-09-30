//! Linear synchronization provider

use async_trait::async_trait;
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;

use super::{
    Delivery, ExternalIssue, IgnoredDelivery, IssueOrigin, IssueState, Provider, StateChange,
    SyncConfig, SyncError, SyncProvider, SyncResult, WebhookEvent, WebhookPayload,
};
use crate::db::sync_config::SyncEventType;
use crate::db::tasks::TaskRow;

const SIGNATURE_HEADER: &str = "Linear-Signature";
const DELIVERY_HEADER: &str = "Linear-Delivery";
const ISSUE_TYPE: &str = "Issue";
const TIMESTAMP_TOLERANCE_MILLISECONDS: u64 = 60_000;

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
    action: Action,
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    data: serde_json::Value,
    #[serde(rename = "webhookTimestamp")]
    webhook_timestamp: Option<i64>,
}

/// What happened to an entity, as a delivery's `action` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Create,
    Remove,
    #[serde(other)]
    Other,
}

impl Action {
    fn event_type(self) -> SyncEventType {
        match self {
            Self::Create => SyncEventType::Create,
            Self::Remove => SyncEventType::Close,
            Self::Other => SyncEventType::Update,
        }
    }
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
        use hmac::{Hmac, KeyInit, Mac};
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

    fn verify_timestamp(sent_at: Option<i64>, now: i64) -> SyncResult<()> {
        let sent_at = sent_at.ok_or_else(|| {
            SyncError::WebhookVerificationFailed("Missing webhookTimestamp".to_string())
        })?;
        if now.abs_diff(sent_at) > TIMESTAMP_TOLERANCE_MILLISECONDS {
            return Err(SyncError::WebhookVerificationFailed(format!(
                "webhookTimestamp {sent_at} is more than {TIMESTAMP_TOLERANCE_MILLISECONDS} ms from now"
            )));
        }
        Ok(())
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
        Provider::Linear.as_str()
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

        let payload: LinearWebhookPayload = serde_json::from_slice(body).map_err(|e| {
            SyncError::InvalidWebhookPayload(format!("Failed to parse JSON: {}", e))
        })?;

        Self::verify_timestamp(payload.webhook_timestamp, Utc::now().timestamp_millis())?;

        if payload.event_type != ISSUE_TYPE {
            return Ok(Delivery::Ignored(IgnoredDelivery::NotAnIssue(
                payload.event_type,
            )));
        }

        let issue_id = payload
            .data
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| SyncError::InvalidWebhookPayload("Missing issue ID".to_string()))?;

        let text = |name: &str| {
            payload
                .data
                .get(name)
                .and_then(|value| value.as_str())
                .map(str::to_string)
        };
        let title = text("title");
        let description = text("description");
        let url = text("url");
        let project_id = text("projectId");
        let updated_at = text("updatedAt").and_then(|time| time.parse::<DateTime<Utc>>().ok());
        let delivery_id = headers
            .get(DELIVERY_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        // Parse state
        let state = payload
            .data
            .get("state")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str())
            .map(Self::map_linear_state_to_issue_state);

        Ok(Delivery::Issue(WebhookEvent {
            event_type: payload.action.event_type(),
            external_id: issue_id.to_string(),
            url,
            origin: IssueOrigin::Linear { project_id },
            delivery_id,
            created_at: None,
            state_change: StateChange::Reported,
            payload: WebhookPayload {
                title,
                description,
                state,
                updated_at,
                raw: Some(payload.data),
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
        let provider = LinearSyncProvider::new();
        assert_eq!(provider.provider_name(), "linear");
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
        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(b"wrong-secret").unwrap();
        mac.update(body);
        let result = mac.finalize();
        let sig = hex::encode(result.into_bytes());

        assert!(!LinearSyncProvider::verify_signature(secret, body, &sig));
    }

    const SECRET: &str = "my-secret";

    fn parse_signed(body: &serde_json::Value) -> SyncResult<Delivery> {
        use hmac::{Hmac, KeyInit, Mac};
        type HmacSha256 = Hmac<Sha256>;

        let body = serde_json::to_vec(body).unwrap();
        let mut mac = HmacSha256::new_from_slice(SECRET.as_bytes()).unwrap();
        mac.update(&body);
        let mut headers = HeaderMap::new();
        headers.insert(
            SIGNATURE_HEADER,
            hex::encode(mac.finalize().into_bytes()).parse().unwrap(),
        );

        LinearSyncProvider::new().parse_webhook(&headers, &body, SECRET)
    }

    fn delivery(entity: &str, action: &str) -> serde_json::Value {
        serde_json::json!({
            "action": action,
            "type": entity,
            "data": { "id": "issue-123", "title": "Title" },
            "webhookTimestamp": Utc::now().timestamp_millis()
        })
    }

    fn parse_signed_action(action: &str) -> SyncEventType {
        match parse_signed(&delivery(ISSUE_TYPE, action)).expect("a signed webhook parses") {
            Delivery::Issue(event) => event.event_type,
            Delivery::Ignored(reason) => panic!("an issue delivery was ignored: {reason:?}"),
        }
    }

    #[test]
    fn a_signed_delivery_for_another_entity_is_ignored_as_not_an_issue() {
        for entity in ["Comment", "Project"] {
            let delivery = parse_signed(&delivery(entity, "update")).expect("it parses");

            assert!(
                matches!(
                    &delivery,
                    Delivery::Ignored(IgnoredDelivery::NotAnIssue(event)) if event == entity
                ),
                "{entity}: {delivery:?}"
            );
        }
    }

    #[test]
    fn a_signed_delivery_sent_over_a_minute_ago_fails_verification() {
        let mut body = delivery(ISSUE_TYPE, "update");
        body["webhookTimestamp"] = serde_json::json!(Utc::now().timestamp_millis() - 61_000);

        let result = parse_signed(&body);

        assert!(matches!(
            result,
            Err(SyncError::WebhookVerificationFailed(_))
        ));
    }

    #[test]
    fn a_signed_delivery_without_a_timestamp_fails_verification() {
        let mut body = delivery(ISSUE_TYPE, "update");
        body.as_object_mut().unwrap().remove("webhookTimestamp");

        let result = parse_signed(&body);

        assert!(matches!(
            result,
            Err(SyncError::WebhookVerificationFailed(_))
        ));
    }

    #[test]
    fn a_timestamp_within_a_minute_either_side_of_now_is_fresh() {
        let now = 1_700_000_000_000;

        assert!(LinearSyncProvider::verify_timestamp(Some(now - 60_000), now).is_ok());
        assert!(LinearSyncProvider::verify_timestamp(Some(now + 60_000), now).is_ok());
        assert!(LinearSyncProvider::verify_timestamp(Some(now - 60_001), now).is_err());
        assert!(LinearSyncProvider::verify_timestamp(Some(now + 60_001), now).is_err());
    }

    #[test]
    fn webhook_actions_map_to_sync_event_types() {
        assert_eq!(parse_signed_action("create"), SyncEventType::Create);
        assert_eq!(parse_signed_action("remove"), SyncEventType::Close);
        for action in ["update", "restore"] {
            assert_eq!(
                parse_signed_action(action),
                SyncEventType::Update,
                "{action}"
            );
        }
    }

    #[test]
    fn an_issue_delivery_carries_the_issue_url_and_project() {
        let mut body = delivery(ISSUE_TYPE, "create");
        body["data"]["url"] = serde_json::json!("https://linear.app/acme/issue/ACME-1/title");
        body["data"]["projectId"] = serde_json::json!("project-1");

        let Delivery::Issue(event) = parse_signed(&body).unwrap() else {
            panic!("an issue delivery is an issue");
        };

        assert_eq!(
            event.url.as_deref(),
            Some("https://linear.app/acme/issue/ACME-1/title")
        );
        assert_eq!(
            event.origin,
            IssueOrigin::Linear {
                project_id: Some("project-1".to_string())
            }
        );
    }

    #[test]
    fn an_issue_delivery_reports_its_state_delivery_id_and_when_the_issue_last_changed() {
        use hmac::{Hmac, KeyInit, Mac};
        type HmacSha256 = Hmac<Sha256>;

        let mut body = delivery(ISSUE_TYPE, "update");
        body["data"]["updatedAt"] = serde_json::json!("2026-01-01T00:00:10.500Z");
        let bytes = serde_json::to_vec(&body).unwrap();
        let mut mac = HmacSha256::new_from_slice(SECRET.as_bytes()).unwrap();
        mac.update(&bytes);
        let mut headers = HeaderMap::new();
        headers.insert(
            SIGNATURE_HEADER,
            hex::encode(mac.finalize().into_bytes()).parse().unwrap(),
        );
        headers.insert(DELIVERY_HEADER, "delivery-1".parse().unwrap());

        let Delivery::Issue(event) = LinearSyncProvider::new()
            .parse_webhook(&headers, &bytes, SECRET)
            .unwrap()
        else {
            panic!("an issue delivery is an issue");
        };

        assert_eq!(event.state_change, StateChange::Reported);
        assert_eq!(event.delivery_id.as_deref(), Some("delivery-1"));
        assert_eq!(
            event.payload.updated_at,
            Some("2026-01-01T00:00:10.500Z".parse().unwrap())
        );
    }

    #[test]
    fn an_issue_delivery_outside_any_project_has_none() {
        let Delivery::Issue(event) = parse_signed(&delivery(ISSUE_TYPE, "create")).unwrap() else {
            panic!("an issue delivery is an issue");
        };

        assert_eq!(event.url, None);
        assert_eq!(event.origin, IssueOrigin::Linear { project_id: None });
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
