use abnegate_vcs::GitLabService;
use chrono::Utc;
use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha256;
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Mutex;
use std::thread;
use uuid::Uuid;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::any;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::path_regex;
use wiremock::matchers::query_param;
use wiremock::matchers::query_param_is_missing;
use zone_context::ContextError;
use zone_context::FetchResult;
use zone_context::adapters::GitLabAdapter;
use zone_context::adapters::NoOpProgress;
use zone_context::adapters::ProgressCallback;
use zone_context::adapters::SourceAdapter;
use zone_context::content::ContentItem;
use zone_context::content::FetchConfig;
use zone_context::content::FetchStrategy;
use zone_context::content::IndexedBlob;
use zone_core::Source;
use zone_core::SourceCategory;
use zone_core::SourceType;

const LOOPBACK: &str = "127.0.0.1:0";
const PROJECT: &str = "group/project";
const PROJECT_ROUTE: &str = "/api/v4/projects/group%2Fproject";
const TREE_ROUTE: &str = "/api/v4/projects/group%2Fproject/repository/tree";
const BLOB_ROUTES: &str = r"^/api/v4/projects/group%2Fproject/repository/blobs/";
const FILE_ROUTES: &str = r"^/api/v4/projects/group%2Fproject/repository/files/";
const FIRST_PAGE_QUERY: &str = "ref=main&recursive=true&per_page=100";
const USER_AGENT: &str = "abnegate-vcs";
const PRIVATE_TOKEN: &str = "private-token";
const MAXIMUM_ANSWER_BYTES: usize = 16 * 1024 * 1024;
const RETRIED_ATTEMPTS: u64 = 4;
const ACCESS_FORBIDDEN: &str = "Permission denied: Access forbidden - check token permissions";
const MALFORMED_TOKEN: &str =
    "Configuration error: Invalid authorization token format (token not logged for security)";
const MISSING_TREE: &str = "Adapter error for gitlab: Failed to fetch tree: 404 Not Found";
const UNREADABLE_TREE: &str = "Adapter error for gitlab: Failed to parse tree: GitLab answered in a form this crate cannot read";
const REPEATED_TREE: &str = "Adapter error for gitlab: Failed to parse tree: GitLab linked a tree's next page to one already read";
const EMPTY_PAGE_TREE: &str = "Adapter error for gitlab: Failed to parse tree: GitLab linked an empty page of a tree to a next one";

fn source(config: Value) -> Source {
    Source {
        id: Uuid::new_v4(),
        name: PROJECT.to_string(),
        source_type: SourceType::GitLab,
        category: SourceCategory::File,
        config,
        is_active: true,
        last_synced_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn project() -> Source {
    source(json!({ "project": PROJECT }))
}

#[derive(Default)]
struct RecordedProgress {
    reports: Mutex<Vec<(usize, Option<usize>)>>,
}

impl RecordedProgress {
    fn reports(&self) -> Vec<(usize, Option<usize>)> {
        self.reports.lock().unwrap().clone()
    }
}

impl ProgressCallback for RecordedProgress {
    fn on_item(&self, _item: &ContentItem) {}

    fn on_progress(&self, current: usize, total: Option<usize>) {
        self.reports.lock().unwrap().push((current, total));
    }

    fn on_message(&self, _message: &str) {}
}

fn addressed(origin: &str) -> GitLabAdapter {
    GitLabAdapter::with_service(
        GitLabService::configured_allowing_http(origin)
            .unwrap()
            .with_anonymous_reads(),
    )
}

fn served(server: &MockServer) -> GitLabAdapter {
    addressed(&server.uri())
}

fn dropping() -> String {
    let listener = TcpListener::bind(LOOPBACK).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || listener.incoming().for_each(drop));
    origin
}

fn object(digit: char) -> String {
    digit.to_string().repeat(40)
}

fn blob_route(digit: char) -> String {
    format!("{PROJECT_ROUTE}/repository/blobs/{}/raw", object(digit))
}

fn listed(path: &str, kind: &str, mode: &str, digit: char) -> Value {
    json!({
        "id": object(digit),
        "name": path.rsplit('/').next().unwrap(),
        "type": kind,
        "path": path,
        "mode": mode,
    })
}

fn file(path: &str, digit: char) -> Value {
    listed(path, "blob", "100644", digit)
}

fn described(default_branch: Value) -> Value {
    json!({
        "id": 1,
        "name": "project",
        "default_branch": default_branch,
    })
}

fn linking(link: String, entries: Value) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("link", link)
        .set_body_json(entries)
}

