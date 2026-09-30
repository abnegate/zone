//! GitHub Issues synchronization provider

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

const SIGNATURE_HEADER: &str = "X-Hub-Signature-256";
const EVENT_HEADER: &str = "X-GitHub-Event";
const DELIVERY_HEADER: &str = "X-GitHub-Delivery";
const ISSUES_EVENT: &str = "issues";
const PING_EVENT: &str = "ping";
const COMMENT_FIELD: &str = "comment";

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
    #[serde(default)]
    author_association: AuthorAssociation,
    #[serde(default)]
    updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRepository {
    full_name: String,
}

/// GitHub webhook event
#[derive(Debug, Clone, Deserialize)]
struct GitHubWebhookPayload {
    action: Action,
    issue: GitHubIssue,
    #[serde(default)]
    repository: Option<GitHubRepository>,
}

/// What happened to an issue, as an `issues` delivery's `action` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Opened,
    Closed,
    Reopened,
    Deleted,
    #[serde(other)]
    Other,
}

impl Action {
    fn event_type(self) -> SyncEventType {
        match self {
            Self::Opened => SyncEventType::Create,
            Self::Closed => SyncEventType::Close,
            Self::Deleted => SyncEventType::Unlink,
            Self::Reopened | Self::Other => SyncEventType::Update,
        }
    }

    fn state_change(self) -> StateChange {
        match self {
            Self::Closed => StateChange::MovedTo(IssueState::Closed),
            Self::Reopened => StateChange::MovedTo(IssueState::Open),
            Self::Opened | Self::Deleted | Self::Other => StateChange::Kept,
        }
    }
}

/// How an issue's author is related to its repository, as GitHub reports it
/// in `issue.author_association`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthorAssociation {
    Owner,
    Member,
    Collaborator,
    Contributor,
    FirstTimer,
    FirstTimeContributor,
    Mannequin,
    #[default]
    None,
    #[serde(other)]
    Unknown,
}

impl AuthorAssociation {
    /// Whether the author has write access to the repository: its owner, a
    /// member of the organization that owns it, or a collaborator.
    pub fn can_write(self) -> bool {
        matches!(self, Self::Owner | Self::Member | Self::Collaborator)
    }
}

/// The lowercase `owner/name` a repository URL points at, ignoring a trailing
/// slash or `.git`; `None` when it names no owner and repository.
pub fn repository_name(url: &str) -> Option<String> {
    let url = url.trim().to_lowercase();
    let url = url.trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let mut segments = url.rsplit(['/', ':']).filter(|segment| !segment.is_empty());
    let name = segments.next()?;
    let owner = segments.next()?;
    Some(format!("{owner}/{name}"))
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
}

