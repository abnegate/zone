//! The console's Pull Requests page: every open pull request across a
//! workspace's GitHub sources, assessed and bucketed, read in detail, and
//! merged only while it is ready and its head is the one the reader saw.

use crate::common::TestClient;
use crate::common::context_database_url;
use crate::common::create_test_router;
use crate::common::create_test_state;
use crate::common::github_app;
use crate::common::test_email;
use crate::common::test_password;

use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;
use axum::http::StatusCode;
use axum::http::header::ACCEPT_RANGES;
use axum::http::header::CONTENT_RANGE;
use axum::http::header::LOCATION;
use axum::http::header::RANGE;
use chrono::SecondsFormat;
use chrono::Utc;
use serde_json::Value;
use serde_json::json;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::Instant;
use uuid::Uuid;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::body_partial_json;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::query_param;
use zone_server::config::Config;
use zone_server::db::sources;
use zone_server::db::workspace_members;
use zone_server::db::workspace_members::WorkspaceRole;
use zone_server::services::github::diff::PER_FILE_DIFF;
use zone_server::services::github::pull_requests::LOG_TAIL;
use zone_server::services::github::throttle::CREDENTIAL_PERMITS;
use zone_server::state::AppState;

const TOKEN: &str = "ghp_pull_requests_page_test";
const OWNER: &str = "acme";
const REPOSITORY: &str = "widgets";
const ENTERPRISE_API: &str = "/api/v3";
const ENTERPRISE_GRAPHQL: &str = "/api/graphql";
const GRAPHQL: &str = "/graphql";
const DIFF_MEDIA_TYPE: &str = "application/vnd.github.diff";
const READY_HEAD: &str = "1111111111111111111111111111111111111111";
const RUNNING_HEAD: &str = "2222222222222222222222222222222222222222";
const FAILED_HEAD: &str = "3333333333333333333333333333333333333333";
const MOVED_HEAD: &str = "4444444444444444444444444444444444444444";
const BASE_HEAD: &str = "5555555555555555555555555555555555555555";
const MERGE_COMMIT: &str = "6666666666666666666666666666666666666666";
const JOB: u64 = 71;
const RUN: u64 = 99;
const BYTES: &str = "bytes";
const STORED_LOG: &str = "/store/job-logs/71.txt";
const LOG_LINE: &str = "2026-11-14T00:10:00Z running the suite\n";
const FAILURE: &str = "error: the build failed\n";
const LONG_LOG_BYTES: usize = 17 * 1024 * 1024;
const SLOW_RELEASES: u64 = 6;
const RELEASE_LOOKUP_DELAY: Duration = Duration::from_millis(300);
const DIFF: &str = "diff --git a/src/lib.rs b/src/lib.rs\nindex 1..2 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n fn main() {}\n+fn added() {}\n";

/// One open pull request GitHub knows about, and how its checks stand.
struct Pull {
    number: u64,
    head: &'static str,
    conclusion: Option<&'static str>,
    threads: Vec<Value>,
}

impl Pull {
    fn ready() -> Self {
        Self::of(1, READY_HEAD, Some("success"))
    }

    fn running() -> Self {
        Self::of(2, RUNNING_HEAD, None)
    }

    fn failed() -> Self {
        Self::of(3, FAILED_HEAD, Some("failure"))
    }

    fn of(number: u64, head: &'static str, conclusion: Option<&'static str>) -> Self {
        Self {
            number,
            head,
            conclusion,
            threads: Vec::new(),
        }
    }
}

/// A stand-in GitHub, at github.com's paths or at an Enterprise server's.
struct Hub {
    server: MockServer,
    api: &'static str,
    graphql: &'static str,
}

impl Hub {
    async fn hosted() -> Self {
        Self {
            server: MockServer::start().await,
            api: "",
            graphql: GRAPHQL,
        }
    }

    async fn enterprise() -> Self {
        Self {
            server: MockServer::start().await,
            api: ENTERPRISE_API,
            graphql: ENTERPRISE_GRAPHQL,
        }
    }

    fn config(&self) -> Config {
        let mut config = github_app::config_for(&self.server);
        config.github_api_url = format!("{}{}", self.server.uri(), self.api);
        config.github_web_url = self.server.uri();
        config.database_url = context_database_url();
        config.comfyui.enabled = false;
        config.web_search.enabled = false;
        config
    }

    fn endpoint(&self, rest: &str) -> String {
        format!("{}/repos/{OWNER}/{REPOSITORY}/{rest}", self.api)
    }

    fn web(&self, rest: &str) -> String {
        format!("{}/{OWNER}/{REPOSITORY}/{rest}", self.server.uri())
    }