async fn answer_project(server: &MockServer, answer: ResponseTemplate) {
    Mock::given(method("GET"))
        .and(path(PROJECT_ROUTE))
        .respond_with(answer)
        .mount(server)
        .await;
}

async fn mount_project(server: &MockServer) {
    answer_project(
        server,
        ResponseTemplate::new(200).set_body_json(described(json!("main"))),
    )
    .await;
}

async fn mount_tree(server: &MockServer, entries: Value) {
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .respond_with(ResponseTemplate::new(200).set_body_json(entries))
        .mount(server)
        .await;
}

async fn refuse_tree(server: &MockServer) {
    Mock::given(path(TREE_ROUTE))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(0)
        .mount(server)
        .await;
}

async fn mount_blob(server: &MockServer, digit: char, content: &[u8]) {
    Mock::given(method("GET"))
        .and(path(blob_route(digit)))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(content.to_vec()))
        .mount(server)
        .await;
}

async fn received(server: &MockServer) -> Vec<Request> {
    server
        .received_requests()
        .await
        .expect("the stand-in records requests")
}

async fn tree_requests(server: &MockServer) -> Vec<Request> {
    received(server)
        .await
        .into_iter()
        .filter(|request| request.url.path() == TREE_ROUTE)
        .collect()
}

async fn fetched(
    adapter: &GitLabAdapter,
    source: &Source,
    strategy: FetchStrategy,
) -> zone_context::Result<FetchResult> {
    adapter
        .fetch(source, &FetchConfig::default(), strategy, &NoOpProgress)
        .await
}

fn titles(result: &FetchResult) -> Vec<&str> {
    let mut titles: Vec<&str> = result
        .items
        .iter()
        .map(|item| item.title.as_str())
        .collect();
    titles.sort_unstable();
    titles
}

fn content<'a>(result: &'a FetchResult, title: &str) -> Option<&'a str> {
    result
        .items
        .iter()
        .find(|item| item.title == title)
        .and_then(|item| item.content.as_deref())
}

fn assert_gitlab_failure(error: &ContextError, expected: &str) {
    assert!(
        matches!(error, ContextError::Adapter { source_type, .. } if source_type == "gitlab"),
        "{error:?}"
    );
    assert_eq!(error.to_string(), expected);
}

fn assert_invalid_source(error: &ContextError, expected: &str) {
    assert!(
        matches!(error, ContextError::InvalidSourceConfig(_)),
        "{error:?}"
    );
    assert_eq!(error.to_string(), expected);
}

#[tokio::test]
async fn an_unauthorized_or_forbidden_project_is_permission_denied() {
    for status in [401, 403] {
        let server = MockServer::start().await;
        answer_project(&server, ResponseTemplate::new(status)).await;

        let error = served(&server).verify(&project()).await.unwrap_err();

        assert!(
            matches!(error, ContextError::PermissionDenied(_)),
            "{status}: {error:?}"
        );
        assert_eq!(error.to_string(), ACCESS_FORBIDDEN, "{status}");
    }
}

#[tokio::test]
async fn a_null_default_branch_is_fatal_with_or_without_a_configured_branch() {
    let expected = "Adapter error for gitlab: Failed to parse project info: default_branch is null";
    for config in [
        json!({ "project": PROJECT }),
        json!({ "project": PROJECT, "branch": "main" }),
    ] {
        let server = MockServer::start().await;
        answer_project(
            &server,
            ResponseTemplate::new(200).set_body_json(described(Value::Null)),
        )
        .await;
        refuse_tree(&server).await;
        let adapter = served(&server);
        let configured = source(config.clone());

        let verified = adapter.verify(&configured).await.unwrap_err();
        let error = fetched(&adapter, &configured, FetchStrategy::MetadataOnly)
            .await
            .unwrap_err();

        assert_gitlab_failure(&verified, expected);
        assert_gitlab_failure(&error, expected);
    }
}