impl Default for GitHubSyncProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SyncProvider for GitHubSyncProvider {
    fn provider_name(&self) -> &str {
        Provider::GitHub.as_str()
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

        let raw: serde_json::Value = serde_json::from_slice(body).map_err(|error| {
            SyncError::InvalidWebhookPayload(format!("Failed to parse JSON: {error}"))
        })?;
        if raw.get(COMMENT_FIELD).is_some() {
            return Err(SyncError::InvalidWebhookPayload(format!(
                "An {ISSUES_EVENT} delivery carries a {COMMENT_FIELD}, so its body is from another event"
            )));
        }
        let payload: GitHubWebhookPayload =
            serde_json::from_value(raw.clone()).map_err(|error| {
                SyncError::InvalidWebhookPayload(format!("Failed to parse JSON: {error}"))
            })?;
        let delivery_id = headers
            .get(DELIVERY_HEADER)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        Ok(Delivery::Issue(WebhookEvent {
            event_type: payload.action.event_type(),
            external_id: payload.issue.number.to_string(),
            url: Some(payload.issue.html_url.clone()),
            origin: IssueOrigin::GitHub {
                repository: payload
                    .repository
                    .as_ref()
                    .map(|repository| repository.full_name.clone()),
                author_association: payload.issue.author_association,
            },
            delivery_id,
            created_at: payload.issue.created_at,
            state_change: payload.action.state_change(),
            payload: WebhookPayload {
                title: Some(payload.issue.title),
                description: payload.issue.body,
                state: Some(Self::map_github_state_to_issue_state(&payload.issue.state)),
                updated_at: payload.issue.updated_at,
                raw: Some(raw),
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::crypto::generate_token;
    use std::sync::LazyLock;
    use uuid::Uuid;

    #[test]
    fn test_provider_name() {
        let provider = GitHubSyncProvider::new();
        assert_eq!(provider.provider_name(), "github");
    }

    #[test]
    fn test_verify_signature_valid() {
        let secret = generate_token();
        let body = b"test payload";

        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        let sig = format!("sha256={}", hex::encode(result.into_bytes()));

        assert!(GitHubSyncProvider::verify_signature(&secret, body, &sig));
    }

    #[test]
    fn test_verify_signature_invalid() {
        let secret = generate_token();
        let body = b"test payload";
        let invalid_sig = "sha256=invalid";

        assert!(!GitHubSyncProvider::verify_signature(
            &secret,
            body,
            invalid_sig
        ));
    }

    #[test]
    fn test_verify_signature_wrong_secret() {
        let secret = generate_token();
        let body = b"test payload";

        use hmac::{Hmac, KeyInit, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(generate_token().as_bytes()).unwrap();
        mac.update(body);
        let result = mac.finalize();
        let sig = format!("sha256={}", hex::encode(result.into_bytes()));

        assert!(!GitHubSyncProvider::verify_signature(&secret, body, &sig));
    }

    #[test]
    fn test_verify_signature_missing_prefix() {
        let secret = generate_token();
        let body = b"test payload";
        let sig_no_prefix = "abcdef1234567890";

        assert!(!GitHubSyncProvider::verify_signature(
            &secret,
            body,
            sig_no_prefix
        ));
    }

    static SECRET: LazyLock<String> = LazyLock::new(generate_token);

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
        GitHubSyncProvider::new().parse_webhook(&signed_headers(&body, event), &body, &SECRET)
    }

    fn ping() -> serde_json::Value {
        serde_json::json!({ "zen": "Keep it logically awesome.", "hook_id": 1 })
    }

    fn parse_signed_issue(action: &str) -> WebhookEvent {
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
            Delivery::Issue(event) => event,
            Delivery::Ignored(reason) => panic!("an issues delivery was ignored: {reason:?}"),
        }
    }

    fn parse_signed_action(action: &str) -> SyncEventType {
        parse_signed_issue(action).event_type
    }

    #[test]
    fn only_a_close_or_a_reopen_moves_the_issue_state() {
        assert_eq!(
            parse_signed_issue("closed").state_change,
            StateChange::MovedTo(IssueState::Closed)
        );
        assert_eq!(
            parse_signed_issue("reopened").state_change,
            StateChange::MovedTo(IssueState::Open)
        );
        for action in [
            "opened",
            "edited",
            "labeled",
            "assigned",
            "deleted",
            "transferred",
        ] {
            assert_eq!(
                parse_signed_issue(action).state_change,
                StateChange::Kept,
                "{action}"
            );
        }
    }

    #[test]
    fn an_issues_delivery_carries_its_delivery_id_and_when_the_issue_last_changed() {
        let body = serde_json::json!({
            "action": "edited",
            "issue": {
                "number": 123,
                "html_url": "https://github.com/owner/repo/issues/123",
                "state": "open",
                "title": "Title",
                "body": null,
                "updated_at": "2026-01-01T00:00:10Z"
            }
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let mut headers = signed_headers(&bytes, Some(ISSUES_EVENT));
        headers.insert(DELIVERY_HEADER, "delivery-1".parse().unwrap());

        let Delivery::Issue(event) = GitHubSyncProvider::new()
            .parse_webhook(&headers, &bytes, &SECRET)
            .unwrap()
        else {
            panic!("an issues delivery is an issue");
        };

        assert_eq!(event.delivery_id.as_deref(), Some("delivery-1"));
        assert_eq!(
            event.payload.updated_at,
            Some("2026-01-01T00:00:10Z".parse().unwrap())
        );
        assert_eq!(event.payload.raw, Some(body));
        assert_eq!(parse_signed_issue("edited").delivery_id, None);
    }

    #[test]
    fn an_issues_delivery_carries_when_the_issue_was_opened() {
        let body = serde_json::json!({
            "action": "opened",
            "issue": {
                "number": 123,
                "html_url": "https://github.com/owner/repo/issues/123",
                "state": "open",
                "title": "Title",
                "body": null,
                "created_at": "2026-01-01T00:00:01Z"
            }
        });

        let Delivery::Issue(event) = parse_signed(Some(ISSUES_EVENT), &body).unwrap() else {
            panic!("an issues delivery is an issue");
        };

        assert_eq!(
            event.created_at,
            Some("2026-01-01T00:00:01Z".parse().unwrap())
        );
        assert_eq!(parse_signed_issue("opened").created_at, None);
    }

    #[test]
    fn a_signed_comment_body_sent_as_an_issues_delivery_is_an_invalid_payload() {
        let body = serde_json::json!({
            "action": "deleted",
            "issue": {
                "number": 123,
                "html_url": "https://github.com/owner/repo/issues/123",
                "state": "open",
                "title": "Title",
                "body": null
            },
            "comment": { "id": 1, "body": "Thanks" }
        });

        let result = parse_signed(Some(ISSUES_EVENT), &body);

        assert!(
            matches!(result, Err(SyncError::InvalidWebhookPayload(_))),
            "{result:?}"
        );
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

        let result = GitHubSyncProvider::new().parse_webhook(&headers, &body, &SECRET);

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
        assert_eq!(parse_signed_action("deleted"), SyncEventType::Unlink);
        for action in ["edited", "reopened", "labeled", "assigned"] {
            assert_eq!(
                parse_signed_action(action),
                SyncEventType::Update,
                "{action}"
            );
        }
    }

    #[test]
    fn an_issues_delivery_carries_the_issue_url_repository_and_author_association() {
        let body = serde_json::json!({
            "action": "opened",
            "issue": {
                "number": 456,
                "html_url": "https://github.com/acme/widgets/issues/456",
                "state": "open",
                "title": "Title",
                "body": null,
                "author_association": "COLLABORATOR"
            },
            "repository": { "full_name": "acme/widgets" }
        });

        let Delivery::Issue(event) = parse_signed(Some(ISSUES_EVENT), &body).unwrap() else {
            panic!("an issues delivery is an issue");
        };

        assert_eq!(
            event.url.as_deref(),
            Some("https://github.com/acme/widgets/issues/456")
        );
        assert_eq!(
            event.origin,
            IssueOrigin::GitHub {
                repository: Some("acme/widgets".to_string()),
                author_association: AuthorAssociation::Collaborator,
            }
        );
    }

    #[test]
    fn an_issues_delivery_without_a_repository_or_association_has_neither() {
        let body = serde_json::json!({
            "action": "opened",
            "issue": {
                "number": 456,
                "html_url": "https://github.com/acme/widgets/issues/456",
                "state": "open",
                "title": "Title",
                "body": null
            }
        });

        let Delivery::Issue(event) = parse_signed(Some(ISSUES_EVENT), &body).unwrap() else {
            panic!("an issues delivery is an issue");
        };

        assert_eq!(
            event.origin,
            IssueOrigin::GitHub {
                repository: None,
                author_association: AuthorAssociation::None,
            }
        );
    }

    #[test]
    fn only_an_owner_member_or_collaborator_can_write() {
        let parse = |value: &str| -> AuthorAssociation {
            serde_json::from_value(serde_json::json!(value)).unwrap()
        };

        for writer in ["OWNER", "MEMBER", "COLLABORATOR"] {
            assert!(parse(writer).can_write(), "{writer}");
        }
        for reader in [
            "CONTRIBUTOR",
            "FIRST_TIMER",
            "FIRST_TIME_CONTRIBUTOR",
            "MANNEQUIN",
            "NONE",
            "SOMETHING_NEW",
        ] {
            assert!(!parse(reader).can_write(), "{reader}");
        }
        assert_eq!(parse("SOMETHING_NEW"), AuthorAssociation::Unknown);
    }

    #[test]
    fn a_repository_url_names_its_lowercase_owner_and_repository() {
        for url in [
            "https://github.com/Acme/Widgets",
            "https://github.com/acme/widgets/",
            "https://github.com/acme/widgets.git",
            "https://github.com/acme/widgets.git/",
            " git@github.com:Acme/Widgets.git ",
            "acme/widgets",
        ] {
            assert_eq!(
                repository_name(url).as_deref(),
                Some("acme/widgets"),
                "{url}"
            );
        }
        for url in ["", "widgets", "/"] {
            assert_eq!(repository_name(url), None, "{url:?}");
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
        let secret = generate_token();

        let result = provider.parse_webhook(&headers, body, &secret);
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

        let result = GitHubSyncProvider::new().parse_webhook(&headers, body, &SECRET);

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
