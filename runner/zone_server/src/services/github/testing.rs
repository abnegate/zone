pub(crate) mod connected;

use abnegate_secret::SecretValue;
use abnegate_vcs::PullRequestService;
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::client::Github;
use super::configuration::Configuration;
use super::connection::ConnectedSource;
use crate::db::sources;
use crate::db::sources::SourceRow;

pub(crate) const OWNER: &str = "owner";
pub(crate) const REPOSITORY: &str = "repository";
pub(crate) const MINTED_TOKEN: &str = "ghs_mintedinstallationtoken";
pub(crate) const STORED_TOKEN: &str = "ghp_storedpersonaltoken";
pub(crate) const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(crate) const BLOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BEARER: &str = "Bearer test-secret";
const WEB_ORIGIN: &str = "https://github.com";
pub(crate) const ENTERPRISE_ORIGIN: &str = "https://github.example.com";
pub(crate) const RETRY_AFTER_HEADER: &str = "Retry-After";
pub(crate) const RATE_LIMIT_REMAINING_HEADER: &str = "X-RateLimit-Remaining";

/// A GitHub installation number no other test uses.
pub(crate) fn number() -> u64 {
    rand::random_range(1..i64::MAX as u64)
}

pub(crate) fn mocked(server: &MockServer) -> Github {
    github_named(server, OWNER, REPOSITORY)
}

pub(crate) fn configured(owner: &str, repository: &str) -> Configuration {
    Configuration {
        owner: owner.into(),
        repository: repository.into(),
        branch: Some("main".into()),
        path: None,
        token: Some("test-secret".into()),
        review_signals: None,
    }
}

/// A source named `name`, connected to `owner/repository`, that no database holds.
pub(crate) fn connected_source(id: Uuid, name: &str) -> ConnectedSource {
    ConnectedSource {
        source: SourceRow {
            id,
            name: name.into(),
            source_type: sources::GITHUB.into(),
            config: json!({"owner": OWNER, "repo": REPOSITORY}),
            credentials_encrypted: None,
            description: None,
            url: None,
            is_active: Some(true),
            last_verified_at: None,
            last_error: None,
            created_at: None,
            updated_at: None,
            workspace_id: None,
            github_installation_id: None,
        },
        configuration: configured(OWNER, REPOSITORY),
    }
}

pub(crate) fn github_named(server: &MockServer, owner: &str, repository: &str) -> Github {
    serving(&server.uri(), configured(owner, repository))
}

/// The client for `owner/repository` against a stand-in GitHub at `origin`.
pub(crate) fn served(origin: &str) -> Github {
    serving(origin, configured(OWNER, REPOSITORY))
}

fn serving(origin: &str, configuration: Configuration) -> Github {
    Github::serving(
        configuration,
        PullRequestService::standing_in_for("github.com", origin).unwrap(),
        WEB_ORIGIN,
    )
    .unwrap()
}

pub(crate) fn enterprise(server: &MockServer) -> Github {
    Github::serving(
        configured(OWNER, REPOSITORY),
        PullRequestService::standing_in_for("github.com", &server.uri()).unwrap(),
        ENTERPRISE_ORIGIN,
    )
    .unwrap()
}

pub(crate) fn github_with_token(server: &MockServer, token: Option<&str>) -> Github {
    serving(
        &server.uri(),
        Configuration {
            token: token.map(SecretValue::from),
            ..configured(OWNER, REPOSITORY)
        },
    )
}

pub(crate) async fn mock(server: &MockServer, endpoint: &str, response: Value) {
    mock_answering(server, endpoint, response, Some(BEARER)).await;
}

pub(crate) async fn mock_answering(
    server: &MockServer,
    endpoint: &str,
    response: Value,
    authorization: Option<&str>,
) {
    let request =
        Mock::given(method("GET")).and(path(format!("/repos/owner/repository/{endpoint}")));
    let request = match authorization {
        Some(value) => request.and(header("Authorization", value)),
        None => request,
    };
    request
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .mount(server)
        .await;
}

pub(crate) async fn assert_sent_without_authorization(server: &MockServer) {
    let requests = server.received_requests().await.unwrap();
    assert!(!requests.is_empty(), "the source must have been read");
    for request in requests {
        assert!(
            !request.headers.contains_key("authorization"),
            "{} {} carried an Authorization header",
            request.method,
            request.url.path()
        );
    }
}