    async fn answer(&self, rest: &str, body: Value) {
        Mock::given(method("GET"))
            .and(path(self.endpoint(rest)))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.server)
            .await;
    }

    fn row(&self, pull: &Pull) -> Value {
        json!({
            "node_id": format!("PR_{}", pull.number),
            "number": pull.number,
            "html_url": self.web(&format!("pull/{}", pull.number)),
            "state": "open",
            "title": format!("Change {}", pull.number),
            "draft": false,
            "updated_at": format!("2026-11-14T0{}:00:00Z", pull.number),
            "head": {"ref": format!("feature-{}", pull.number), "sha": pull.head},
            "base": {"ref": "main", "sha": BASE_HEAD},
            "user": {"login": "octocat"},
            "commits": 1,
        })
    }

    async fn list(&self, pulls: &[Pull]) {
        let rows: Vec<Value> = pulls.iter().map(|pull| self.row(pull)).collect();
        self.answer("pulls", json!(rows)).await;
        for pull in pulls {
            self.pull(pull).await;
        }
    }

    async fn pull(&self, pull: &Pull) {
        self.answer(&format!("pulls/{}", pull.number), self.row(pull))
            .await;
        self.answer(
            &format!("issues/{}/comments", pull.number),
            json!([{
                "id": 900 + pull.number,
                "body": format!("**Confidence Score:** 5/5\n_Last reviewed commit: {}_", pull.head),
                "html_url": self.web(&format!("pull/{}#issuecomment-{}", pull.number, 900 + pull.number)),
                "created_at": "2026-11-14T00:30:00Z",
                "user": {"login": "greptile-apps[bot]"},
            }]),
        )
        .await;
        Mock::given(method("POST"))
            .and(path(self.graphql))
            .and(body_partial_json(
                json!({"variables": {"number": pull.number}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
                "repository": {"pullRequest": {
                    "number": pull.number,
                    "reviewThreads": {
                        "pageInfo": {"hasNextPage": false, "endCursor": null},
                        "nodes": pull.threads,
                    },
                }},
            }})))
            .mount(&self.server)
            .await;
        Mock::given(method("GET"))
            .and(path(self.endpoint("actions/runs")))
            .and(query_param("head_sha", pull.head))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"workflow_runs": [], "total_count": 0})),
            )
            .mount(&self.server)
            .await;
        let check = match pull.conclusion {
            Some(conclusion) => json!({"status": "completed", "conclusion": conclusion}),
            None => json!({"status": "in_progress"}),
        };
        let mut run = json!({
            "id": 70 + pull.number,
            "name": "test",
            "head_sha": pull.head,
            "html_url": self.web(&format!("actions/runs/{RUN}/job/{}", 70 + pull.number)),
            "started_at": "2026-11-14T00:10:00Z",
        });
        run.as_object_mut()
            .unwrap()
            .extend(check.as_object().unwrap().clone());
        self.answer(
            &format!("commits/{}/check-runs", pull.head),
            json!({"total_count": 1, "check_runs": [run]}),
        )
        .await;
        self.answer(
            &format!("commits/{}/status", pull.head),
            json!({"statuses": [], "total_count": 0}),
        )
        .await;
    }

    async fn requests_to(&self, endpoint: &str) -> usize {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == endpoint)
            .count()
    }

    async fn ranges_to(&self, endpoint: &str) -> Vec<String> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() == endpoint)
            .filter_map(|request| request.headers.get(RANGE))
            .map(|range| range.to_str().unwrap().to_string())
            .collect()
    }
}

/// A workspace whose owner connected `acme/widgets`, served by a router
/// whose GitHub is `hub`.
struct Board {
    hub: Hub,
    client: TestClient,
    state: AppState,
    token: String,
    workspace: Uuid,
    source: Uuid,
}

impl Board {
    async fn new(hub: Hub) -> Self {
        let config = hub.config();
        Self::configured(hub, config).await
    }

    async fn configured(hub: Hub, config: Config) -> Self {
        let pool = PgPool::connect(&config.database_url)
            .await
            .expect("the test database");
        let state = create_test_state(config, pool);
        let client = TestClient::new(create_test_router(state.clone()));
        let (token, workspace, _) = member(&client).await;
        let encrypted = encrypt_value(&SecretValue::new(TOKEN), state.master_key())
            .expect("an encrypted token");
        let source = sources::create_source(
            state.db(),
            sources::NewSource {
                workspace_id: workspace,
                name: &format!("widgets-{}", Uuid::new_v4()),
                source_type: sources::GITHUB,
                config: json!({"owner": OWNER, "repo": REPOSITORY, "branch": "main"}),
                description: None,
                url: None,
                credentials_encrypted: Some(&encrypted),
                github_installation_id: None,
            },
        )
        .await
        .expect("a GitHub source")
        .id;
        Self {
            hub,
            client,
            state,
            token,
            workspace,
            source,
        }
    }

    fn collection(&self) -> String {
        format!("/api/workspaces/{}/pull-requests", self.workspace)
    }

    fn pull(&self, number: u64) -> String {
        format!("{}/{}/{number}", self.collection(), self.source)
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        self.get_as(uri, &self.token).await
    }

    async fn get_as(&self, uri: &str, token: &str) -> (StatusCode, Value) {
        let response = self.client.get_auth(uri, token).await;
        (response.status, response.json_value())
    }

