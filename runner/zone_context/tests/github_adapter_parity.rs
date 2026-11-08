use abnegate_vcs::PullRequestService;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::Utc;
use serde_json::Value;
use serde_json::json;
use std::net::TcpListener;
use uuid::Uuid;
use wiremock::Mock;
use wiremock::MockBuilder;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::any;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::path_regex;
use wiremock::matchers::query_param;
use zone_context::ContextError;
use zone_context::Result;
use zone_context::adapters::GitHubAdapter;
use zone_context::adapters::NoOpProgress;
use zone_context::adapters::SourceAdapter;
use zone_context::content::FetchConfig;
use zone_context::content::FetchResult;
use zone_context::content::FetchStrategy;
use zone_core::Source;
use zone_core::SourceCategory;
use zone_core::SourceType;

const REPOSITORY: &str = "/repos/o/r";
const MAIN: &str = "main";
const MALFORMED_TOKEN: &str =
    "Configuration error: Invalid authorization token format (token not logged for security)";
const NOT_FOUND: &str = "Invalid source configuration: Repository not found: o/r";
const RATE_LIMITED: &str = "Rate limited: retry after 60s";
const RETRIED_ATTEMPTS: u64 = 4;
const MAXIMUM_ANSWER_BYTES: usize = 16 * 1024 * 1024;

fn source(config: Value) -> Source {
    Source {
        id: Uuid::new_v4(),
        name: "o/r".to_string(),
        source_type: SourceType::GitHub,
        category: SourceCategory::File,
        config,
        is_active: true,
        last_synced_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn plain() -> Source {
    source(json!({ "owner": "o", "repo": "r" }))
}

fn addressed(origin: &str) -> GitHubAdapter {
    GitHubAdapter::with_service(
        PullRequestService::standing_in_for("github.com", origin)
            .unwrap()
            .with_anonymous_reads(),
    )
}

fn served(server: &MockServer) -> GitHubAdapter {
    addressed(&server.uri())
}

fn object(digit: char) -> String {
    std::iter::repeat_n(digit, 40).collect()
}

fn encoded(text: &str) -> String {
    STANDARD.encode(text)
}

fn blob(file: &str, digit: char, size: u64) -> Value {
    json!({
        "path": file,
        "mode": "100644",
        "type": "blob",
        "sha": object(digit),
        "size": size
    })
}

fn repository(default_branch: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "name": "r",
        "default_branch": default_branch
    }))
}

fn tree(entries: Vec<Value>) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "sha": object('a'),
        "tree": entries,
        "truncated": false
    }))
}

fn file(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "name": "file",
        "path": "file",
        "sha": object('f'),
        "size": text.len(),
        "type": "file",
        "content": encoded(text),
        "encoding": "base64"
    }))
}

fn repository_request() -> MockBuilder {
    Mock::given(method("GET")).and(path(REPOSITORY))
}

fn tree_request(reference: &str) -> MockBuilder {
    Mock::given(method("GET"))
        .and(path(format!("{REPOSITORY}/git/trees/{reference}")))
        .and(query_param("recursive", "1"))
}

fn contents_request(file: &str, reference: &str) -> MockBuilder {
    Mock::given(method("GET"))
        .and(path(format!("{REPOSITORY}/contents/{file}")))
        .and(query_param("ref", reference))
}

async fn mount_repository(server: &MockServer) {
    repository_request()
        .respond_with(repository(MAIN))
        .mount(server)
        .await;
}

async fn mount_tree(server: &MockServer, entries: Vec<Value>) {
    tree_request(MAIN)
        .respond_with(tree(entries))
        .mount(server)
        .await;
}

async fn mount_file(server: &MockServer, file_path: &str, text: &str) {
    contents_request(file_path, MAIN)
        .respond_with(file(text))
        .mount(server)
        .await;
}

