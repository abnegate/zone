//! GitHub Issues synchronization provider

use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_vcs::IssueReference;
use abnegate_vcs::IssueState as GitHubState;
use abnegate_vcs::IssueUpdate;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::PullRequestService;
use abnegate_vcs::Repository;
use abnegate_vcs::WebhookSignature;
use async_trait::async_trait;
use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use super::{
    Delivery, ExternalIssue, IgnoredDelivery, IssueOrigin, IssueState, Provider, StateChange,
    SyncConfig, SyncError, SyncProvider, SyncResult, WebhookEvent, WebhookPayload,
};
use crate::db::sync_config::SyncEventType;
use crate::db::tasks::TaskRow;
use crate::secret::Unseal;

const TOKEN_UNOPENABLE: &str =
    "the GitHub token is sealed under a key this deployment does not hold";
const CLIENT_UNAVAILABLE: &str = "a client for GitHub's own API";
const GITHUB_REFUSED: &str =
    "GitHub API error: GitHub refused the request or sent an answer that could not be read";
const ISSUE_NUMBER_INVALID: &str = "the linked GitHub issue ID is not an issue number";
const INVALID_SIGNATURE: &str = "Invalid signature";
const CRITERIA_HEADING: &str = "\n\n## Acceptance Criteria\n\n";
const CLOSING_STATUSES: [&str; 2] = ["complete", "blocked"];
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
    pub token: SecretValue,
    /// Optional: Custom field mappings
    #[serde(default)]
    pub field_mappings: HashMap<String, String>,
}

impl GitHubConfig {
    /// The repository its owner and name address, refused when GitHub would not accept either
    fn repository(&self) -> SyncResult<Repository> {
        Repository::new(&self.owner, &self.repo)
            .map_err(|error| SyncError::InvalidConfig(format!("Invalid GitHub config: {error}")))
    }
}

/// GitHub issue response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GitHubIssue {
    number: i64,
    html_url: String,
    state: GitHubState,
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
    service: PullRequestService,
    master_key: Arc<MasterKey>,
}

impl GitHubSyncProvider {
    /// Create a new GitHub sync provider that opens stored tokens under `master_key`
    pub fn new(master_key: Arc<MasterKey>) -> Self {
        Self {
            service: PullRequestService::new().expect(CLIENT_UNAVAILABLE),
            master_key,
        }
    }

    /// Create with custom base URL (for testing)
    #[cfg(test)]
    pub fn with_base_url(base_url: String, master_key: Arc<MasterKey>) -> Self {
        const GITHUB_HOST: &str = "github.com";
        Self {
            service: PullRequestService::standing_in_for(GITHUB_HOST, &base_url)
                .expect("a stand-in for GitHub's API"),
            master_key,
        }
    }

    /// Parse GitHub config from sync config, opening its token when it is sealed
    fn parse_config(&self, config: &SyncConfig) -> SyncResult<GitHubConfig> {
        let mut parsed: GitHubConfig = serde_json::from_value(config.config.clone())
            .map_err(|error| SyncError::InvalidConfig(format!("Invalid GitHub config: {error}")))?;
        parsed.token = self
            .master_key
            .open_lenient(parsed.token.expose())
            .map_err(|_| SyncError::InvalidConfig(TOKEN_UNOPENABLE.to_string()))?;
        Ok(parsed)
    }

    /// The issue `external_id` numbers in `repository`, refused before anything is sent when
    /// it is not a positive number
    fn issue(repository: Repository, external_id: &str) -> SyncResult<IssueReference> {
        external_id
            .parse::<NonZeroU64>()
            .map(|number| IssueReference::new(repository, number))
            .map_err(|_| SyncError::ExternalApiError(ISSUE_NUMBER_INVALID.to_string()))
    }

    /// An issue's body: the task's description, followed by its acceptance criteria if any
    fn body(task: &TaskRow) -> String {
        match &task.acceptance_criteria {
            Some(criteria) => format!("{}{CRITERIA_HEADING}{criteria}", task.description),
            None => task.description.clone(),
        }
    }

    /// The sync error for what GitHub answered, never repeating GitHub's own words
    fn refused(error: PullRequestError) -> SyncError {
        match error {
            PullRequestError::Http(error) => SyncError::HttpError(error),
            PullRequestError::GitHubApi(_) => {
                SyncError::ExternalApiError(GITHUB_REFUSED.to_string())
            }
            PullRequestError::Refused { status, .. } => {
                SyncError::ExternalApiError(format!("GitHub API error {status}"))
            }
            other => SyncError::ExternalApiError(other.to_string()),
        }
    }