    async fn listed(&self, query: &str) -> Value {
        let (status, body) = self.get(&format!("{}{query}", self.collection())).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn merge(&self, number: u64, body: Value) -> (StatusCode, Value) {
        self.merge_as(number, body, &self.token).await
    }

    async fn merge_as(&self, number: u64, body: Value, token: &str) -> (StatusCode, Value) {
        let response = self
            .client
            .post_json_auth(&format!("{}/merge", self.pull(number)), &body, token)
            .await;
        (response.status, response.json_value())
    }

    async fn viewer(&self) -> String {
        let (token, user) = registered(&self.client).await;
        workspace_members::add_member(
            self.state.db(),
            self.workspace,
            user,
            WorkspaceRole::Viewer,
            None,
        )
        .await
        .expect("a viewer");
        token
    }
}

async fn registered(client: &TestClient) -> (String, Uuid) {
    let registered = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value();
    let token = registered["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();
    let user = registered["user"]["id"]
        .as_str()
        .expect("registration returns the user")
        .parse()
        .expect("a user id");
    (token, user)
}

/// A user who owns a new organization's workspace: their token, the
/// workspace, and who they are.
async fn member(client: &TestClient) -> (String, Uuid, Uuid) {
    let suffix = Uuid::new_v4();
    let (token, user) = registered(client).await;
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Reviews", "slug": format!("reviews-{suffix}")}),
            &token,
        )
        .await
        .json_value()["organization"]["id"]
        .as_str()
        .expect("an organization")
        .to_string();
    let workspace = client
        .post_json_auth(
            &format!("/api/organizations/{organization}/workspaces"),
            &json!({"name": "Reviews", "slug": format!("reviews-{suffix}")}),
            &token,
        )
        .await
        .json_value()["workspace"]["id"]
        .as_str()
        .expect("a workspace")
        .parse()
        .expect("a workspace id");
    (token, workspace, user)
}

fn numbers(listing: &Value) -> Vec<(u64, String, u64)> {
    listing["pull_requests"]
        .as_array()
        .expect("pull requests")
        .iter()
        .map(|row| {
            (
                row["number"].as_u64().unwrap(),
                row["readiness"].as_str().unwrap().to_string(),
                row["rank"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn open_pull_requests_are_bucketed_counted_and_ranked() {
    let board = Board::new(Hub::hosted().await).await;
    board
        .hub
        .list(&[Pull::failed(), Pull::running(), Pull::ready()])
        .await;

    let listing = board.listed("").await;

    assert_eq!(
        numbers(&listing),
        [
            (1, "ready".to_string(), 1),
            (2, "in_progress".to_string(), 2),
            (3, "not_ready".to_string(), 3),
        ]
    );
    assert_eq!(
        listing["counts"],
        json!({"ready": 1, "in_progress": 1, "not_ready": 1, "total": 3})
    );
    assert_eq!(listing["stale"], json!(false));
    assert_eq!(listing["partial"], json!(false));
    assert_eq!(listing["ttl_secs"], json!(45));
    assert_eq!(listing["refresh_floor_secs"], json!(10));
    assert_eq!(listing["warnings"], json!([]));
    let source = &listing["sources"][0];
    assert_eq!(source["source_id"], json!(board.source));
    assert_eq!(source["repository"], json!("acme/widgets"));
    assert_eq!(
        source["repository_url"],
        json!(format!("{}/{OWNER}/{REPOSITORY}", board.hub.server.uri()))
    );
    assert_eq!(source["freshness"], json!("refreshed"));
    let ready = &listing["pull_requests"][0];
    assert_eq!(ready["source_id"], json!(board.source));
    assert_eq!(ready["url"], json!(board.hub.web("pull/1")));
    assert_eq!(ready["author"], json!("octocat"));
    assert_eq!(ready["head"], json!(READY_HEAD));
    assert_eq!(ready["head_ref"], json!("feature-1"));
    assert_eq!(ready["base_ref"], json!("main"));
    assert_eq!(ready["blockers"], json!([]));
    assert_eq!(ready["review_current"], json!(true));
    assert_eq!(
        ready["check_details"],
        json!([{
            "kind": "check_run",
            "id": 71,
            "name": "test",
            "state": "success",
            "url": board.hub.web(&format!("actions/runs/{RUN}/job/71")),
            "updated_at": "2026-11-14T00:10:00Z",
        }])
    );
    let failed = &listing["pull_requests"][2];
    assert!(
        failed["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker["code"] == "checks_failed"),
        "{failed}"
    );
    assert_eq!(
        board
            .hub
            .requests_to("/repos/acme/widgets/pulls/1/files")
            .await,
        0,
        "a listing never walks a pull request's files"
    );
}

#[tokio::test]
async fn a_refresh_inside_the_floor_is_served_what_was_already_read() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;

    let first = board.listed("").await;
    let floored = board.listed("?refresh=1").await;
    let cached = board.listed("").await;

    assert_eq!(first["sources"][0]["freshness"], json!("refreshed"));
    assert_eq!(floored["sources"][0]["freshness"], json!("floored"));
    assert_eq!(cached["sources"][0]["freshness"], json!("cached"));
    assert_eq!(numbers(&cached), numbers(&first));
    assert_eq!(board.hub.requests_to("/repos/acme/widgets/pulls").await, 1);
}

#[tokio::test]
async fn a_source_github_stops_answering_is_served_stale_with_why() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    board.listed("").await;
    board
        .state
        .github_snapshots()
        .pull_requests
        .invalidate(&board.source);
    board.hub.server.reset().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&board.hub.server)
        .await;

    let stale = board.listed("").await;

    assert_eq!(stale["stale"], json!(true));
    assert_eq!(stale["sources"][0]["freshness"], json!("stale"));
    assert!(
        stale["sources"][0]["warning"]
            .as_str()
            .is_some_and(|warning| warning.contains("500")),
        "{stale}"
    );
    assert_eq!(numbers(&stale), [(1, "ready".to_string(), 1)]);
}

#[tokio::test]
async fn a_source_never_read_is_partial_and_names_why() {
    let board = Board::new(Hub::hosted().await).await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&board.hub.server)
        .await;

    let listing = board.listed("").await;

    assert_eq!(listing["partial"], json!(true));
    assert_eq!(listing["pull_requests"], json!([]));
    assert_eq!(listing["sources"][0]["freshness"], json!("stale"));
    assert_eq!(listing["sources"][0]["generated_at"], json!(null));
    assert!(listing["sources"][0]["warning"].is_string(), "{listing}");
}

#[tokio::test]
async fn a_pull_request_list_is_not_held_behind_a_slow_release_listing_on_its_credential() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    let published = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let releases: Vec<Value> = (1..=SLOW_RELEASES)
        .map(|id| {
            let tag = format!("v1.0.{id}");
            json!({
                "id": id,
                "tag_name": tag,
                "name": tag,
                "html_url": board.hub.web(&format!("releases/tag/{tag}")),
                "published_at": published,
                "draft": false,
                "prerelease": false,
                "body": "",
                "target_commitish": "main",
            })
        })
        .collect();
    board.hub.answer("releases", json!(releases)).await;
    Mock::given(method("GET"))
        .and(path(board.hub.endpoint("actions/runs")))
        .and(query_param("event", "release"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"workflow_runs": [], "total_count": 0}))
                .set_delay(RELEASE_LOOKUP_DELAY),
        )
        .mount(&board.hub.server)
        .await;
    let source = sources::get_source(board.state.db(), board.source, board.workspace)
        .await
        .expect("the source reads")
        .expect("the source exists");
    let permits = board.state.github_snapshots().throttle.credential(&source);
    let _elsewhere = permits
        .acquire_many(u32::try_from(CREDENTIAL_PERMITS - 1).expect("a permit count"))
        .await
        .expect("every permit but one is taken elsewhere");
    let releases_uri = format!("/api/workspaces/{}/releases", board.workspace);

    let (released_at, listed_at) = tokio::join!(
        async {
            let (status, body) = board.get(&releases_uri).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            Instant::now()
        },
        async {
            tokio::time::sleep(RELEASE_LOOKUP_DELAY / 2).await;
            let listing = board.listed("").await;
            assert_eq!(numbers(&listing).len(), 1, "{listing}");
            Instant::now()
        },
    );

    assert!(
        listed_at < released_at,
        "the pull request list waited out the whole release listing"
    );
    let release_reads = board
        .hub
        .server
        .received_requests()
        .await
        .expect("requests are recorded")
        .iter()
        .filter(|request| {
            request
                .url
                .query_pairs()
                .any(|(key, value)| key == "event" && value == "release")
        })
        .count();
    assert_eq!(
        release_reads,
        usize::try_from(SLOW_RELEASES).expect("a release count") + 1,
        "one sweep and one exact read per release"
    );
}