async fn fetched(
    adapter: &GitHubAdapter,
    source: &Source,
    strategy: FetchStrategy,
) -> Result<FetchResult> {
    adapter
        .fetch(source, &FetchConfig::default(), strategy, &NoOpProgress)
        .await
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

fn titles(result: &FetchResult) -> Vec<&str> {
    result
        .items
        .iter()
        .map(|item| item.title.as_str())
        .collect()
}

fn adapted(error: &ContextError) -> bool {
    matches!(error, ContextError::Adapter { source_type, .. } if source_type == "github")
}

#[tokio::test]
async fn a_contents_401_skips_the_file_and_keeps_the_rest() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    mount_tree(
        &server,
        vec![blob("README.md", 'b', 10), blob("src/lib.rs", 'c', 10)],
    )
    .await;
    contents_request("README.md", MAIN)
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    mount_file(&server, "src/lib.rs", "pub fn lib() {}").await;

    let result = fetched(&served(&server), &plain(), FetchStrategy::Full)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["lib.rs"]);
    assert_eq!(result.items[0].content.as_deref(), Some("pub fn lib() {}"));
}

#[tokio::test]
async fn contents_that_are_not_base64_abort_the_fetch_as_a_parse_error() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    mount_tree(&server, vec![blob("README.md", 'b', 10)]).await;
    contents_request("README.md", MAIN)
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": "@@not base64@@"
        })))
        .mount(&server)
        .await;

    let error = fetched(&served(&server), &plain(), FetchStrategy::Full)
        .await
        .unwrap_err();

    assert!(matches!(error, ContextError::Parse(_)), "{error:?}");
    assert_eq!(
        error.to_string(),
        "Parse error: Failed to decode base64: GitHub sent file content that is not valid base64"
    );
}

#[tokio::test]
async fn a_429_is_retried_three_times_and_then_is_rate_limited() {
    let server = MockServer::start().await;
    repository_request()
        .respond_with(ResponseTemplate::new(429))
        .expect(RETRIED_ATTEMPTS)
        .mount(&server)
        .await;

    let error = served(&server).verify(&plain()).await.unwrap_err();

    assert!(
        matches!(
            error,
            ContextError::RateLimited {
                retry_after_secs: 60
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.to_string(), RATE_LIMITED);
}

async fn recovers_after_one_limited_answer(limited: ResponseTemplate) {
    let server = MockServer::start().await;
    repository_request()
        .respond_with(limited)
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount_repository(&server).await;

    served(&server).verify(&plain()).await.unwrap();

    assert_eq!(requests(&server).await.len(), 2);
}

#[tokio::test]
async fn a_403_with_no_requests_remaining_is_retried() {
    recovers_after_one_limited_answer(
        ResponseTemplate::new(403).insert_header("x-ratelimit-remaining", "0"),
    )
    .await;
}

#[tokio::test]
async fn a_403_with_retry_after_is_retried() {
    recovers_after_one_limited_answer(ResponseTemplate::new(403).insert_header("retry-after", "1"))
        .await;
}

#[tokio::test]
async fn a_403_that_says_rate_limit_is_retried() {
    recovers_after_one_limited_answer(ResponseTemplate::new(403).set_body_json(json!({
        "message": "You have exceeded a secondary rate limit"
    })))
    .await;
}

#[tokio::test]
async fn each_repository_status_keeps_its_zone_error() {
    let cases: [(u16, fn(&ContextError) -> bool, &str); 5] = [
        (
            599,
            adapted,
            "Adapter error for github: API error: 599 <unknown status code>",
        ),
        (
            500,
            adapted,
            "Adapter error for github: API error: 500 Internal Server Error",
        ),
        (
            401,
            adapted,
            "Adapter error for github: API error: 401 Unauthorized",
        ),
        (
            403,
            |error| matches!(error, ContextError::PermissionDenied(_)),
            "Permission denied: Access forbidden - check token permissions",
        ),
        (
            404,
            |error| matches!(error, ContextError::InvalidSourceConfig(_)),
            NOT_FOUND,
        ),
    ];

    for (status, expected, text) in cases {
        let server = MockServer::start().await;
        repository_request()
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount(&server)
            .await;

        let error = served(&server).verify(&plain()).await.unwrap_err();

        assert!(expected(&error), "{status}: {error:?}");
        assert_eq!(error.to_string(), text, "{status}");
    }
}

#[tokio::test]
async fn a_tree_404_fails_the_fetch() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    tree_request(MAIN)
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let error = fetched(&served(&server), &plain(), FetchStrategy::MetadataOnly)
        .await
        .unwrap_err();

    assert!(adapted(&error), "{error:?}");
    assert_eq!(
        error.to_string(),
        "Adapter error for github: Failed to fetch tree: 404 Not Found"
    );
}

#[tokio::test]
async fn a_short_tree_sha_is_a_tree_parse_error_that_quotes_it() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    tree_request(MAIN)
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": "abc123",
            "tree": [blob("README.md", 'b', 10), blob("src/lib.rs", 'c', 10)],
            "truncated": false
        })))
        .mount(&server)
        .await;

    let error = fetched(&served(&server), &plain(), FetchStrategy::MetadataOnly)
        .await
        .unwrap_err();

    assert!(adapted(&error), "{error:?}");
    assert_eq!(
        error.to_string(),
        r#"Adapter error for github: Failed to parse tree: Invalid object name: "abc123""#
    );
}