#[tokio::test]
async fn a_rate_limited_project_is_retried_three_times_and_then_rate_limited() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(PROJECT_ROUTE))
        .respond_with(ResponseTemplate::new(429))
        .expect(RETRIED_ATTEMPTS)
        .mount(&server)
        .await;

    let error = served(&server).verify(&project()).await.unwrap_err();

    assert!(
        matches!(
            error,
            ContextError::RateLimited {
                retry_after_secs: 60
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.to_string(), "Rate limited: retry after 60s");
}

#[tokio::test]
async fn a_rate_limit_on_a_later_tree_page_retries_the_whole_walk() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(query_param_is_missing("page"))
        .respond_with(linking(
            format!("<{}{TREE_ROUTE}?page=2>; rel=\"next\"", server.uri()),
            json!([file("README.md", 'a')]),
        ))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([file("src/lib.rs", 'b')])))
        .expect(1)
        .mount(&server)
        .await;

    let result = fetched(&served(&server), &project(), FetchStrategy::MetadataOnly)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["README.md", "lib.rs"]);
    let first_pages = tree_requests(&server)
        .await
        .iter()
        .filter(|request| request.url.query() == Some(FIRST_PAGE_QUERY))
        .count();
    assert_eq!(
        first_pages, 2,
        "a rate limit on page 2 restarts the walk from page 1"
    );
}

#[tokio::test]
async fn a_project_status_is_reported_in_zone_s_words() {
    for (status, expected) in [
        (
            500,
            "Adapter error for gitlab: API error: 500 Internal Server Error",
        ),
        (
            599,
            "Adapter error for gitlab: API error: 599 <unknown status code>",
        ),
    ] {
        let server = MockServer::start().await;
        answer_project(&server, ResponseTemplate::new(status)).await;

        let error = served(&server).verify(&project()).await.unwrap_err();

        assert_gitlab_failure(&error, expected);
    }

    let server = MockServer::start().await;
    answer_project(&server, ResponseTemplate::new(404)).await;

    let error = served(&server).verify(&project()).await.unwrap_err();

    assert_invalid_source(
        &error,
        "Invalid source configuration: Project not found: group/project",
    );
}

#[tokio::test]
async fn a_tree_status_is_fatal_with_its_status() {
    for (status, written) in [
        (401, "401 Unauthorized"),
        (403, "403 Forbidden"),
        (404, "404 Not Found"),
        (500, "500 Internal Server Error"),
    ] {
        let server = MockServer::start().await;
        mount_project(&server).await;
        Mock::given(method("GET"))
            .and(path(TREE_ROUTE))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;

        let error = fetched(&served(&server), &project(), FetchStrategy::MetadataOnly)
            .await
            .unwrap_err();

        assert_gitlab_failure(
            &error,
            &format!("Adapter error for gitlab: Failed to fetch tree: {written}"),
        );
    }
}

#[tokio::test]
async fn a_blob_status_aborts_a_full_fetch() {
    for (status, written) in [
        (401, "401 Unauthorized"),
        (403, "403 Forbidden"),
        (404, "404 Not Found"),
        (500, "500 Internal Server Error"),
    ] {
        let server = MockServer::start().await;
        mount_project(&server).await;
        mount_tree(&server, json!([file("README.md", 'a')])).await;
        Mock::given(method("GET"))
            .and(path(blob_route('a')))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;

        let error = fetched(&served(&server), &project(), FetchStrategy::Full)
            .await
            .unwrap_err();

        assert_gitlab_failure(
            &error,
            &format!("Adapter error for gitlab: Failed to fetch content: {written}"),
        );
    }
}