#[tokio::test]
async fn a_pull_request_reads_with_its_open_threads_and_bot_summaries() {
    let board = Board::new(Hub::hosted().await).await;
    let mut pull = Pull::failed();
    let comment_url = board.hub.web("pull/3#discussion_r501");
    pull.threads = vec![
        json!({
            "id": "PRRT_open", "isResolved": false, "isOutdated": false,
            "path": "src/lib.rs", "line": 3,
            "comments": {"pageInfo": {"hasNextPage": false}, "nodes": [{
                "databaseId": 501, "body": "Rename this.", "url": comment_url,
                "createdAt": "2026-11-14T01:00:00Z", "author": {"login": "reviewer"},
            }]},
        }),
        json!({
            "id": "PRRT_done", "isResolved": true, "isOutdated": false,
            "path": "src/lib.rs", "line": 9,
            "comments": {"pageInfo": {"hasNextPage": false}, "nodes": [{
                "databaseId": 502, "body": "Done.", "url": board.hub.web("pull/3#discussion_r502"),
                "createdAt": "2026-11-14T01:00:00Z", "author": {"login": "reviewer"},
            }]},
        }),
    ];
    board.hub.pull(&pull).await;

    let (status, detail) = board.get(&board.pull(3)).await;

    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["pull_request"]["number"], json!(3));
    assert_eq!(detail["pull_request"]["readiness"], json!("not_ready"));
    assert_eq!(detail["pull_request"]["unresolved_threads"], json!(1));
    assert_eq!(detail["pull_request"]["threads_complete"], json!(true));
    assert_eq!(
        board.hub.requests_to(GRAPHQL).await,
        1,
        "the review threads are read once for the threads and the verdict"
    );
    assert_eq!(
        detail["pull_request"]["check_details"][0]["id"],
        json!(73),
        "an Actions check run offers its job"
    );
    assert_eq!(
        detail["threads"],
        json!([{
            "id": "PRRT_open",
            "path": "src/lib.rs",
            "line": 3,
            "outdated": false,
            "url": comment_url,
            "comments": [{
                "id": 501,
                "author": "reviewer",
                "body": "Rename this.",
                "url": comment_url,
                "created_at": "2026-11-14T01:00:00Z",
            }],
        }])
    );
    assert_eq!(
        detail["comments"],
        json!([{
            "reviewer": "greptile-apps",
            "body": format!("**Confidence Score:** 5/5\n_Last reviewed commit: {FAILED_HEAD}_"),
            "url": board.hub.web("pull/3#issuecomment-903"),
            "created_at": "2026-11-14T00:30:00Z",
        }])
    );
}

#[tokio::test]
async fn a_diff_is_parsed_for_the_head_the_reader_saw() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.pull(&Pull::ready()).await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/1"))
        .and(header("Accept", DIFF_MEDIA_TYPE))
        .respond_with(ResponseTemplate::new(200).set_body_string(DIFF))
        .with_priority(1)
        .mount(&board.hub.server)
        .await;

    let (status, body) = board
        .get(&format!("{}/diff?head={READY_HEAD}", board.pull(1)))
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let diff = &body["diff"];
    assert_eq!(diff["number"], json!(1));
    assert_eq!(diff["head"], json!(READY_HEAD));
    assert_eq!(diff["base"], json!("main"));
    assert_eq!(diff["complete"], json!(true));
    assert_eq!(diff["warning"], json!(null));
    assert_eq!(diff["files"][0]["path"], json!("src/lib.rs"));
    assert_eq!(
        diff["files"][0]["blob_url"],
        json!(board.hub.web(&format!("blob/{READY_HEAD}/src/lib.rs")))
    );
    assert_eq!(
        diff["files"][0]["hunks"][0]["lines"][1],
        json!({"kind": "addition", "content": "fn added() {}", "old_line": null, "new_line": 2})
    );

    let (status, moved) = board
        .get(&format!("{}/diff?head={MOVED_HEAD}", board.pull(1)))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{moved}");
    assert_eq!(moved["success"], json!(false));
    assert_eq!(moved["code"], json!("head_moved"));
    assert_eq!(moved["current_head"], json!(READY_HEAD));

    let (status, _) = board
        .get(&format!("{}/diff?head=main", board.pull(1)))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_diff_github_will_not_render_is_read_file_by_file() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.pull(&Pull::ready()).await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/1"))
        .and(header("Accept", DIFF_MEDIA_TYPE))
        .respond_with(ResponseTemplate::new(406).set_body_json(json!({
            "message": "Sorry, the diff exceeded the maximum number of lines (20000)",
        })))
        .with_priority(1)
        .mount(&board.hub.server)
        .await;
    board
        .hub
        .answer(
            "pulls/1/files",
            json!([{"filename": "src/basket.rs", "previous_filename": "src/cart.rs",
                    "status": "renamed", "additions": 1, "deletions": 1,
                    "patch": "@@ -1 +1 @@\n-cart\n+basket"}]),
        )
        .await;

    let (status, body) = board
        .get(&format!("{}/diff?head={READY_HEAD}", board.pull(1)))
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let whole: usize = board
        .hub
        .server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| {
            request.url.path() == "/repos/acme/widgets/pulls/1"
                && request
                    .headers
                    .get("accept")
                    .is_some_and(|accept| accept == DIFF_MEDIA_TYPE)
        })
        .count();
    assert_eq!(whole, 1, "the whole diff was asked for once and refused");
    assert_eq!(
        board
            .hub
            .requests_to("/repos/acme/widgets/pulls/1/files")
            .await,
        1
    );
    let diff = &body["diff"];
    assert_eq!(diff["complete"], json!(false));
    assert_eq!(diff["warning"], json!(PER_FILE_DIFF));
    assert_eq!(diff["files"][0]["status"], json!("renamed"));
    assert_eq!(diff["files"][0]["previous_path"], json!("src/cart.rs"));
    assert_eq!(
        diff["files"][0]["hunks"][0]["lines"],
        json!([
            {"kind": "deletion", "content": "cart", "old_line": 1, "new_line": null},
            {"kind": "addition", "content": "basket", "old_line": null, "new_line": 1},
        ])
    );
}