#[tokio::test]
async fn a_tree_entry_with_no_mode_is_an_unreadable_tree() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    mount_tree(
        &server,
        vec![json!({
            "path": "README.md",
            "type": "blob",
            "sha": object('b'),
            "size": 10
        })],
    )
    .await;

    let error = fetched(&served(&server), &plain(), FetchStrategy::MetadataOnly)
        .await
        .unwrap_err();

    assert!(adapted(&error), "{error:?}");
    assert_eq!(
        error.to_string(),
        "Adapter error for github: Failed to parse tree: GitHub answered in a form this crate cannot read"
    );
}

#[tokio::test]
async fn a_repository_answer_that_is_not_json_is_unreadable_repo_info() {
    let server = MockServer::start().await;
    repository_request()
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&server)
        .await;

    let error = served(&server).verify(&plain()).await.unwrap_err();

    assert!(adapted(&error), "{error:?}");
    assert_eq!(
        error.to_string(),
        "Adapter error for github: Failed to parse repo info: GitHub answered in a form this crate cannot read"
    );
}

#[tokio::test]
async fn an_origin_that_refuses_connections_is_a_request_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    let error = addressed(&format!("http://{address}"))
        .verify(&plain())
        .await
        .unwrap_err();

    assert!(
        matches!(
            &error,
            ContextError::Adapter { source_type, message }
                if source_type == "github" && message.starts_with("Request failed: ")
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_branch_with_a_plus_reaches_github_as_written() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/o/r/git/trees/release(\+|%2B)1$"))
        .and(query_param("recursive", "1"))
        .respond_with(tree(vec![blob("src/lib.rs", 'c', 10)]))
        .expect(1)
        .mount(&server)
        .await;
    contents_request("src/lib.rs", "release+1")
        .respond_with(file("pub fn lib() {}"))
        .expect(1)
        .mount(&server)
        .await;

    let result = fetched(
        &served(&server),
        &source(json!({ "owner": "o", "repo": "r", "branch": "release+1" })),
        FetchStrategy::Full,
    )
    .await
    .unwrap();

    assert_eq!(titles(&result), ["lib.rs"]);
    assert_eq!(result.items[0].content.as_deref(), Some("pub fn lib() {}"));
}

#[tokio::test]
async fn a_dash_led_default_branch_fails_even_with_a_branch_configured() {
    for config in [
        json!({ "owner": "o", "repo": "r" }),
        json!({ "owner": "o", "repo": "r", "branch": "main" }),
    ] {
        let server = MockServer::start().await;
        repository_request()
            .respond_with(repository("-dash"))
            .mount(&server)
            .await;
        tree_request(MAIN)
            .respond_with(tree(vec![]))
            .expect(0)
            .mount(&server)
            .await;
        let adapter = served(&server);
        let configured = source(config);

        for error in [
            adapter.verify(&configured).await.unwrap_err(),
            fetched(&adapter, &configured, FetchStrategy::MetadataOnly)
                .await
                .unwrap_err(),
        ] {
            assert!(adapted(&error), "{error:?}");
            assert_eq!(
                error.to_string(),
                r#"Adapter error for github: Failed to parse repo info: Invalid branch name: "-dash""#
            );
        }
    }
}

#[tokio::test]
async fn a_branch_git_refuses_as_a_reference_is_a_tree_404_unsent() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/o/r/git/trees/"))
        .respond_with(tree(vec![]))
        .expect(0)
        .mount(&server)
        .await;

    let error = fetched(
        &served(&server),
        &source(json!({ "owner": "o", "repo": "r", "branch": "@" })),
        FetchStrategy::MetadataOnly,
    )
    .await
    .unwrap_err();

    assert!(adapted(&error), "{error:?}");
    assert_eq!(
        error.to_string(),
        "Adapter error for github: Failed to fetch tree: 404 Not Found"
    );
}