#[tokio::test]
async fn a_blob_that_is_not_utf8_is_skipped_and_the_rest_are_returned() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    mount_tree(
        &server,
        json!([file("README.md", 'a'), file("binary.txt", 'b')]),
    )
    .await;
    mount_blob(&server, 'a', b"# Readme").await;
    mount_blob(&server, 'b', &[0xff, 0xfe]).await;

    for strategy in [
        FetchStrategy::Full,
        FetchStrategy::Partial {
            max_tokens: 100_000,
        },
        FetchStrategy::Progressive {
            priority_order: Vec::new(),
        },
    ] {
        let result = fetched(&served(&server), &project(), strategy.clone())
            .await
            .unwrap();

        assert_eq!(titles(&result), ["README.md"], "{strategy:?}");
        assert_eq!(content(&result, "README.md"), Some("# Readme"));
    }
}

#[tokio::test]
async fn a_project_with_an_empty_or_dot_segment_is_not_found_unsent() {
    for name in ["group/", "/group", "a//b", "group/./x"] {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(described(json!("main"))))
            .expect(0)
            .mount(&server)
            .await;

        let error = served(&server)
            .verify(&source(json!({ "project": name })))
            .await
            .unwrap_err();

        assert_invalid_source(
            &error,
            &format!("Invalid source configuration: Project not found: {name}"),
        );
        assert!(received(&server).await.is_empty(), "{name} was sent");
    }
}

#[tokio::test]
async fn a_path_prefix_the_crate_refuses_is_a_missing_tree_unsent() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    refuse_tree(&server).await;

    for prefix in ["a//b", "./src", "src/.", "%2e%2e/x", "src\tlib"] {
        let error = fetched(
            &served(&server),
            &source(json!({ "project": PROJECT, "path": prefix })),
            FetchStrategy::MetadataOnly,
        )
        .await
        .unwrap_err();

        assert_gitlab_failure(&error, MISSING_TREE);
    }
    assert!(tree_requests(&server).await.is_empty());
}

#[tokio::test]
async fn a_path_prefix_is_sent_without_its_slashes() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(query_param("path", "src"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([file("src/lib.rs", 'a'), file("README.md", 'b')])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let result = fetched(
        &served(&server),
        &source(json!({ "project": PROJECT, "path": "src/" })),
        FetchStrategy::MetadataOnly,
    )
    .await
    .unwrap();

    assert_eq!(titles(&result), ["lib.rs"]);
    let sent = tree_requests(&server).await;
    assert_eq!(
        sent[0].url.query(),
        Some("ref=main&recursive=true&per_page=100&path=src")
    );
}

#[tokio::test]
async fn a_file_named_with_two_dots_is_fetched() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    mount_tree(
        &server,
        json!([file("foo..bar.txt", 'a'), file("src/lib.rs", 'b')]),
    )
    .await;
    mount_blob(&server, 'a', b"two dots").await;
    mount_blob(&server, 'b', b"pub fn lib() {}").await;

    let result = fetched(&served(&server), &project(), FetchStrategy::Full)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["foo..bar.txt", "lib.rs"]);
    assert_eq!(content(&result, "foo..bar.txt"), Some("two dots"));
    assert_eq!(content(&result, "lib.rs"), Some("pub fn lib() {}"));
}

#[tokio::test]
async fn a_path_prefix_that_climbs_or_is_absolute_is_still_refused() {
    for (prefix, expected) in [
        (
            "src/../x",
            "Invalid source configuration: Path cannot contain '..'",
        ),
        (
            "/src",
            "Invalid source configuration: Path cannot be absolute",
        ),
    ] {
        let server = MockServer::start().await;
        mount_project(&server).await;
        refuse_tree(&server).await;
        Mock::given(path_regex(BLOB_ROUTES))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;

        let error = fetched(
            &served(&server),
            &source(json!({ "project": PROJECT, "path": prefix })),
            FetchStrategy::Full,
        )
        .await
        .unwrap_err();

        assert_invalid_source(&error, expected);
    }
}