#[tokio::test]
async fn commits_and_a_commits_diff_are_read_for_the_head_the_reader_saw() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.pull(&Pull::ready()).await;
    board
        .hub
        .answer(
            "pulls/1/commits",
            json!([{
                "sha": READY_HEAD,
                "html_url": board.hub.web(&format!("commit/{READY_HEAD}")),
                "commit": {"message": "feat: widgets", "author": {"name": "Mona Lisa", "date": "2026-11-14T00:00:00Z"}},
                "author": {"login": "octocat"},
            }]),
        )
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/acme/widgets/commits/{READY_HEAD}")))
        .and(header("Accept", DIFF_MEDIA_TYPE))
        .respond_with(ResponseTemplate::new(200).set_body_string(DIFF))
        .mount(&board.hub.server)
        .await;

    let (status, body) = board
        .get(&format!("{}/commits?head={READY_HEAD}", board.pull(1)))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["commits"],
        json!({
            "number": 1,
            "head": READY_HEAD,
            "base": "main",
            "complete": true,
            "commits": [{
                "sha": READY_HEAD,
                "message": "feat: widgets",
                "author_name": "Mona Lisa",
                "author_login": "octocat",
                "authored_at": "2026-11-14T00:00:00Z",
                "url": board.hub.web(&format!("commit/{READY_HEAD}")),
            }],
        })
    );

    let (status, body) = board
        .get(&format!(
            "{}/commits/{READY_HEAD}?head={READY_HEAD}",
            board.pull(1)
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["diff"]["head"], json!(READY_HEAD));
    assert_eq!(body["diff"]["files"][0]["path"], json!("src/lib.rs"));

    let (status, body) = board
        .get(&format!(
            "{}/commits/{FAILED_HEAD}?head={READY_HEAD}",
            board.pull(1)
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    let (status, body) = board
        .get(&format!(
            "{}/commits/{READY_HEAD}?head={MOVED_HEAD}",
            board.pull(1)
        ))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], json!("head_moved"));
}

async fn mock_job(hub: &Hub, head: &str) {
    hub.answer(
        &format!("actions/jobs/{JOB}"),
        json!({
            "id": JOB, "run_id": RUN, "name": "test", "status": "completed",
            "conclusion": "failure", "head_sha": head,
            "html_url": hub.web(&format!("actions/runs/{RUN}/job/{JOB}")),
        }),
    )
    .await;
}

async fn mock_log(hub: &Hub) {
    Mock::given(method("GET"))
        .and(path(hub.endpoint(&format!("actions/jobs/{JOB}/logs"))))
        .respond_with(ResponseTemplate::new(200).set_body_string(FAILURE))
        .mount(&hub.server)
        .await;
}

#[tokio::test]
async fn a_failed_jobs_log_is_read_for_the_pull_requests_head_only() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.pull(&Pull::ready()).await;
    mock_job(&board.hub, READY_HEAD).await;
    mock_log(&board.hub).await;

    let (status, body) = board
        .get(&format!(
            "{}/jobs/{JOB}/log?head={READY_HEAD}",
            board.pull(1)
        ))
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["log"]["job_id"], json!(JOB));
    assert_eq!(body["log"]["run_id"], json!(RUN));
    assert_eq!(body["log"]["head"], json!(READY_HEAD));
    assert_eq!(body["log"]["text"], json!(FAILURE));
    assert_eq!(body["log"]["truncated"], json!(false));

    board.hub.server.reset().await;
    board.hub.pull(&Pull::ready()).await;
    mock_job(&board.hub, FAILED_HEAD).await;
    let (status, body) = board
        .get(&format!(
            "{}/jobs/{JOB}/log?head={READY_HEAD}",
            board.pull(1)
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        board
            .hub
            .requests_to(&format!("/repos/acme/widgets/actions/jobs/{JOB}/logs"))
            .await,
        0,
        "another head's job log is never downloaded"
    );

    let (status, _) = board
        .get(&format!("{}/jobs/0/log?head={READY_HEAD}", board.pull(1)))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

async fn mock_stored_log(hub: &Hub, log: &str, start: usize) {
    Mock::given(method("GET"))
        .and(path(hub.endpoint(&format!("actions/jobs/{JOB}/logs"))))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header(LOCATION, format!("{}{STORED_LOG}", hub.server.uri())),
        )
        .mount(&hub.server)
        .await;
    Mock::given(method("GET"))
        .and(path(STORED_LOG))
        .and(header(RANGE, format!("{BYTES}={start}-")))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header(
                    CONTENT_RANGE,
                    format!("{BYTES} {start}-{}/{}", log.len() - 1, log.len()),
                )
                .set_body_string(&log[start..]),
        )
        .with_priority(1)
        .mount(&hub.server)
        .await;
    Mock::given(method("GET"))
        .and(path(STORED_LOG))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(ACCEPT_RANGES, BYTES)
                .set_body_string(log),
        )
        .mount(&hub.server)
        .await;
}

#[tokio::test]
async fn a_long_jobs_log_is_read_from_its_end_where_the_job_failed() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.pull(&Pull::ready()).await;
    mock_job(&board.hub, READY_HEAD).await;
    let mut log = LOG_LINE.repeat(LONG_LOG_BYTES / LOG_LINE.len());
    log.push_str(FAILURE);
    let tail = usize::try_from(LOG_TAIL).unwrap();
    let start = log.len() - tail;
    mock_stored_log(&board.hub, &log, start).await;

    let (status, body) = board
        .get(&format!(
            "{}/jobs/{JOB}/log?head={READY_HEAD}",
            board.pull(1)
        ))
        .await;

    assert_eq!(status, StatusCode::OK);
    let text = body["log"]["text"].as_str().unwrap();
    assert!(
        text.ends_with(FAILURE),
        "the log is read to its end, where the job failed"
    );
    assert_eq!(text.len(), tail, "only the last {tail} bytes are kept");
    assert!(text == &log[start..], "the kept bytes are the log's last");
    assert_eq!(body["log"]["truncated"], json!(true));
    assert_eq!(
        board.hub.ranges_to(STORED_LOG).await,
        [format!("{BYTES}=-{tail}"), format!("{BYTES}={start}-")],
        "the store's whole answer is left unread and only the end is asked for again"
    );
}