pub(crate) async fn mock_green_ci(server: &MockServer) {
    mock_green_ci_answering(server, Some(BEARER)).await;
}

pub(crate) async fn mock_green_ci_answering(server: &MockServer, authorization: Option<&str>) {
    mock_answering(
        server,
        "actions/runs",
        json!({"workflow_runs": [{"id": 11, "workflow_id": 1, "name": "CI",
               "path": ".github/workflows/ci.yml", "event": "pull_request",
               "head_sha": COMMIT, "status": "completed", "conclusion": "success",
               "created_at": "2026-09-05T00:00:00Z", "updated_at": "2026-09-05T00:05:00Z",
               "html_url": "https://github.com/owner/repository/actions/runs/11",
               "repository": {"full_name": "owner/repository"}}], "total_count": 1}),
        authorization,
    )
    .await;
    mock_answering(
        server,
        &format!("commits/{COMMIT}/check-runs"),
        json!({"check_runs": [{"id": 21, "name": "test", "head_sha": COMMIT, "status": "completed", "conclusion": "success"}]}),
        authorization,
    )
    .await;
    mock_answering(
        server,
        &format!("commits/{COMMIT}/status"),
        json!({"statuses": [], "total_count": 0}),
        authorization,
    )
    .await;
}

pub(crate) async fn commit_and_empty_ci(server: &MockServer) {
    mock(server, "commits/main", json!({"sha": COMMIT})).await;
    mock(
        server,
        "actions/runs",
        json!({"workflow_runs": [], "total_count": 0}),
    )
    .await;
    mock_statuses(server, json!([])).await;
}

pub(crate) async fn mock_statuses(server: &MockServer, statuses: Value) {
    let total = statuses.as_array().map_or(0, Vec::len);
    mock(
        server,
        &format!("commits/{COMMIT}/status"),
        json!({"statuses": statuses, "total_count": total}),
    )
    .await;
}

pub(crate) async fn check_runs(server: &MockServer, rows: Value) {
    mock(
        server,
        &format!("commits/{COMMIT}/check-runs"),
        json!({ "check_runs": rows }),
    )
    .await;
}

pub(crate) async fn mock_graphql(server: &MockServer, response: Value) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(header("Authorization", BEARER))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .mount(server)
        .await;
}

pub(crate) fn summary(score: u8, sha: &str) -> String {
    format!("**Confidence Score:** {score}/5\n_Last reviewed commit: {sha}_")
}

pub(crate) fn thread_counts(resolved: bool, more_threads: bool) -> Value {
    json!({"data": {"repository": {"pullRequest": {
        "number": 7,
        "reviewThreads": {
            "pageInfo": {"hasNextPage": more_threads, "endCursor": null},
            "nodes": [{"id": "PRRT_1", "isResolved": resolved}]
        }
    }}}})
}

pub(crate) fn conversation_comment(id: u64, author: &str, body: &str) -> Value {
    json!({
        "id": id,
        "body": body,
        "html_url": format!("https://github.com/owner/repository/pull/7#issuecomment-{id}"),
        "created_at": "2026-09-05T00:00:00Z",
        "user": {"login": author}
    })
}

/// Pull request 7's review: one thread, resolved or not, more of them
/// GitHub would not page to when `more_threads`, and the conversation
/// comment a review bot summarised the change in.
pub(crate) async fn mock_review(
    server: &MockServer,
    resolved: bool,
    body: &str,
    more_threads: bool,
) {
    mock_graphql(server, thread_counts(resolved, more_threads)).await;
    mock(
        server,
        "issues/7/comments",
        json!([conversation_comment(11, "greptile-apps", body)]),
    )
    .await;
}

pub(crate) fn pull_detail(head: &str) -> Value {
    json!({"node_id": "PR_7", "number": 7, "draft": false, "state": "open",
           "title": "Ship the billing export",
           "html_url": "https://github.com/owner/repository/pull/7",
           "updated_at": "2026-09-05T00:00:00Z",
           "head": {"ref": "feature", "sha": head},
           "base": {"ref": "main", "sha": COMMIT}})
}

pub(crate) async fn mock_pull(server: &MockServer, head: &str) {
    mock(server, "pulls/7", pull_detail(head)).await;
}
