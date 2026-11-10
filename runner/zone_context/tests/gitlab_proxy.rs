//! Every GitLab client the adapter builds honours the proxy the environment
//! names, as the GitHub client does: the one handed to it, the gitlab.com
//! default and the one it configures for a source's own instance.
//!
//! Each test holds the process-wide [`Variables`] lock from before it names a
//! proxy until its last read, and builds every client inside that hold, so a
//! client reads only the proxy its own test named, whether the tests share a
//! process under `cargo test` or each has one under nextest. They live in
//! their own binary so that no other test builds a client while a proxy
//! variable is set.

use abnegate_vcs::GitLabService;
use abnegate_vcs::PullRequestService;
use chrono::Utc;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
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

const PROJECT: &str = "group/project";
const GITLAB_ROUTE: &str = "/api/v4/projects/group%2Fproject";
const GITLAB_API: &str = "/api/v4/";
const GITHUB_ROUTE: &str = "/repos/acme/project";
const LOOPBACK: &str = "127.0.0.1:0";
const BYPASSING: &[&str] = &[
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
    "REQUEST_METHOD",
];
const PLAIN_PROXIES: &[&str] = &["HTTP_PROXY", "http_proxy"];
const SECURE_PROXIES: &[&str] = &["HTTPS_PROXY", "https_proxy"];
const ASKED_WITHIN: Duration = Duration::from_secs(5);

/// Names `proxy` in `proxying` alone, unsetting every other variable that
/// names a proxy or exempts a host from one.
fn route(
    variables: &mut Variables,
    proxying: &[&'static str],
    ignored: &[&'static str],
    proxy: &str,
) {
    variables.clear(BYPASSING);
    variables.clear(ignored);
    for &name in proxying {
        variables.set(name, proxy);
    }
}

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

/// A proxy that reads the first line it is sent and then drops the
/// connection, so no read it is asked to tunnel ever leaves the machine.
async fn tunnelling() -> (String, JoinHandle<String>) {
    let listener = TcpListener::bind(LOOPBACK).await.unwrap();
    let proxy = format!("http://{}", listener.local_addr().unwrap());
    let request_line = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).await.unwrap();
        line
    });
    (proxy, request_line)
}

async fn asked(request_line: JoinHandle<String>) -> String {
    tokio::time::timeout(ASKED_WITHIN, request_line)
        .await
        .expect("the proxy is asked to tunnel the read")
        .unwrap()
}

async fn unused_port() -> u16 {
    TcpListener::bind(LOOPBACK)
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn a_handed_service_reads_through_the_proxy_as_github_does() {
    let mut variables = Variables::lock();
    let proxy = serving_both_forges().await;
    let target = serving_both_forges().await;
    route(&mut variables, PLAIN_PROXIES, SECURE_PROXIES, &proxy.uri());

    let gitlab = GitLabAdapter::with_service(
        GitLabService::configured_allowing_http(&target.uri()).unwrap(),
    );
    let github = GitHubAdapter::with_service(
        PullRequestService::standing_in_for("github.com", &target.uri())
            .unwrap()
            .with_anonymous_reads(),
    );

    let gitlab_verified = gitlab
        .verify(&source(SourceType::GitLab, json!({ "project": PROJECT })))
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
        "the GitLab read is answered through the proxy: {gitlab_verified:?}"
    );
    assert!(
        proxied.iter().any(|sent| sent == GITLAB_ROUTE),
        "the proxy received the GitLab read: {proxied:?}"
    );
    assert!(
        !direct.iter().any(|sent| sent.starts_with(GITLAB_API)),
        "the GitLab instance received no read directly: {direct:?}"
    );
}

#[tokio::test]
async fn the_gitlab_com_default_is_tunnelled_through_the_proxy() {
    let mut variables = Variables::lock();
    let (proxy, request_line) = tunnelling().await;
    route(&mut variables, SECURE_PROXIES, PLAIN_PROXIES, &proxy);

    let verified = tokio::time::timeout(
        ASKED_WITHIN,
        GitLabAdapter::new().verify(&source(SourceType::GitLab, json!({ "project": PROJECT }))),
    )
    .await;

    assert_eq!(
        asked(request_line).await,
        "CONNECT gitlab.com:443 HTTP/1.1\r\n"
    );
    assert!(
        matches!(verified, Ok(Err(_))),
        "the stand-in drops the tunnel it is asked for: {verified:?}"
    );
}

#[tokio::test]
async fn a_configured_instance_is_tunnelled_through_the_proxy() {
    let mut variables = Variables::lock();
    let (proxy, request_line) = tunnelling().await;
    let instance = format!("127.0.0.1:{}", unused_port().await);
    route(&mut variables, SECURE_PROXIES, PLAIN_PROXIES, &proxy);

    let verified = GitLabAdapter::new()
        .verify(&source(
            SourceType::GitLab,
            json!({ "project": PROJECT, "host": format!("https://{instance}") }),
        ))
        .await;

    assert_eq!(
        asked(request_line).await,
        format!("CONNECT {instance} HTTP/1.1\r\n")
    );
    assert!(
        verified.is_err(),
        "the stand-in drops the tunnel it is asked for: {verified:?}"
    );
}