async fn mock_merge(hub: &Hub, number: u64, response: ResponseTemplate) {
    Mock::given(method("PUT"))
        .and(path(hub.endpoint(&format!("pulls/{number}/merge"))))
        .respond_with(response)
        .mount(&hub.server)
        .await;
}

#[tokio::test]
async fn a_ready_pull_request_merges_is_audited_and_its_source_read_again() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    mock_merge(
        &board.hub,
        1,
        ResponseTemplate::new(200).set_body_json(
            json!({"sha": MERGE_COMMIT, "merged": true, "message": "Pull Request successfully merged"}),
        ),
    )
    .await;
    board
        .hub
        .answer(
            "pulls/1/commits",
            json!([
                {
                    "sha": BASE_HEAD,
                    "html_url": board.hub.web(&format!("commit/{BASE_HEAD}")),
                    "commit": {"message": "feat: widgets\n\nCo-authored-by: Ada <ada@example.com>", "author": {"name": "Mona Lisa", "date": "2026-11-14T00:00:00Z"}},
                },
                {
                    "sha": READY_HEAD,
                    "html_url": board.hub.web(&format!("commit/{READY_HEAD}")),
                    "commit": {"message": "fix: gadgets", "author": {"name": "Mona Lisa", "date": "2026-11-14T00:10:00Z"}},
                },
            ]),
        )
        .await;
    board.listed("").await;

    let (status, body) = board
        .merge(1, json!({"expected_head": READY_HEAD, "method": "squash"}))
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["merged"],
        json!({"number": 1, "sha": MERGE_COMMIT, "admin": false, "url": board.hub.web("pull/1")})
    );
    let sent = board
        .hub
        .server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|request| request.method.as_str() == "PUT")
        .expect("the merge was asked for");
    let asked: Value = serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(asked["sha"], json!(READY_HEAD));
    assert_eq!(asked["merge_method"], json!("squash"));
    assert_eq!(asked["commit_title"], json!("Change 1 (#1)"));
    assert_eq!(
        asked["commit_message"],
        json!("* feat: widgets\n\n* fix: gadgets\n\nCo-authored-by: Ada <ada@example.com>"),
        "a squash keeps every commit and its co-authors"
    );

    let audited: Vec<(String, String, Option<Uuid>, Option<Value>)> = sqlx::query_as(
        "SELECT action, resource_type, resource_id, new_values FROM audit_logs \
         WHERE workspace_id = $1 ORDER BY created_at",
    )
    .bind(board.workspace)
    .fetch_all(board.state.db())
    .await
    .expect("the audit log reads");
    let merged: Vec<_> = audited
        .iter()
        .filter(|(action, ..)| action == "pull_request.merged")
        .collect();
    assert_eq!(merged.len(), 1, "{audited:?}");
    assert_eq!(merged[0].1, "pull_request");
    assert_eq!(merged[0].2, Some(board.source));
    let values = merged[0].3.as_ref().expect("what was merged");
    assert_eq!(values["number"], json!(1));
    assert_eq!(values["sha"], json!(MERGE_COMMIT));
    assert_eq!(values["method"], json!("squash"));

    let again = board.listed("").await;
    assert_eq!(again["sources"][0]["freshness"], json!("refreshed"));
    assert_eq!(board.hub.requests_to("/repos/acme/widgets/pulls").await, 2);
}

#[tokio::test]
async fn a_merge_is_refused_with_a_code_for_each_reason_it_cannot_go_ahead() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready(), Pull::failed()]).await;

    let (status, body) = board.merge(1, json!({"expected_head": MOVED_HEAD})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], json!("head_moved"));
    assert_eq!(body["current_head"], json!(READY_HEAD));

    let (status, body) = board.merge(3, json!({"expected_head": FAILED_HEAD})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], json!("not_ready"));
    assert!(
        body["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker["code"] == "checks_failed"),
        "{body}"
    );

    mock_merge(
        &board.hub,
        1,
        ResponseTemplate::new(422)
            .set_body_json(json!({"message": "Pull Request is not mergeable"})),
    )
    .await;
    let (status, body) = board.merge(1, json!({"expected_head": READY_HEAD})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], json!("conflict"));
    assert!(body.get("current_head").is_none(), "{body}");

    assert_eq!(
        merges(&board.hub).await,
        1,
        "only a ready pull request at its head is merged"
    );
}

async fn merges(hub: &Hub) -> usize {
    hub.server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.method.as_str() == "PUT")
        .count()
}

#[tokio::test]
async fn a_head_that_moves_while_merging_is_refused_with_the_head_read_again() {
    let board = Board::new(Hub::hosted().await).await;
    let ready = Pull::ready();
    let moved = Pull::of(1, MOVED_HEAD, Some("success"));
    for (row, priority, reads) in [
        (board.hub.row(&ready), 1, Some(1)),
        (board.hub.row(&moved), 2, None),
    ] {
        let mock = Mock::given(method("GET"))
            .and(path("/repos/acme/widgets/pulls/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(row))
            .with_priority(priority);
        match reads {
            Some(times) => mock.up_to_n_times(times),
            None => mock,
        }
        .mount(&board.hub.server)
        .await;
    }
    board.hub.pull(&ready).await;
    mock_merge(
        &board.hub,
        1,
        ResponseTemplate::new(409).set_body_json(
            json!({"message": "Head branch was modified. Review and try the merge again."}),
        ),
    )
    .await;

    let (status, body) = board.merge(1, json!({"expected_head": READY_HEAD})).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], json!("head_moved"));
    assert_eq!(body["current_head"], json!(MOVED_HEAD));
}

#[tokio::test]
async fn branch_protection_refusing_a_merge_is_refused_as_protected() {
    let hub = Hub::hosted().await;
    let mut config = hub.config();
    config.github.admin_merge = false;
    let board = Board::configured(hub, config).await;
    board.hub.list(&[Pull::ready()]).await;
    mock_merge(
        &board.hub,
        1,
        ResponseTemplate::new(405)
            .set_body_json(json!({"message": "Required status check \"deploy\" is expected."})),
    )
    .await;

    let (status, body) = board.merge(1, json!({"expected_head": READY_HEAD})).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], json!("protected"));
    assert!(
        !body.to_string().contains("deploy"),
        "GitHub's words are not repeated: {body}"
    );
}