#[tokio::test]
async fn a_redirect_off_the_origin_is_not_followed() {
    let elsewhere = MockServer::start().await;
    Mock::given(any())
        .respond_with(repository(MAIN))
        .mount(&elsewhere)
        .await;
    let server = MockServer::start().await;
    repository_request()
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}{REPOSITORY}", elsewhere.uri())),
        )
        .mount(&server)
        .await;

    let error = served(&server).verify(&plain()).await.unwrap_err();

    assert!(adapted(&error), "{error:?}");
    assert_eq!(
        error.to_string(),
        "Adapter error for github: API error: 302 Found"
    );
    assert!(requests(&elsewhere).await.is_empty());
}

#[tokio::test]
async fn requests_name_abnegate_vcs_as_the_user_agent() {
    let server = MockServer::start().await;
    repository_request()
        .and(header("user-agent", "abnegate-vcs"))
        .respond_with(repository(MAIN))
        .expect(1)
        .mount(&server)
        .await;

    served(&server).verify(&plain()).await.unwrap();
}

#[tokio::test]
async fn an_empty_or_absent_token_sends_no_authorization() {
    for config in [
        json!({ "owner": "o", "repo": "r", "token": "" }),
        json!({ "owner": "o", "repo": "r" }),
    ] {
        let server = MockServer::start().await;
        mount_repository(&server).await;
        mount_tree(&server, vec![blob("README.md", 'b', 10)]).await;
        let adapter = served(&server);
        let configured = source(config);

        adapter.verify(&configured).await.unwrap();
        fetched(&adapter, &configured, FetchStrategy::MetadataOnly)
            .await
            .unwrap();

        let received = requests(&server).await;
        assert_eq!(received.len(), 3);
        assert!(
            received
                .iter()
                .all(|request| !request.headers.contains_key("authorization")),
            "{received:?}"
        );
    }
}

#[tokio::test]
async fn a_token_that_is_not_a_header_value_is_a_config_error_unsent() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(repository(MAIN))
        .expect(0)
        .mount(&server)
        .await;
    let adapter = served(&server);
    let configured = source(json!({ "owner": "o", "repo": "r", "token": "bad\ntoken" }));

    for error in [
        adapter.verify(&configured).await.unwrap_err(),
        fetched(&adapter, &configured, FetchStrategy::Full)
            .await
            .unwrap_err(),
        adapter.estimate_tokens(&configured).await.unwrap_err(),
        adapter.get_sync_state(&configured).await.unwrap_err(),
    ] {
        assert!(matches!(error, ContextError::Config(_)), "{error:?}");
        assert_eq!(error.to_string(), MALFORMED_TOKEN);
    }
    assert!(requests(&server).await.is_empty());
}