#[tokio::test]
async fn a_symlink_blob_is_listed() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    mount_tree(
        &server,
        json!([
            file("README.md", 'a'),
            listed("link.md", "blob", "120000", 'b'),
            listed("src", "tree", "040000", 'c'),
        ]),
    )
    .await;

    let result = fetched(&served(&server), &project(), FetchStrategy::MetadataOnly)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["README.md", "link.md"]);
}

#[tokio::test]
async fn a_short_tree_id_fails_the_tree() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    mount_tree(
        &server,
        json!([{
            "id": "abc123",
            "name": "README.md",
            "type": "blob",
            "path": "README.md",
            "mode": "100644",
        }]),
    )
    .await;

    let error = fetched(&served(&server), &project(), FetchStrategy::MetadataOnly)
        .await
        .unwrap_err();

    assert_gitlab_failure(
        &error,
        "Adapter error for gitlab: Failed to parse tree: Invalid object name: \"abc123\"",
    );
}

#[tokio::test]
async fn a_next_page_linked_on_another_origin_is_read_from_the_configured_one() {
    let elsewhere = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(0)
        .mount(&elsewhere)
        .await;
    let server = MockServer::start().await;
    mount_project(&server).await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(query_param_is_missing("page"))
        .respond_with(linking(
            format!("<{}{TREE_ROUTE}?page=2>; rel=\"next\"", elsewhere.uri()),
            json!([file("README.md", 'a')]),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([file("src/lib.rs", 'b')])))
        .expect(1)
        .mount(&server)
        .await;

    let result = fetched(&served(&server), &project(), FetchStrategy::MetadataOnly)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["README.md", "lib.rs"]);
    let sent = tree_requests(&server).await;
    assert_eq!(
        sent[1].url.query(),
        Some("page=2"),
        "only the link's query is taken"
    );
    assert!(
        received(&elsewhere).await.is_empty(),
        "the linked origin is never reached"
    );
}

#[tokio::test]
async fn a_next_link_with_an_empty_query_fails_the_tree_unfollowed() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .respond_with(linking(
            format!("<{}{TREE_ROUTE}?>; rel=\"next\"", server.uri()),
            json!([file("README.md", 'a')]),
        ))
        .mount(&server)
        .await;

    let error = fetched(&served(&server), &project(), FetchStrategy::MetadataOnly)
        .await
        .unwrap_err();

    assert_gitlab_failure(&error, UNREADABLE_TREE);
    assert_eq!(tree_requests(&server).await.len(), 1);
}

#[tokio::test]
async fn a_next_link_back_to_a_page_already_read_fails_the_tree() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(query_param_is_missing("page"))
        .respond_with(linking(
            format!(
                "<{}{TREE_ROUTE}?{FIRST_PAGE_QUERY}>; rel=\"next\"",
                server.uri()
            ),
            json!([file("README.md", 'a')]),
        ))
        .mount(&server)
        .await;

    let error = fetched(&served(&server), &project(), FetchStrategy::MetadataOnly)
        .await
        .unwrap_err();

    assert_gitlab_failure(&error, REPEATED_TREE);
    let sent = tree_requests(&server).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].url.query(), Some(FIRST_PAGE_QUERY));
}

#[tokio::test]
async fn an_empty_page_that_links_on_fails_the_tree() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(query_param_is_missing("page"))
        .respond_with(linking(
            format!("<{}{TREE_ROUTE}?page=2>; rel=\"next\"", server.uri()),
            json!([]),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([file("README.md", 'a')])))
        .expect(0)
        .mount(&server)
        .await;

    let error = fetched(&served(&server), &project(), FetchStrategy::MetadataOnly)
        .await
        .unwrap_err();

    assert_gitlab_failure(&error, EMPTY_PAGE_TREE);
}