#[tokio::test]
async fn github_failing_to_merge_is_a_bad_gateway() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    mock_merge(&board.hub, 1, ResponseTemplate::new(503)).await;

    let (status, body) = board.merge(1, json!({"expected_head": READY_HEAD})).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["success"], json!(false));
    assert!(body.get("code").is_none(), "{body}");
    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE workspace_id = $1 AND action = 'pull_request.merged'",
    )
    .bind(board.workspace)
    .fetch_one(board.state.db())
    .await
    .unwrap();
    assert_eq!(audited, 0, "a merge that failed is never audited");
}

#[tokio::test]
async fn another_workspace_can_neither_read_nor_reach_these_pull_requests() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    let (outsider, elsewhere, _) = member(&board.client).await;

    for uri in [
        board.collection(),
        board.pull(1),
        format!("{}/diff?head={READY_HEAD}", board.pull(1)),
        format!("{}/commits?head={READY_HEAD}", board.pull(1)),
    ] {
        let (status, body) = board.get_as(&uri, &outsider).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{uri}: {body}");
    }
    let (status, body) = board
        .merge_as(1, json!({"expected_head": READY_HEAD}), &outsider)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");

    let smuggled = format!(
        "/api/workspaces/{elsewhere}/pull-requests/{}/1",
        board.source
    );
    let (status, body) = board.get_as(&smuggled, &outsider).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, listing) = board
        .get_as(
            &format!("/api/workspaces/{elsewhere}/pull-requests"),
            &outsider,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    assert_eq!(listing["sources"], json!([]));
    assert_eq!(listing["pull_requests"], json!([]));
}

#[tokio::test]
async fn a_viewer_reads_pull_requests_but_cannot_merge_them() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    mock_merge(
        &board.hub,
        1,
        ResponseTemplate::new(200).set_body_json(json!({"sha": MERGE_COMMIT, "merged": true})),
    )
    .await;
    let viewer = board.viewer().await;

    let (status, listing) = board.get_as(&board.collection(), &viewer).await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    assert_eq!(listing["counts"]["total"], json!(1));

    let (status, body) = board
        .merge_as(1, json!({"expected_head": READY_HEAD}), &viewer)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(merges(&board.hub).await, 0);
}

#[tokio::test]
async fn an_enterprise_server_is_read_under_its_api_and_graphql_paths() {
    let board = Board::new(Hub::enterprise().await).await;
    board.hub.list(&[Pull::ready()]).await;

    let listing = board.listed("").await;

    assert_eq!(numbers(&listing), [(1, "ready".to_string(), 1)]);
    assert_eq!(
        listing["sources"][0]["repository_url"],
        json!(format!("{}/acme/widgets", board.hub.server.uri()))
    );
    assert_eq!(
        board
            .hub
            .requests_to("/api/v3/repos/acme/widgets/pulls")
            .await,
        1
    );
    assert!(board.hub.requests_to(ENTERPRISE_GRAPHQL).await > 0);
    assert_eq!(board.hub.requests_to(GRAPHQL).await, 0);
    assert_eq!(board.hub.requests_to("/repos/acme/widgets/pulls").await, 0);
    assert_eq!(
        listing["pull_requests"][0]["check_details"][0]["id"],
        json!(71),
        "an Enterprise Actions job is offered at the Enterprise origin"
    );

    mock_job(&board.hub, READY_HEAD).await;
    mock_log(&board.hub).await;
    let (status, body) = board
        .get(&format!(
            "{}/jobs/{JOB}/log?head={READY_HEAD}",
            board.pull(1)
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["log"]["run_id"], json!(RUN));
    assert_eq!(body["log"]["text"], json!(FAILURE));
}

async fn writer(board: &Board, role: WorkspaceRole, global: &str) -> String {
    let email = test_email();
    let password = test_password();
    let registered = board
        .client
        .post_json(
            "/api/auth/register",
            &json!({"email": email, "password": password}),
        )
        .await
        .json_value();
    let user: Uuid = registered["user"]["id"]
        .as_str()
        .expect("registration returns the user")
        .parse()
        .expect("a user id");
    sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
        .bind(user)
        .execute(board.state.db())
        .await
        .expect("the user's roles are cleared");
    zone_server::db::users::assign_user_role(board.state.db(), user, global)
        .await
        .expect("the global role is assigned");
    workspace_members::add_member(board.state.db(), board.workspace, user, role, None)
        .await
        .expect("a workspace member");
    board
        .client
        .post_json(
            "/api/auth/login",
            &json!({"email": email, "password": password}),
        )
        .await
        .json_value()["access_token"]
        .as_str()
        .expect("login returns an access token")
        .to_string()
}

async fn protected(hub: &Hub) {
    mock_merge(
        hub,
        1,
        ResponseTemplate::new(405)
            .set_body_json(json!({"message": "Required status check \"deploy\" is expected."})),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(hub.graphql))
        .and(body_partial_json(
            json!({"variables": {"input": {"pullRequestId": "PR_1"}}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "mergePullRequest": {"pullRequest": {"merged": true, "mergeCommit": {"oid": MERGE_COMMIT}}},
        }})))
        .mount(&hub.server)
        .await;
}

async fn administrator_merges(hub: &Hub) -> usize {
    hub.server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| {
            request.url.path() == hub.graphql
                && String::from_utf8_lossy(&request.body).contains("mergePullRequest")
        })
        .count()
}

#[tokio::test]
async fn only_a_workspace_administrator_merges_past_protection_and_only_when_allowed() {
    let hub = Hub::hosted().await;
    let mut config = hub.config();
    config.github.admin_merge = true;
    let board = Board::configured(hub, config).await;
    board.hub.list(&[Pull::ready()]).await;
    protected(&board.hub).await;
    let member = writer(&board, WorkspaceRole::Member, "user").await;

    let (status, body) = board
        .merge_as(1, json!({"expected_head": READY_HEAD}), &member)
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], json!("protected"));
    assert_eq!(
        administrator_merges(&board.hub).await,
        0,
        "a member's merge never asks for the administrator merge"
    );

    let (status, body) = board.merge(1, json!({"expected_head": READY_HEAD})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["merged"]["admin"], json!(true));
    assert_eq!(body["merged"]["sha"], json!(MERGE_COMMIT));
    assert_eq!(administrator_merges(&board.hub).await, 1);
}

