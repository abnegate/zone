//! The GitLab client ignores the proxy the environment names, while the
//! GitHub client honours it. Alone in its binary, so no other test builds a
//! client while the proxy variables are set.

use abnegate_vcs::GitLabService;
use abnegate_vcs::PullRequestService;
use chrono::Utc;
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;
use zone_context::adapters::GitHubAdapter;
use zone_context::adapters::GitLabAdapter;
use zone_context::adapters::SourceAdapter;
use zone_core::Source;
use zone_core::SourceCategory;
use zone_core::SourceType;
use zone_core::variables::Variables;

const GITLAB_ROUTE: &str = "/api/v4/projects/group%2Fproject";
const GITLAB_API: &str = "/api/v4/";
const GITHUB_ROUTE: &str = "/repos/acme/project";
const BYPASSING: &[&str] = &[
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
    "HTTPS_PROXY",
    "https_proxy",
    "REQUEST_METHOD",
];
const PROXYING: &[&str] = &["HTTP_PROXY", "http_proxy"];

fn source(source_type: SourceType, config: Value) -> Source {
    Source {
        id: Uuid::new_v4(),
        name: "proxied".to_string(),
        source_type,
        category: SourceCategory::File,
        config,
        is_active: true,
        last_synced_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

async fn serving_both_forges() -> MockServer {
    let server = MockServer::start().await;
    for route in [GITLAB_ROUTE, GITHUB_ROUTE] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "default_branch": "main" })),
            )
            .mount(&server)
            .await;
    }
    server
}

async fn paths(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .expect("the stand-in records requests")
        .iter()
        .map(|request| request.url.path().to_string())
        .collect()
}

#[tokio::test]
async fn the_gitlab_client_ignores_the_environment_proxy_that_github_honours() {
    let mut variables = Variables::lock();
    variables.clear(BYPASSING);
    let proxy = serving_both_forges().await;
    let target = serving_both_forges().await;
    for &name in PROXYING {
        variables.set(name, proxy.uri());
    }

    let gitlab = GitLabAdapter::with_service(
        GitLabService::configured_allowing_http(&target.uri())
            .unwrap()
            .with_anonymous_reads(),
    );
    let github = GitHubAdapter::with_service(
        PullRequestService::standing_in_for("github.com", &target.uri())
            .unwrap()
            .with_anonymous_reads(),
    );

    let gitlab_verified = gitlab
        .verify(&source(
            SourceType::GitLab,
            json!({ "project": "group/project" }),
        ))
        .await;
    let github_verified = github
        .verify(&source(
            SourceType::GitHub,
            json!({ "owner": "acme", "repo": "project" }),
        ))
        .await;

    let proxied = paths(&proxy).await;
    let direct = paths(&target).await;
    assert!(
        github_verified.is_ok(),
        "the GitHub control reads through the proxy: {github_verified:?}"
    );
    assert!(
        proxied.iter().any(|sent| sent == GITHUB_ROUTE),
        "the proxy received the GitHub read, so the environment proxy was in force: {proxied:?}"
    );
    assert!(
        !direct.iter().any(|sent| sent == GITHUB_ROUTE),
        "the GitHub read went only through the proxy: {direct:?}"
    );
    assert!(
        gitlab_verified.is_ok(),
        "the GitLab read reaches its instance with the proxy set: {gitlab_verified:?}"
    );
    assert!(
        direct.iter().any(|sent| sent == GITLAB_ROUTE),
        "the GitLab instance received the GitLab read directly: {direct:?}"
    );
    assert!(
        !proxied.iter().any(|sent| sent.starts_with(GITLAB_API)),
        "the proxy received no GitLab read, so the GitLab client was built with no_proxy: {proxied:?}"
    );
}