#[tokio::test]
async fn a_well_formed_token_is_sent_as_a_bearer_token() {
    let server = MockServer::start().await;
    repository_request()
        .and(header("authorization", "Bearer ghp_abc"))
        .respond_with(repository(MAIN))
        .expect(1)
        .mount(&server)
        .await;

    served(&server)
        .verify(&source(
            json!({ "owner": "o", "repo": "r", "token": "ghp_abc" }),
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn an_owner_github_would_not_allow_is_not_found_unsent() {
    for owner in ["o r", ".."] {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(repository(MAIN))
            .expect(0)
            .mount(&server)
            .await;

        let error = served(&server)
            .verify(&source(json!({ "owner": owner, "repo": "r" })))
            .await
            .unwrap_err();

        assert!(
            matches!(error, ContextError::InvalidSourceConfig(_)),
            "{owner}: {error:?}"
        );
        assert_eq!(
            error.to_string(),
            format!("Invalid source configuration: Repository not found: {owner}/r")
        );
        assert!(requests(&server).await.is_empty(), "{owner}");
    }
}

#[tokio::test]
async fn answers_without_fields_zone_never_used_are_read() {
    let server = MockServer::start().await;
    repository_request()
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "default_branch": "main"
        })))
        .mount(&server)
        .await;
    mount_tree(&server, vec![blob("README.md", 'b', 10)]).await;
    contents_request("README.md", MAIN)
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": encoded("# Read me")
        })))
        .mount(&server)
        .await;

    let result = fetched(&served(&server), &plain(), FetchStrategy::Full)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["README.md"]);
    assert_eq!(result.items[0].content.as_deref(), Some("# Read me"));
}

#[tokio::test]
async fn a_symbolic_link_blob_is_listed() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    mount_tree(
        &server,
        vec![json!({
            "path": "docs/current.md",
            "mode": "120000",
            "type": "blob",
            "sha": object('b'),
            "size": 12
        })],
    )
    .await;

    let result = fetched(&served(&server), &plain(), FetchStrategy::MetadataOnly)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["current.md"]);
}

#[tokio::test]
async fn a_truncated_tree_is_still_fetched() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    tree_request(MAIN)
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sha": object('a'),
            "tree": [blob("README.md", 'b', 10)],
            "truncated": true
        })))
        .mount(&server)
        .await;
    mount_file(&server, "README.md", "# Read me").await;

    let result = fetched(&served(&server), &plain(), FetchStrategy::Full)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["README.md"]);
    assert_eq!(result.items[0].content.as_deref(), Some("# Read me"));
}

#[tokio::test]
async fn a_file_github_sends_as_empty_content_is_kept_empty() {
    let size = 2_000_000;
    let server = MockServer::start().await;
    mount_repository(&server).await;
    mount_tree(&server, vec![blob("large.txt", 'b', size)]).await;
    contents_request("large.txt", MAIN)
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": "",
            "size": size
        })))
        .mount(&server)
        .await;

    let result = fetched(&served(&server), &plain(), FetchStrategy::Full)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["large.txt"]);
    assert_eq!(result.items[0].content.as_deref(), Some(""));
}

#[tokio::test]
async fn a_repository_answer_past_the_cap_is_oversized_repo_info() {
    let server = MockServer::start().await;
    repository_request()
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(vec![b' '; MAXIMUM_ANSWER_BYTES + 1]),
        )
        .mount(&server)
        .await;

    let error = served(&server).verify(&plain()).await.unwrap_err();

    assert!(adapted(&error), "{error:?}");
    assert_eq!(
        error.to_string(),
        "Adapter error for github: Failed to parse repo info: GitHub's answer was larger than this crate reads"
    );
}

#[tokio::test]
async fn a_path_that_names_a_directory_skips_the_file_and_keeps_the_rest() {
    let server = MockServer::start().await;
    mount_repository(&server).await;
    mount_tree(
        &server,
        vec![blob("README.md", 'b', 10), blob("src/lib.rs", 'c', 10)],
    )
    .await;
    contents_request("README.md", MAIN)
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "name": "nested.md", "path": "README.md/nested.md", "type": "file" }
        ])))
        .mount(&server)
        .await;
    mount_file(&server, "src/lib.rs", "pub fn lib() {}").await;

    let result = fetched(&served(&server), &plain(), FetchStrategy::Full)
        .await
        .unwrap();

    assert_eq!(titles(&result), ["lib.rs"]);
}