#[tokio::test]
async fn an_administrator_merge_is_never_asked_for_unless_the_server_allows_it() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    protected(&board.hub).await;

    let (status, body) = board.merge(1, json!({"expected_head": READY_HEAD})).await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], json!("protected"));
    assert_eq!(administrator_merges(&board.hub).await, 0);
}

#[tokio::test]
async fn a_writer_without_the_merge_permission_is_refused() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    mock_merge(
        &board.hub,
        1,
        ResponseTemplate::new(200).set_body_json(json!({"sha": MERGE_COMMIT, "merged": true})),
    )
    .await;
    let reader = writer(&board, WorkspaceRole::Admin, "viewer").await;

    let (status, listing) = board.get_as(&board.collection(), &reader).await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    let (status, body) = board
        .merge_as(1, json!({"expected_head": READY_HEAD}), &reader)
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("pull_requests:update")),
        "{body}"
    );
    assert_eq!(merges(&board.hub).await, 0);
}

async fn reached(hub: &Hub, verb: &str) {
    while !hub
        .server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .any(|request| request.method.as_str() == verb)
    {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

async fn audited_merges(board: &Board) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM audit_logs WHERE workspace_id = $1 AND action = 'pull_request.merged'",
    )
    .bind(board.workspace)
    .fetch_one(board.state.db())
    .await
    .unwrap()
}

#[tokio::test]
async fn a_merge_whose_caller_hangs_up_still_finishes_and_is_audited() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    mock_merge(
        &board.hub,
        1,
        ResponseTemplate::new(200)
            .set_body_json(json!({"sha": MERGE_COMMIT, "merged": true}))
            .set_delay(std::time::Duration::from_millis(500)),
    )
    .await;

    let answered = tokio::select! {
        answered = board.merge(1, json!({"expected_head": READY_HEAD})) => Some(answered),
        () = reached(&board.hub, "PUT") => None,
    };
    assert!(
        answered.is_none(),
        "the caller hangs up while GitHub merges"
    );

    let mut audited = 0;
    for _ in 0..100 {
        audited = audited_merges(&board).await;
        if audited > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(audited, 1, "the merge finished and was audited");
    assert_eq!(merges(&board.hub).await, 1);
}

#[tokio::test]
async fn an_explicit_refresh_reassesses_only_what_changed() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    board.listed("").await;
    board
        .state
        .github_snapshots()
        .pull_requests
        .invalidate(&board.source);

    let refreshed = board.listed("?refresh=1").await;

    assert_eq!(refreshed["sources"][0]["freshness"], json!("refreshed"));
    assert_eq!(numbers(&refreshed), [(1, "ready".to_string(), 1)]);
    assert_eq!(board.hub.requests_to("/repos/acme/widgets/pulls").await, 2);
    assert_eq!(
        board.hub.requests_to("/repos/acme/widgets/pulls/1").await,
        1,
        "a settled, unchanged pull request is not assessed again"
    );
}

#[tokio::test]
async fn a_rate_limited_assessment_stops_the_source_and_backs_off() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready(), Pull::running()]).await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/widgets/pulls/1"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({"message": "slow down"})))
        .with_priority(1)
        .mount(&board.hub.server)
        .await;

    let limited = board.listed("").await;

    assert_eq!(limited["pull_requests"], json!([]), "{limited}");
    assert_eq!(limited["sources"][0]["generated_at"], json!(null));
    assert!(
        limited["sources"][0]["warning"]
            .as_str()
            .is_some_and(|warning| warning.contains("429")),
        "{limited}"
    );
    let asked = board.hub.requests_to("/repos/acme/widgets/pulls").await;

    let held = board.listed("?refresh=1").await;
    assert_eq!(held["sources"][0]["freshness"], json!("stale"));
    assert_eq!(
        board.hub.requests_to("/repos/acme/widgets/pulls").await,
        asked,
        "GitHub is not asked again while the rate limit backs off"
    );
}

#[tokio::test]
async fn a_credential_github_refuses_is_a_bad_gateway() {
    let board = Board::new(Hub::hosted().await).await;
    Mock::given(wiremock::matchers::any())
        .respond_with(
            ResponseTemplate::new(401).set_body_json(json!({"message": "Bad credentials"})),
        )
        .mount(&board.hub.server)
        .await;

    let (status, body) = board.get(&board.pull(1)).await;

    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["success"], json!(false));
}

#[tokio::test]
async fn a_moved_head_drops_the_cached_listing() {
    let board = Board::new(Hub::hosted().await).await;
    board.hub.list(&[Pull::ready()]).await;
    board.listed("").await;

    let (status, body) = board.merge(1, json!({"expected_head": MOVED_HEAD})).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], json!("head_moved"));

    let again = board.listed("").await;
    assert_eq!(again["sources"][0]["freshness"], json!("refreshed"));
}

#[tokio::test]
async fn a_merged_or_closed_pull_request_is_refused_for_what_it_is() {
    let board = Board::new(Hub::hosted().await).await;
    for (number, state, merged, code) in [
        (8, "closed", true, "merged"),
        (9, "closed", false, "closed"),
    ] {
        let mut row = board
            .hub
            .row(&Pull::of(number, READY_HEAD, Some("success")));
        row["state"] = json!(state);
        row["merged"] = json!(merged);
        board.hub.answer(&format!("pulls/{number}"), row).await;

        let (status, body) = board
            .merge(number, json!({"expected_head": READY_HEAD}))
            .await;

        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["code"], json!(code));
    }
    assert_eq!(merges(&board.hub).await, 0);
}

#[tokio::test]
async fn a_malformed_request_is_refused_in_the_api_shape() {
    let board = Board::new(Hub::hosted().await).await;

    for uri in [
        format!("{}/diff", board.pull(1)),
        format!("{}/{}/not-a-number", board.collection(), board.source),
        format!("{}?refresh=1&refresh=2", board.collection()),
    ] {
        let (status, body) = board.get(&uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_eq!(body["success"], json!(false), "{uri}");
        assert!(body["error"].is_string(), "{uri}: {body}");
    }
    let (status, body) = board.merge(1, json!({"method": "squash"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["success"], json!(false));
}