#[tokio::test]
async fn content_is_read_by_blob_object_not_by_path() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    mount_tree(&server, json!([file("README.md", 'a')])).await;
    Mock::given(method("GET"))
        .and(path(blob_route('a')))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"# Readme".to_vec()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path_regex(FILE_ROUTES))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let result = fetched(&served(&server), &project(), FetchStrategy::Full)
        .await
        .unwrap();

    assert_eq!(content(&result, "README.md"), Some("# Readme"));
    assert_eq!(
        result.items[0].metadata.size_bytes,
        Some(b"# Readme".len()),
        "the size is the blob's byte length"
    );
    assert_eq!(
        result.items[0].metadata.commit_hash.as_deref(),
        Some(object('a').as_str())
    );
}

#[tokio::test]
async fn a_redirect_off_the_origin_is_refused_and_never_reached() {
    let elsewhere = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(described(json!("main"))))
        .expect(0)
        .mount(&elsewhere)
        .await;
    let server = MockServer::start().await;
    answer_project(
        &server,
        ResponseTemplate::new(302)
            .insert_header("location", format!("{}{PROJECT_ROUTE}", elsewhere.uri())),
    )
    .await;

    let error = served(&server).verify(&project()).await.unwrap_err();

    assert_gitlab_failure(&error, "Adapter error for gitlab: API error: 302 Found");
    assert!(received(&elsewhere).await.is_empty());
}

#[tokio::test]
async fn every_request_carries_the_crate_s_user_agent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(PROJECT_ROUTE))
        .and(header("user-agent", USER_AGENT))
        .respond_with(ResponseTemplate::new(200).set_body_json(described(json!("main"))))
        .expect(1)
        .mount(&server)
        .await;
    mount_tree(&server, json!([file("README.md", 'a')])).await;
    mount_blob(&server, 'a', b"# Readme").await;

    fetched(&served(&server), &project(), FetchStrategy::Full)
        .await
        .unwrap();

    let sent = received(&server).await;
    assert_eq!(sent.len(), 3);
    for request in &sent {
        assert_eq!(
            request
                .headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok()),
            Some(USER_AGENT),
            "{}",
            request.url
        );
    }
}

#[tokio::test]
async fn an_empty_or_absent_token_sends_no_private_token() {
    for config in [
        json!({ "project": PROJECT, "token": "" }),
        json!({ "project": PROJECT }),
    ] {
        let server = MockServer::start().await;
        mount_project(&server).await;
        mount_tree(&server, json!([file("README.md", 'a')])).await;
        mount_blob(&server, 'a', b"# Readme").await;

        fetched(
            &served(&server),
            &source(config.clone()),
            FetchStrategy::Full,
        )
        .await
        .unwrap();

        let sent = received(&server).await;
        assert_eq!(sent.len(), 3, "{config}");
        for request in &sent {
            assert!(
                !request.headers.contains_key(PRIVATE_TOKEN),
                "{config}: {}",
                request.url
            );
        }
    }
}

#[tokio::test]
async fn a_token_that_cannot_be_a_header_fails_every_read_unsent() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(described(json!("main"))))
        .expect(0)
        .mount(&server)
        .await;
    let adapter = served(&server);
    let configured = source(json!({ "project": PROJECT, "token": "bad\ntoken" }));

    let mut errors = vec![
        adapter.verify(&configured).await.unwrap_err(),
        adapter.estimate_tokens(&configured).await.unwrap_err(),
        adapter.get_sync_state(&configured).await.unwrap_err(),
    ];
    for strategy in [
        FetchStrategy::Full,
        FetchStrategy::MetadataOnly,
        FetchStrategy::Partial {
            max_tokens: 100_000,
        },
        FetchStrategy::Progressive {
            priority_order: Vec::new(),
        },
    ] {
        errors.push(fetched(&adapter, &configured, strategy).await.unwrap_err());
    }

    for error in &errors {
        assert!(matches!(error, ContextError::Config(_)), "{error:?}");
        assert_eq!(error.to_string(), MALFORMED_TOKEN);
        assert!(!format!("{error} {error:?}").contains("bad"));
    }
    assert!(received(&server).await.is_empty());
}