    /// Map Zone task status to GitHub issue state
    fn map_task_status_to_github_state(status: &str) -> GitHubState {
        if CLOSING_STATUSES.contains(&status) {
            GitHubState::Closed
        } else {
            GitHubState::Open
        }
    }

    /// Map GitHub issue state to Zone IssueState
    fn map_github_state_to_issue_state(state: GitHubState) -> IssueState {
        match state {
            GitHubState::Closed => IssueState::Closed,
            _ => IssueState::Open,
        }
    }
}

#[async_trait]
impl SyncProvider for GitHubSyncProvider {
    fn provider_name(&self) -> &str {
        Provider::GitHub.as_str()
    }

    async fn create_issue(&self, config: &SyncConfig, task: &TaskRow) -> SyncResult<ExternalIssue> {
        let github = self.parse_config(config)?;
        let issue = self
            .service
            .create_issue(
                &github.repository()?,
                &github.token,
                &task.title,
                &Self::body(task),
            )
            .await
            .map_err(Self::refused)?;

        Ok(ExternalIssue {
            external_id: issue.reference.number().to_string(),
            url: issue.url,
            state: Self::map_github_state_to_issue_state(issue.state),
            metadata: HashMap::new(),
        })
    }

    async fn update_issue(
        &self,
        config: &SyncConfig,
        task: &TaskRow,
        external_id: &str,
    ) -> SyncResult<()> {
        let github = self.parse_config(config)?;
        let reference = Self::issue(github.repository()?, external_id)?;
        let update = IssueUpdate::default()
            .with_title(task.title.as_str())
            .with_body(Self::body(task))
            .with_state(Self::map_task_status_to_github_state(&task.status));

        self.service
            .update_issue(&reference, &github.token, &update)
            .await
            .map_err(Self::refused)
    }

    async fn close_issue(&self, config: &SyncConfig, external_id: &str) -> SyncResult<()> {
        let github = self.parse_config(config)?;
        let reference = Self::issue(github.repository()?, external_id)?;

        self.service
            .close_issue(&reference, &github.token)
            .await
            .map_err(Self::refused)
    }

    fn parse_webhook(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        secret: &str,
    ) -> SyncResult<Delivery> {
        let signature = headers
            .get(WebhookSignature::HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                SyncError::WebhookVerificationFailed(format!(
                    "Missing {} header",
                    WebhookSignature::HEADER
                ))
            })?;

        if !WebhookSignature::verify(signature, &SecretValue::new(secret), body) {
            return Err(SyncError::WebhookVerificationFailed(
                INVALID_SIGNATURE.to_string(),
            ));
        }