#[tokio::test]
async fn a_token_is_sent_as_the_private_token_header() {
    let token = "glpat-abc";
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(PROJECT_ROUTE))
        .and(header(PRIVATE_TOKEN, token))
        .respond_with(ResponseTemplate::new(200).set_body_json(described(json!("main"))))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(TREE_ROUTE))
        .and(header(PRIVATE_TOKEN, token))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([file("README.md", 'a')])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(blob_route('a')))
        .and(header(PRIVATE_TOKEN, token))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"# Readme".to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    let result = fetched(
        &served(&server),
        &source(json!({ "project": PROJECT, "token": token })),
        FetchStrategy::Full,
    )
    .await
    .unwrap();

    assert_eq!(content(&result, "README.md"), Some("# Readme"));
}

#[tokio::test]
async fn a_configured_host_or_base_url_is_ignored() {
    for key in ["host", "base_url"] {
        let server = MockServer::start().await;
        mount_project(&server).await;
        mount_tree(&server, json!([file("README.md", 'a')])).await;
        let mut config = json!({ "project": PROJECT });
        config[key] = json!("http://127.0.0.1:9");

        let result = fetched(
            &served(&server),
            &source(config),
            FetchStrategy::MetadataOnly,
        )
        .await
        .unwrap();

        assert_eq!(titles(&result), ["README.md"], "{key}");
    }
}

#[tokio::test]
async fn the_stored_version_hashes_every_blob_s_path_and_object() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    mount_tree(
        &server,
        json!([file("README.md", 'a'), listed("src", "tree", "040000", 'b')]),
    )
    .await;
    let mut hasher = Sha256::new();
    hasher.update(b"README.md");
    hasher.update(object('a').as_bytes());
    let expected: String = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let adapter = served(&server);

    let result = fetched(&adapter, &project(), FetchStrategy::MetadataOnly)
        .await
        .unwrap();
    let state = adapter.get_sync_state(&project()).await.unwrap();

    assert_eq!(result.version.as_deref(), Some(expected.as_str()));
    assert_eq!(state.version.as_deref(), Some(expected.as_str()));
}

#[tokio::test]
async fn a_default_branch_git_would_refuse_fails_the_project_read() {
    for config in [
        json!({ "project": PROJECT }),
        json!({ "project": PROJECT, "branch": "main" }),
    ] {
        let server = MockServer::start().await;
        answer_project(
            &server,
            ResponseTemplate::new(200).set_body_json(described(json!("a..b"))),
        )
        .await;
        refuse_tree(&server).await;

        let error = fetched(
            &served(&server),
            &source(config.clone()),
            FetchStrategy::MetadataOnly,
        )
        .await
        .unwrap_err();

        assert_gitlab_failure(
            &error,
            "Adapter error for gitlab: Failed to parse project info: Invalid branch name: \"a..b\"",
        );
    }
}

#[tokio::test]
async fn a_configured_branch_the_crate_refuses_is_a_missing_tree_unsent() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    refuse_tree(&server).await;

    let error = fetched(
        &served(&server),
        &source(json!({ "project": PROJECT, "branch": "@" })),
        FetchStrategy::MetadataOnly,
    )
    .await
    .unwrap_err();

    assert_gitlab_failure(&error, MISSING_TREE);
}

#[tokio::test]
async fn an_unreadable_or_oversized_project_answer_fails_with_a_fixed_reason() {
    let mut oversized = vec![b' '; MAXIMUM_ANSWER_BYTES + 1];
    oversized[0] = b'{';
    for (body, expected) in [
        (
            b"secret-body {".to_vec(),
            "Adapter error for gitlab: Failed to parse project info: GitLab answered in a form this crate cannot read",
        ),
        (
            oversized,
            "Adapter error for gitlab: Failed to parse project info: GitLab's answer was larger than this crate reads",
        ),
    ] {
        let server = MockServer::start().await;
        answer_project(&server, ResponseTemplate::new(200).set_body_bytes(body)).await;

        let error = served(&server).verify(&project()).await.unwrap_err();

        assert_gitlab_failure(&error, expected);
        assert!(!error.to_string().contains("secret-body"));
    }
}

#[tokio::test]
async fn an_origin_that_drops_the_connection_is_a_request_failure() {
    let error = addressed(&dropping()).verify(&project()).await.unwrap_err();

    assert!(
        matches!(
            &error,
            ContextError::Adapter { source_type, message }
                if source_type == "gitlab" && message.starts_with("Request failed: ")
        ),
        "{error:?}"
    );
    assert!(
        error
            .to_string()
            .starts_with("Adapter error for gitlab: Request failed: "),
        "{error}"
    );
}

#[tokio::test]
async fn a_project_answer_with_only_its_default_branch_is_read() {
    let server = MockServer::start().await;
    answer_project(
        &server,
        ResponseTemplate::new(200).set_body_json(json!({ "default_branch": "main" })),
    )
    .await;
    mount_tree(&server, json!([file("README.md", 'a')])).await;
    let adapter = served(&server);

    adapter.verify(&project()).await.unwrap();
    let result = fetched(&adapter, &project(), FetchStrategy::MetadataOnly)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["README.md"]);
}

#[tokio::test]
async fn a_progressive_fetch_returns_items_in_priority_order() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    mount_tree(
        &server,
        json!([
            file("README.md", 'a'),
            file("src/lib.rs", 'b'),
            file("notes.txt", 'c'),
        ]),
    )
    .await;
    mount_blob(&server, 'a', b"# Readme").await;
    mount_blob(&server, 'b', b"fn lib() {}").await;
    mount_blob(&server, 'c', b"notes").await;

    let result = fetched(
        &served(&server),
        &project(),
        FetchStrategy::Progressive {
            priority_order: vec![
                "notes.txt".to_string(),
                "*.rs".to_string(),
                "README.md".to_string(),
            ],
        },
    )
    .await
    .unwrap();

    let ordered: Vec<&str> = result
        .items
        .iter()
        .map(|item| item.title.as_str())
        .collect();
    assert_eq!(ordered, ["notes.txt", "lib.rs", "README.md"]);
}

async fn reported(
    server: &MockServer,
    config: &FetchConfig,
    strategy: FetchStrategy,
) -> Vec<(usize, Option<usize>)> {
    let progress = RecordedProgress::default();
    let result = served(server)
        .fetch(&project(), config, strategy, &progress)
        .await
        .unwrap();
    assert!(!titles(&result).contains(&"binary.txt"));
    progress.reports()
}

#[tokio::test]
async fn only_a_full_fetch_reports_progress_for_a_file_that_is_not_utf8() {
    let server = MockServer::start().await;
    mount_project(&server).await;
    mount_tree(
        &server,
        json!([
            file("unchanged.rs", 'c'),
            file("README.md", 'a'),
            file("binary.txt", 'b'),
        ]),
    )
    .await;
    mount_blob(&server, 'a', b"# Readme").await;
    mount_blob(&server, 'b', &[0xff, 0xfe]).await;
    mount_blob(&server, 'c', b"fn unchanged() {}").await;
    let indexed = FetchConfig {
        known_blobs: HashMap::from([(
            format!("gitlab://{PROJECT}/unchanged.rs@main"),
            IndexedBlob {
                blob_sha: Some(object('c')),
                has_embeddings: true,
            },
        )]),
        ..FetchConfig::default()
    };

    let full = reported(&server, &indexed, FetchStrategy::Full).await;
    let partial = reported(
        &server,
        &FetchConfig::default(),
        FetchStrategy::Partial {
            max_tokens: 100_000,
        },
    )
    .await;
    let progressive = reported(
        &server,
        &FetchConfig::default(),
        FetchStrategy::Progressive {
            priority_order: vec!["unchanged.rs".to_string(), "README.md".to_string()],
        },
    )
    .await;

    assert_eq!(
        full,
        [(1, Some(2)), (2, Some(3))],
        "a full fetch counts the skipped file against every listed file"
    );
    assert_eq!(partial, [(1, Some(3)), (2, Some(3))]);
    assert_eq!(progressive, [(1, Some(3)), (2, Some(3))]);
}