        let event = headers
            .get(EVENT_HEADER)
            .and_then(|value| value.to_str().ok())
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
                state: Some(Self::map_github_state_to_issue_state(payload.issue.state)),
                updated_at: payload.issue.updated_at,
                raw: Some(raw),
            },
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::fixture;
    use crate::utils::crypto::generate_token;
    use hmac::Hmac;
    use hmac::KeyInit;
    use hmac::Mac;
    use serde_json::json;
    use sha2::Sha256;
    use std::net::TcpListener;
    use std::sync::LazyLock;
    use uuid::Uuid;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::any;
    use wiremock::matchers::body_json;
    use wiremock::matchers::header;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    const TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
    const OWNER: &str = "test-owner";
    const REPOSITORY: &str = "test-repo";
    const ISSUE: &str = "42";
    const ISSUE_URL: &str = "https://github.com/test-owner/test-repo/issues/42";
    const TITLE: &str = "Add a cart";
    const DESCRIPTION: &str = "Shoppers need a cart.";
    const CRITERIA: &str = "- A shopper can add an item";
    const LEAKED: &str = "test-secret";
    const PREFIX: &str = "sha256=";

    fn task(criteria: Option<&str>, status: &str) -> TaskRow {
        TaskRow {
            id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            created_by: None,
            project_ids: Vec::new(),
            title: TITLE.to_string(),
            title_pending: false,
            description: DESCRIPTION.to_string(),
            acceptance_criteria: criteria.map(str::to_string),
            status: status.to_string(),
            priority: None,
            model_name: None,
            dependencies: None,
            is_agentic: false,
            require_plan_approval: false,
            github_repo_url: None,
            source_id: None,
            source_ids: None,
            worker_id: None,
            queued_at: None,
            started_at: None,
            completed_at: None,
            created_at: None,
            updated_at: None,
            pr_url: None,
            branch_name: None,
            pr_status: None,
            pr_created_at: None,
        }
    }

    fn created_issue() -> serde_json::Value {
        json!({
            "number": 42,
            "title": TITLE,
            "body": DESCRIPTION,
            "state": "open",
            "html_url": ISSUE_URL,
            "labels": [],
            "comments": 0,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        })
    }

    fn issue_path() -> String {
        format!("/repos/{OWNER}/{REPOSITORY}/issues/{ISSUE}")
    }

    fn stand_in(server: &MockServer) -> GitHubSyncProvider {
        GitHubSyncProvider::with_base_url(server.uri(), key())
    }

    fn key() -> Arc<MasterKey> {
        Arc::new(MasterKey::generate().expect("a key"))
    }

    fn sync_config(token: &str) -> SyncConfig {
        SyncConfig {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            provider: Provider::GitHub.as_str().to_string(),
            enabled: true,
            config: serde_json::json!({
                "owner": OWNER,
                "repo": REPOSITORY,
                "token": token,
            }),
            webhook_secret_encrypted: None,
        }
    }

    #[test]
    fn a_plaintext_sealed_or_legacy_token_parses_to_the_same_token() {
        let key = key();
        let provider = GitHubSyncProvider::new(Arc::clone(&key));

        for stored in [
            TOKEN.to_string(),
            fixture::seal(TOKEN, &key),
            fixture::seal_legacy(TOKEN, &key),
        ] {
            let parsed = provider.parse_config(&sync_config(&stored)).unwrap();
            assert_eq!(
                parsed.token.expose(),
                TOKEN,
                "{stored} did not parse to the token"
            );
        }
    }

    #[test]
    fn a_token_sealed_under_another_key_is_refused_before_any_request() {
        let provider = GitHubSyncProvider::new(key());
        let foreign = fixture::seal(TOKEN, &MasterKey::generate().expect("a key"));

        assert!(matches!(
            provider.parse_config(&sync_config(&foreign)),
            Err(SyncError::InvalidConfig(_))
        ));
    }

    #[tokio::test]
    async fn a_sealed_token_authorizes_the_same_as_its_plaintext() {
        let key = key();
        for stored in [TOKEN.to_string(), fixture::seal(TOKEN, &key)] {
            let server = MockServer::start().await;
            Mock::given(method("PATCH"))
                .and(path(format!("/repos/{OWNER}/{REPOSITORY}/issues/{ISSUE}")))
                .and(header("Authorization", format!("Bearer {TOKEN}").as_str()))
                .respond_with(ResponseTemplate::new(200))
                .expect(1)
                .mount(&server)
                .await;

            GitHubSyncProvider::with_base_url(server.uri(), Arc::clone(&key))
                .close_issue(&sync_config(&stored), ISSUE)
                .await
                .unwrap_or_else(|error| panic!("{stored} did not authorize: {error}"));
            server.verify().await;
        }
    }

    #[test]
    fn test_provider_name() {
        let provider = GitHubSyncProvider::new(key());
        assert_eq!(provider.provider_name(), "github");
    }

    static SECRET: LazyLock<String> = LazyLock::new(generate_token);

    fn digest(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    fn signature(secret: &str, body: &[u8]) -> String {
        format!("{PREFIX}{}", digest(secret, body))
    }

    fn headers_signed(signature: &str, event: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(WebhookSignature::HEADER, signature.parse().unwrap());
        if let Some(event) = event {
            headers.insert(EVENT_HEADER, event.parse().unwrap());
        }
        headers
    }

    fn signed_headers(body: &[u8], event: Option<&str>) -> HeaderMap {
        headers_signed(&signature(&SECRET, body), event)
    }

    fn parse_signed(event: Option<&str>, body: &serde_json::Value) -> SyncResult<Delivery> {
        let body = serde_json::to_vec(body).unwrap();
        GitHubSyncProvider::new(key()).parse_webhook(&signed_headers(&body, event), &body, &SECRET)
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

        let Delivery::Issue(event) = GitHubSyncProvider::new(key())
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

        let result = GitHubSyncProvider::new(key()).parse_webhook(&headers, &body, &SECRET);

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
            GitHubState::Closed
        );
        assert_eq!(
            GitHubSyncProvider::map_task_status_to_github_state("blocked"),
            GitHubState::Closed
        );
        assert_eq!(
            GitHubSyncProvider::map_task_status_to_github_state("created"),
            GitHubState::Open
        );
        assert_eq!(
            GitHubSyncProvider::map_task_status_to_github_state("in_progress"),
            GitHubState::Open
        );
    }

    #[test]
    fn test_map_github_state_to_issue_state() {
        assert_eq!(
            GitHubSyncProvider::map_github_state_to_issue_state(GitHubState::Open),
            IssueState::Open
        );
        assert_eq!(
            GitHubSyncProvider::map_github_state_to_issue_state(GitHubState::Closed),
            IssueState::Closed
        );
        assert_eq!(
            GitHubSyncProvider::map_github_state_to_issue_state(GitHubState::Unknown),
            IssueState::Open
        );
    }

    #[test]
    fn test_parse_webhook_missing_signature() {
        let provider = GitHubSyncProvider::new(key());
        let headers = HeaderMap::new();
        let body = b"{}";
        let secret = generate_token();

        let result = provider.parse_webhook(&headers, body, &secret);
        assert!(matches!(
            result,
            Err(SyncError::WebhookVerificationFailed(message))
                if message == "Missing X-Hub-Signature-256 header"
        ));
    }

    #[test]
    fn test_parse_webhook_invalid_json() {
        let body = b"not valid json";
        let headers = signed_headers(body, Some(ISSUES_EVENT));

        let result = GitHubSyncProvider::new(key()).parse_webhook(&headers, body, &SECRET);

        assert!(matches!(result, Err(SyncError::InvalidWebhookPayload(_))));
    }

    #[test]
    fn test_parse_config_valid() {
        let provider = GitHubSyncProvider::new(key());
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
        assert_eq!(gh_config.token.expose(), "ghp_test123");
    }

    #[test]
    fn test_parse_config_invalid() {
        let provider = GitHubSyncProvider::new(key());
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

    #[test]
    fn existing_sync_json_deserialises_with_a_secret_token() {
        let stored = json!({
            "owner": OWNER,
            "repo": REPOSITORY,
            "token": TOKEN,
            "field_mappings": { "priority": "labels" }
        });

        let config: GitHubConfig = serde_json::from_value(stored.clone()).unwrap();

        assert_eq!(config.token.expose(), TOKEN);
        assert_eq!(
            serde_json::to_value(&config).unwrap(),
            stored,
            "a config writes back the shape it was read from"
        );
    }

    #[test]
    fn a_github_config_never_prints_its_token() {
        let config = GitHubSyncProvider::new(key())
            .parse_config(&sync_config(TOKEN))
            .unwrap();

        assert!(!format!("{config:?}").contains(TOKEN));
    }

    #[tokio::test]
    async fn an_invalid_owner_or_repository_is_refused_before_any_request() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(201).set_body_json(created_issue()))
            .expect(0)
            .mount(&server)
            .await;
        let mut config = sync_config(TOKEN);
        config.config = json!({ "owner": "test owner", "repo": "..", "token": TOKEN });

        let result = stand_in(&server)
            .create_issue(&config, &task(None, "created"))
            .await;

        assert!(
            matches!(result, Err(SyncError::InvalidConfig(_))),
            "{result:?}"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn create_posts_the_title_and_the_body_with_its_acceptance_criteria() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/repos/{OWNER}/{REPOSITORY}/issues")))
            .and(header("Authorization", format!("Bearer {TOKEN}").as_str()))
            .and(body_json(json!({
                "title": TITLE,
                "body": format!("{DESCRIPTION}\n\n## Acceptance Criteria\n\n{CRITERIA}"),
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(created_issue()))
            .expect(1)
            .mount(&server)
            .await;

        let issue = stand_in(&server)
            .create_issue(&sync_config(TOKEN), &task(Some(CRITERIA), "created"))
            .await
            .unwrap();

        assert_eq!(issue.external_id, ISSUE);
        assert_eq!(issue.url, ISSUE_URL);
        assert_eq!(issue.state, IssueState::Open);
        server.verify().await;
    }

    #[tokio::test]
    async fn update_sends_title_body_and_state() {
        for (status, state) in [("in_progress", "open"), ("complete", "closed")] {
            let server = MockServer::start().await;
            Mock::given(method("PATCH"))
                .and(path(issue_path()))
                .and(header("Authorization", format!("Bearer {TOKEN}").as_str()))
                .and(body_json(json!({
                    "title": TITLE,
                    "body": DESCRIPTION,
                    "state": state,
                })))
                .respond_with(ResponseTemplate::new(200))
                .expect(1)
                .mount(&server)
                .await;

            stand_in(&server)
                .update_issue(&sync_config(TOKEN), &task(None, status), ISSUE)
                .await
                .unwrap_or_else(|error| panic!("{status} did not update: {error}"));
            server.verify().await;
        }
    }

    #[tokio::test]
    async fn close_sends_a_closed_state() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path(issue_path()))
            .and(body_json(json!({ "state": "closed" })))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        stand_in(&server)
            .close_issue(&sync_config(TOKEN), ISSUE)
            .await
            .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn a_github_error_body_is_never_echoed() {
        for status in [201, 401, 403, 404, 422, 500] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(json!({ "message": LEAKED })),
                )
                .mount(&server)
                .await;

            let error = stand_in(&server)
                .create_issue(&sync_config(TOKEN), &task(None, "created"))
                .await
                .expect_err("GitHub's answer is refused");

            assert!(
                matches!(&error, SyncError::ExternalApiError(_)),
                "{status}: {error:?}"
            );
            assert!(!format!("{error} {error:?}").contains(LEAKED), "{status}");
        }
    }

    #[tokio::test]
    async fn an_unclassified_refusal_names_its_status_and_never_github_s_words() {
        for status in [409, 422, 500, 503] {
            let server = MockServer::start().await;
            Mock::given(method("PATCH"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(json!({ "message": LEAKED })),
                )
                .mount(&server)
                .await;

            let result = stand_in(&server)
                .close_issue(&sync_config(TOKEN), ISSUE)
                .await;

            assert!(
                matches!(
                    &result,
                    Err(SyncError::ExternalApiError(message))
                        if *message == format!("GitHub API error {status}")
                ),
                "{status}: {result:?}"
            );
            assert!(!format!("{result:?}").contains(LEAKED), "{status}");
        }
    }

    #[test]
    fn an_unreadable_answer_reads_as_one_fixed_text() {
        let error = GitHubSyncProvider::refused(PullRequestError::GitHubApi(LEAKED.to_string()));
        assert!(
            matches!(&error, SyncError::ExternalApiError(message) if message == GITHUB_REFUSED),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_transport_failure_is_an_http_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let provider = GitHubSyncProvider::with_base_url(format!("http://{address}"), key());

        let result = provider.close_issue(&sync_config(TOKEN), ISSUE).await;

        assert!(matches!(result, Err(SyncError::HttpError(_))), "{result:?}");
    }

    #[tokio::test]
    async fn a_non_numeric_external_id_is_refused_before_any_request() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let provider = stand_in(&server);
        let config = sync_config(TOKEN);

        for external_id in ["", "abc", "0", "-1", "4 2", "42/comments"] {
            for result in [
                provider
                    .update_issue(&config, &task(None, "created"), external_id)
                    .await,
                provider.close_issue(&config, external_id).await,
            ] {
                assert!(
                    matches!(&result, Err(SyncError::ExternalApiError(message)) if message == ISSUE_NUMBER_INVALID),
                    "{external_id:?}: {result:?}"
                );
            }
        }
        server.verify().await;
    }

    #[test]
    fn a_wrong_signature_is_refused_as_invalid() {
        let body = serde_json::to_vec(&ping()).unwrap();
        let provider = GitHubSyncProvider::new(key());

        for signature in [
            signature(&generate_token(), &body),
            signature(&SECRET, b"another payload"),
            digest(&SECRET, &body),
            format!("{PREFIX}invalid"),
        ] {
            let result = provider.parse_webhook(
                &headers_signed(&signature, Some(PING_EVENT)),
                &body,
                &SECRET,
            );

            assert!(
                matches!(&result, Err(SyncError::WebhookVerificationFailed(message)) if message == INVALID_SIGNATURE),
                "{signature}: {result:?}"
            );
        }
    }

    #[test]
    fn an_uppercase_hex_signature_verifies() {
        let body = serde_json::to_vec(&ping()).unwrap();
        let signature = format!("{PREFIX}{}", digest(&SECRET, &body).to_uppercase());

        let delivery = GitHubSyncProvider::new(key())
            .parse_webhook(
                &headers_signed(&signature, Some(PING_EVENT)),
                &body,
                &SECRET,
            )
            .expect("an uppercase digest verifies");

        assert!(matches!(delivery, Delivery::Ignored(IgnoredDelivery::Ping)));
    }

    #[test]
    fn an_empty_webhook_secret_refuses_every_delivery() {
        let body = serde_json::to_vec(&ping()).unwrap();
        let result = GitHubSyncProvider::new(key()).parse_webhook(
            &headers_signed(&signature("", &body), Some(PING_EVENT)),
            &body,
            "",
        );

        assert!(
            matches!(&result, Err(SyncError::WebhookVerificationFailed(message)) if message == INVALID_SIGNATURE),
            "{result:?}"
        );
    }

    #[test]
    fn sync_addresses_github_com_and_nothing_else() {
        let provider = GitHubSyncProvider::new(key());

        assert!(
            provider
                .service
                .parse_github_url("https://github.com/acme/widgets")
                .is_ok()
        );
        assert!(
            provider
                .service
                .parse_github_url("https://github.example.com/acme/widgets")
                .is_err()
        );
    }
}
