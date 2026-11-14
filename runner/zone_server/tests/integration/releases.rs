use crate::common::github_app;
use crate::common::{
    TestClient, context_database_url, create_test_router, create_test_state, test_email,
    test_password,
};

use abnegate_secret::SecretValue;
use abnegate_secret::encrypt_value;
use axum::http::StatusCode;
use chrono::{Duration as Interval, SecondsFormat, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};
use zone_server::db::sources;
use zone_server::db::workspace_members::{self, WorkspaceRole};
use zone_server::state::AppState;

const GITHUB_TOKEN: &str = "ghp_releases_test";
const REPOSITORY: &str = "/repos/acme/widgets";
const ENTERPRISE_API: &str = "/api/v3";
const WEB: &str = "https://github.com/acme/widgets";
const TARGET: &str = "1111111111111111111111111111111111111111";
const TAG_OBJECT: &str = "2222222222222222222222222222222222222222";
const OTHER_OBJECT: &str = "3333333333333333333333333333333333333333";
const BUILT: &str = "4444444444444444444444444444444444444444";
const LATEST: &str = "v1.4.0";
const NEXT: &str = "v1.4.1";
const PUBLISHED_ID: u64 = 90;
const DRAFT_ID: u64 = 91;
const MARKER: &str = "<!-- zone-release:";
const BRANCH_FILTER: &str = "branch";
const CREATED_FILTER: &str = "created";
const FRESH: &str = "v1.5.0";
const FRESH_ID: u64 = 92;
const NOTES: &str = "## What's Changed\n\
    * Add widgets by @octo in https://github.com/acme/widgets/pull/12\n\
    * Fix gadgets by @cat in https://github.com/acme/widgets/pull/13\n\
    \n\
    ## New Contributors\n\
    * @cat made their first contribution in https://github.com/acme/widgets/pull/13\n\
    \n\
    **Full Changelog**: https://github.com/acme/widgets/compare/v1.3.0...v1.4.0";

struct Releases {
    github: MockServer,
    client: TestClient,
    state: AppState,
    pool: PgPool,
    token: String,
    workspace: Uuid,
    source: Uuid,
    api: &'static str,
}

impl Releases {
    async fn new() -> Self {
        Self::serving("", false).await
    }

    async fn enterprise() -> Self {
        Self::serving(ENTERPRISE_API, true).await
    }

    async fn serving(api: &'static str, enterprise: bool) -> Self {
        let github = MockServer::start().await;
        let database = context_database_url();
        let pool = PgPool::connect(&database).await.expect("the test database");
        let mut config = github_app::config_for(&github);
        config.github_api_url = format!("{}{api}", github.uri());
        if enterprise {
            config.github_web_url = github.uri();
        }
        config.database_url = database;
        config.comfyui.enabled = false;
        config.web_search.enabled = false;
        let state = create_test_state(config, pool.clone());
        let client = TestClient::new(create_test_router(state.clone()));
        let (token, workspace) = member(&client).await;
        let encrypted = encrypt_value(&SecretValue::new(GITHUB_TOKEN), state.master_key())
            .expect("an encrypted token");
        let source = sources::create_source(
            state.db(),
            sources::NewSource {
                workspace_id: workspace,
                name: &format!("widgets-{}", Uuid::new_v4()),
                source_type: "github",
                config: json!({"owner": "acme", "repo": "widgets", "branch": "main"}),
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
            github,
            client,
            state,
            pool,
            token,
            workspace,
            source,
            api,
        }
    }

    fn route(&self, route: &str) -> String {
        format!("{}{REPOSITORY}{route}", self.api)
    }

    fn releases(&self) -> String {
        format!("/api/workspaces/{}/releases", self.workspace)
    }

    fn pipeline(&self, release_id: u64) -> String {
        format!("{}/{}/{release_id}/pipeline", self.releases(), self.source)
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        self.get_as(uri, &self.token).await
    }

    async fn get_as(&self, uri: &str, token: &str) -> (StatusCode, Value) {
        let response = self.client.get_auth(uri, token).await;
        (response.status, response.json_value())
    }

    async fn post(&self, uri: &str, body: &Value) -> (StatusCode, Value) {
        self.post_as(uri, body, &self.token).await
    }

    async fn post_as(&self, uri: &str, body: &Value, token: &str) -> (StatusCode, Value) {
        let response = self.client.post_json_auth(uri, body, token).await;
        let status = response.status;
        let text = response.text();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    fn proposal(&self, tag: &str, expected: Option<&str>) -> Value {
        json!({"source_id": self.source, "tag": tag, "expected_latest_tag": expected})
    }

    async fn previewed(&self) -> Value {
        let (status, body) = self
            .post(
                &format!("{}/preview", self.releases()),
                &self.proposal(NEXT, Some(LATEST)),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["preview"].clone()
    }

    async fn create(&self, digest: &str) -> (StatusCode, Value) {
        self.create_as(digest, &self.token).await
    }

    async fn create_as(&self, digest: &str, token: &str) -> (StatusCode, Value) {
        let mut body = self.proposal(NEXT, Some(LATEST));
        body["prerelease"] = json!(false);
        body["digest"] = json!(digest);
        self.post_as(&self.releases(), &body, token).await
    }

    async fn viewer(&self) -> String {
        let registered = self
            .client
            .post_json(
                "/api/auth/register",
                &json!({"email": test_email(), "password": test_password()}),
            )
            .await
            .json_value();
        let user: Uuid = registered["user"]["id"]
            .as_str()
            .expect("a viewer id")
            .parse()
            .expect("a user id");
        workspace_members::add_member(
            self.state.db(),
            self.workspace,
            user,
            WorkspaceRole::Viewer,
            None,
        )
        .await
        .expect("a viewer");
        registered["access_token"]
            .as_str()
            .expect("a viewer token")
            .to_string()
    }

    async fn writer_in_role(&self, global: &str) -> String {
        let email = test_email();
        let password = test_password();
        let registered = self
            .client
            .post_json(
                "/api/auth/register",
                &json!({"email": email, "password": password}),
            )
            .await
            .json_value();
        let user: Uuid = registered["user"]["id"]
            .as_str()
            .expect("a user id")
            .parse()
            .expect("a user id");
        sqlx::query("DELETE FROM user_roles WHERE user_id = $1")
            .bind(user)
            .execute(&self.pool)
            .await
            .expect("the user's roles are cleared");
        zone_server::db::users::assign_user_role(&self.pool, user, global)
            .await
            .expect("the global role is assigned");
        workspace_members::add_member(
            self.state.db(),
            self.workspace,
            user,
            WorkspaceRole::Admin,
            None,
        )
        .await
        .expect("a workspace admin");
        self.client
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

    async fn requests(&self) -> Vec<Request> {
        self.github
            .received_requests()
            .await
            .expect("requests are recorded")
    }

    async fn sent(&self, verb: &str, route: &str) -> Vec<Request> {
        self.requests()
            .await
            .into_iter()
            .filter(|request| request.method.as_str() == verb && request.url.path() == route)
            .collect()
    }

    async fn answer(&self, verb: &str, route: &str, response: ResponseTemplate) {
        Mock::given(method(verb))
            .and(path(self.route(route)))
            .respond_with(response)
            .mount(&self.github)
            .await;
    }

    async fn answer_first(&self, verb: &str, route: &str, status: u16, body: Value) {
        Mock::given(method(verb))
            .and(path(self.route(route)))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .with_priority(1)
            .mount(&self.github)
            .await;
    }

    async fn answer_json(&self, verb: &str, route: &str, status: u16, body: Value) {
        self.answer(
            verb,
            route,
            ResponseTemplate::new(status).set_body_json(body),
        )
        .await;
    }

    async fn ready(&self) {
        self.answer_json("GET", "/tags", 200, json!([tag_row(LATEST)]))
            .await;
        self.answer_json("GET", "/releases", 200, json!([])).await;
        self.answer_json("GET", "", 200, json!({"default_branch": "main"}))
            .await;
        self.answer_json("GET", "/commits/main", 200, json!({"sha": TARGET}))
            .await;
        self.answer_json(
            "GET",
            &format!("/git/ref/tags/{NEXT}"),
            404,
            json!({"message": "Not Found"}),
        )
        .await;
        self.answer_json(
            "POST",
            "/releases/generate-notes",
            200,
            json!({"name": NEXT, "body": NOTES}),
        )
        .await;
    }

    async fn writable(&self) {
        self.writable_as(NEXT).await;
    }

    async fn writable_as(&self, tag: &str) {
        self.answer_json("POST", "/git/tags", 201, json!({"sha": TAG_OBJECT}))
            .await;
        self.answer_json(
            "POST",
            "/git/refs",
            201,
            json!({"ref": format!("refs/tags/{tag}"), "object": {"sha": TAG_OBJECT, "type": "tag"}}),
        )
        .await;
        self.answer_json(
            "POST",
            "/releases",
            201,
            release(DRAFT_ID, tag, None, true, NOTES),
        )
        .await;
        let published = release(DRAFT_ID, tag, Some(&ago(0)), false, NOTES);
        self.answer_json(
            "PATCH",
            &format!("/releases/{DRAFT_ID}"),
            200,
            published.clone(),
        )
        .await;
        self.answer_json("GET", &format!("/releases/{DRAFT_ID}"), 200, published)
            .await;
    }

    async fn runs(&self, runs: Vec<Value>) {
        let total = runs.len();
        self.answer_json(
            "GET",
            "/actions/runs",
            200,
            json!({"workflow_runs": runs, "total_count": total}),
        )
        .await;
    }

    async fn run_reads(&self, filter: &str, value: Option<&str>) -> usize {
        self.sent("GET", &format!("{REPOSITORY}/actions/runs"))
            .await
            .iter()
            .filter(|request| {
                request
                    .url
                    .query_pairs()
                    .any(|(key, read)| key == filter && value.is_none_or(|value| read == value))
            })
            .count()
    }

    async fn exact_reads(&self, tag: &str) -> usize {
        self.run_reads(BRANCH_FILTER, Some(tag)).await
    }

    async fn sweeps(&self) -> usize {
        self.run_reads(CREATED_FILTER, None).await
    }

    async fn relisted(&self, query: &str) -> Value {
        self.state
            .github_snapshots()
            .releases
            .invalidate(&self.source);
        let (status, body) = self.get(&format!("{}{query}", self.releases())).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn never_deleted(&self) {
        assert!(
            self.requests()
                .await
                .iter()
                .all(|request| request.method.as_str() != "DELETE"),
            "nothing is ever deleted"
        );
    }
}

async fn member(client: &TestClient) -> (String, Uuid) {
    let suffix = Uuid::new_v4();
    let token = client
        .post_json(
            "/api/auth/register",
            &json!({"email": test_email(), "password": test_password()}),
        )
        .await
        .json_value()["access_token"]
        .as_str()
        .expect("registration returns an access token")
        .to_string();
    let organization = client
        .post_json_auth(
            "/api/organizations",
            &json!({"name": "Releases", "slug": format!("releases-{suffix}")}),
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
            &json!({"name": "Releases", "slug": format!("releases-{suffix}")}),
            &token,
        )
        .await
        .json_value()["workspace"]["id"]
        .as_str()
        .expect("a workspace")
        .parse()
        .expect("a workspace id");
    (token, workspace)
}

fn ago(days: i64) -> String {
    before(Interval::days(days))
}

fn hours_ago(hours: i64) -> String {
    before(Interval::hours(hours))
}

fn before(interval: Interval) -> String {
    (Utc::now() - interval).to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn pipeline_of<'a>(listing: &'a Value, tag: &str) -> &'a Value {
    &listing["releases"]
        .as_array()
        .expect("releases")
        .iter()
        .find(|release| release["tag"] == tag)
        .unwrap_or_else(|| panic!("{tag} is listed: {listing}"))["pipeline"]
}

fn tag_row(name: &str) -> Value {
    json!({
        "name": name,
        "commit": {"sha": BUILT, "url": format!("https://api.github.com{REPOSITORY}/commits/{BUILT}")},
    })
}

fn release(id: u64, tag: &str, published_at: Option<&str>, draft: bool, body: &str) -> Value {
    json!({
        "id": id,
        "tag_name": tag,
        "name": tag,
        "html_url": format!("{WEB}/releases/tag/{tag}"),
        "published_at": published_at,
        "draft": draft,
        "prerelease": false,
        "body": body,
        "target_commitish": "main",
    })
}

fn run(id: u64, tag: &str, conclusion: &str) -> Value {
    let started = ago(0);
    json!({
        "id": id,
        "workflow_id": 3,
        "run_attempt": 1,
        "name": "Publish",
        "path": ".github/workflows/publish.yml",
        "event": "release",
        "status": "completed",
        "conclusion": conclusion,
        "head_branch": tag,
        "head_sha": BUILT,
        "created_at": started,
        "run_started_at": started,
        "updated_at": started,
        "html_url": format!("{WEB}/actions/runs/{id}"),
        "repository": {"full_name": "acme/widgets"},
    })
}

fn absolute(url: &Value) -> bool {
    url.as_str()
        .is_some_and(|url| url.starts_with("https://") || url.starts_with("http://"))
}

fn routes(requests: &[Request]) -> Vec<String> {
    requests
        .iter()
        .map(|request| format!("{} {}", request.method, request.url.path()))
        .collect()
}

#[tokio::test]
async fn releases_published_inside_the_window_are_listed_with_their_pulls_and_pipeline() {
    let releases = Releases::new().await;
    releases
        .answer_json(
            "GET",
            "/releases",
            200,
            json!([
                release(92, "v1.5.0", None, true, NOTES),
                release(PUBLISHED_ID, LATEST, Some(&ago(1)), false, NOTES),
                release(80, "v1.3.0", Some(&ago(30)), false, ""),
            ]),
        )
        .await;
    releases.runs(vec![run(501, LATEST, "success")]).await;

    let (status, body) = releases
        .get(&format!("{}?days=7", releases.releases()))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["window_days"], 7);
    assert_eq!(body["ttl_secs"], 120);
    assert_eq!(body["stale"], false);
    assert_eq!(body["partial"], false);
    assert_eq!(body["warnings"], json!([]));
    assert_eq!(body["sources"][0]["source_id"], json!(releases.source));
    assert_eq!(body["sources"][0]["repository"], "acme/widgets");
    assert_eq!(body["sources"][0]["repository_url"], WEB);
    assert_eq!(body["sources"][0]["freshness"], "refreshed");
    let listed = body["releases"].as_array().expect("releases");
    assert_eq!(
        listed.len(),
        1,
        "a draft and a release older than the window are left out: {body}"
    );
    let release = &listed[0];
    assert_eq!(release["id"], PUBLISHED_ID);
    assert_eq!(release["tag"], LATEST);
    assert_eq!(release["source_id"], json!(releases.source));
    assert_eq!(
        release["pulls"],
        json!([
            {"number": 12, "url": format!("{WEB}/pull/12"), "title": "Add widgets"},
            {"number": 13, "url": format!("{WEB}/pull/13"), "title": "Fix gadgets"},
        ])
    );
    assert_eq!(release["state"], "succeeded");
    assert_eq!(release["succeeded"], true);
    assert_eq!(release["complete"], true);
    assert_eq!(release["blockers"], json!([]));
    assert_eq!(release["pipeline"]["lookup"], "complete");
    assert_eq!(release["pipeline"]["runs"][0]["state"], "succeeded");
    for url in [
        &release["url"],
        &release["repository_url"],
        &release["pipeline"]["release"]["url"],
        &release["pipeline"]["runs"][0]["url"],
        &release["pulls"][0]["url"],
    ] {
        assert!(absolute(url), "{url} is not an absolute link");
    }

    let (_, wider) = releases
        .get(&format!("{}?days=60", releases.releases()))
        .await;
    assert_eq!(wider["window_days"], 60);
    assert_eq!(
        wider["sources"][0]["freshness"], "cached",
        "a wider window is cut from the same read"
    );
    let tags: Vec<&Value> = wider["releases"]
        .as_array()
        .expect("releases")
        .iter()
        .map(|release| &release["tag"])
        .collect();
    assert_eq!(tags, [LATEST, "v1.3.0"], "newest first: {wider}");
    assert_eq!(wider["releases"][1]["state"], "unknown");
    assert_eq!(
        wider["releases"][1]["pipeline"]["lookup"], "unavailable",
        "a release past the lookup horizon is never looked up"
    );
    assert_eq!(wider["releases"][1]["pulls"], json!([]));

    let (_, again) = releases
        .get(&format!("{}?days=7", releases.releases()))
        .await;
    assert_eq!(again["sources"][0]["freshness"], "cached");
    assert_eq!(again["releases"], body["releases"]);
    assert_eq!(
        releases
            .sent("GET", &format!("{REPOSITORY}/releases"))
            .await
            .len(),
        1,
        "one read per source serves every window"
    );

    let (_, clamped) = releases
        .get(&format!("{}?days=500", releases.releases()))
        .await;
    assert_eq!(clamped["window_days"], 90);
}

#[tokio::test]
async fn a_release_still_without_a_run_an_hour_after_publishing_is_not_looked_up_again() {
    let releases = Releases::new().await;
    releases
        .answer_json(
            "GET",
            "/releases",
            200,
            json!([
                release(FRESH_ID, FRESH, Some(&hours_ago(0)), false, NOTES),
                release(PUBLISHED_ID, LATEST, Some(&hours_ago(2)), false, NOTES),
            ]),
        )
        .await;
    releases.runs(Vec::new()).await;

    let first = releases.relisted("").await;
    assert_eq!(
        pipeline_of(&first, LATEST)["lookup"],
        "complete",
        "an hour without a run is no release pipeline: {first}"
    );
    assert_eq!(pipeline_of(&first, LATEST)["runs"], json!([]));
    assert_eq!(pipeline_of(&first, FRESH)["lookup"], "pending");
    assert_eq!(releases.exact_reads(LATEST).await, 1);
    assert_eq!(releases.exact_reads(FRESH).await, 1);

    let second = releases.relisted("").await;
    assert_eq!(pipeline_of(&second, LATEST)["lookup"], "complete");
    assert_eq!(pipeline_of(&second, FRESH)["lookup"], "pending");
    assert_eq!(
        releases.exact_reads(LATEST).await,
        1,
        "a settled release is not looked up again"
    );
    assert_eq!(
        releases.exact_reads(FRESH).await,
        2,
        "a release GitHub may still start a run for is"
    );
    assert_eq!(releases.sweeps().await, 2);
}

#[tokio::test]
async fn a_release_older_than_the_lookup_horizon_makes_no_pipeline_call() {
    let releases = Releases::new().await;
    releases
        .answer_json(
            "GET",
            "/releases",
            200,
            json!([release(80, "v1.3.0", Some(&ago(30)), false, "")]),
        )
        .await;
    releases.runs(vec![run(501, "v1.3.0", "success")]).await;

    for _ in 0..2 {
        let listing = releases.relisted("?days=60").await;
        assert_eq!(pipeline_of(&listing, "v1.3.0")["lookup"], "unavailable");
        assert_eq!(listing["releases"][0]["state"], "unknown");
    }

    assert_eq!(
        releases
            .sent("GET", &format!("{REPOSITORY}/actions/runs"))
            .await
            .len(),
        0,
        "neither a sweep nor an exact read reaches GitHub"
    );
    assert_eq!(
        releases
            .sent("GET", &format!("{REPOSITORY}/releases"))
            .await
            .len(),
        2
    );
}

#[tokio::test]
async fn a_source_github_cannot_answer_is_reported_without_failing_the_page() {
    let releases = Releases::new().await;
    releases
        .answer_json("GET", "/releases", 500, json!({"message": "boom"}))
        .await;

    let (status, body) = releases.get(&releases.releases()).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["partial"], true);
    assert_eq!(body["stale"], true);
    assert_eq!(body["releases"], json!([]));
    assert_eq!(body["sources"][0]["freshness"], "stale");
    assert_eq!(body["sources"][0]["generated_at"], Value::Null);
    assert!(
        body["sources"][0]["warning"]
            .as_str()
            .is_some_and(|warning| warning.contains("HTTP 500")),
        "{body}"
    );
}

#[tokio::test]
async fn the_pipeline_route_is_served_from_its_cache_for_fifteen_seconds() {
    let releases = Releases::new().await;
    let published = release(PUBLISHED_ID, LATEST, Some(&ago(0)), false, NOTES);
    releases
        .answer_json("GET", "/releases", 200, json!([published.clone()]))
        .await;
    releases
        .answer_json("GET", &format!("/releases/{PUBLISHED_ID}"), 200, published)
        .await;
    releases.runs(vec![run(501, LATEST, "success")]).await;

    let (status, first) = releases.get(&releases.pipeline(PUBLISHED_ID)).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["state"], "succeeded");
    assert_eq!(first["succeeded"], true);
    assert_eq!(first["complete"], true);
    assert_eq!(first["blockers"], json!([]));
    assert_eq!(first["pipeline"]["release"]["id"], PUBLISHED_ID);
    assert_eq!(first["pipeline"]["runs"][0]["id"], 501);

    let (_, polled) = releases.get(&releases.pipeline(PUBLISHED_ID)).await;
    let (_, refreshed) = releases
        .get(&format!("{}?refresh=1", releases.pipeline(PUBLISHED_ID)))
        .await;
    assert_eq!(polled, first);
    assert_eq!(refreshed, first);
    assert_eq!(
        releases
            .sent("GET", &format!("{REPOSITORY}/releases/{PUBLISHED_ID}"))
            .await
            .len(),
        1,
        "a poll inside fifteen seconds is answered from the cache"
    );
    assert_eq!(
        releases
            .sent("GET", &format!("{REPOSITORY}/actions/runs"))
            .await
            .len(),
        2,
        "one sweep for the listing the release is checked against, and one exact read"
    );
    assert_eq!(
        releases
            .sent("GET", &format!("{REPOSITORY}/releases"))
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn a_pipeline_is_answered_only_for_a_release_in_the_listing() {
    let releases = Releases::new().await;
    releases
        .answer_json(
            "GET",
            "/releases",
            200,
            json!([release(PUBLISHED_ID, LATEST, Some(&ago(0)), false, NOTES)]),
        )
        .await;
    releases.runs(vec![run(501, LATEST, "success")]).await;
    releases.get(&releases.releases()).await;
    let before = releases.requests().await.len();

    for id in [PUBLISHED_ID + 1, u64::MAX] {
        let (status, body) = releases.get(&releases.pipeline(id)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{id}: {body}");
        assert_eq!(body["success"], false);
    }

    assert_eq!(
        releases.requests().await.len(),
        before,
        "a release the listing does not hold never reaches GitHub"
    );
}

#[tokio::test]
async fn a_draft_or_missing_release_has_no_pipeline() {
    let releases = Releases::new().await;
    releases
        .answer_json(
            "GET",
            "/releases",
            200,
            json!([release(93, "v2.0.0", None, true, "")]),
        )
        .await;

    for id in [93, 94] {
        let (status, body) = releases.get(&releases.pipeline(id)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{id}: {body}");
        assert_eq!(body["success"], false);
        assert!(
            releases
                .sent("GET", &format!("{REPOSITORY}/releases/{id}"))
                .await
                .is_empty()
        );
    }
    let (status, _) = releases
        .get(&format!(
            "/api/workspaces/{}/releases/{}/{PUBLISHED_ID}/pipeline",
            releases.workspace,
            Uuid::new_v4()
        ))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn options_offer_the_next_patch_after_the_latest_stable_tag() {
    let releases = Releases::new().await;
    releases
        .answer_json(
            "GET",
            "/tags",
            200,
            json!([
                tag_row("v1.2.3"),
                tag_row("v1.10.0"),
                tag_row("v1.11.0-rc.1"),
                tag_row("nightly"),
            ]),
        )
        .await;
    releases
        .answer_json("GET", "", 200, json!({"default_branch": "trunk"}))
        .await;

    let (status, body) = releases
        .get(&format!("{}/options", releases.releases()))
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["warnings"], json!([]));
    assert!(body["generated_at"].is_string());
    assert_eq!(
        body["sources"],
        json!([{
            "source_id": releases.source,
            "repository": "acme/widgets",
            "repository_url": WEB,
            "default_branch": "trunk",
            "latest_tag": "v1.10.0",
            "next_tag": "v1.10.1",
            "previous_tags": ["v1.11.0-rc.1", "v1.10.0", "v1.2.3"],
            "lines": [{
                "prefix": null,
                "latest_tag": "v1.10.0",
                "next_tag": "v1.10.1",
                "previous_tags": ["v1.11.0-rc.1", "v1.10.0", "v1.2.3"],
            }],
        }])
    );
}

#[tokio::test]
async fn every_tag_page_is_read() {
    let releases = Releases::new().await;
    for page in 1..=4 {
        let full: Vec<Value> = (0..100)
            .map(|patch| tag_row(&format!("v1.{page}.{patch}")))
            .collect();
        Mock::given(method("GET"))
            .and(path(releases.route("/tags")))
            .and(query_param("page", page.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!(full)))
            .mount(&releases.github)
            .await;
    }
    Mock::given(method("GET"))
        .and(path(releases.route("/tags")))
        .and(query_param("page", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([tag_row("v9.0.0")])))
        .mount(&releases.github)
        .await;
    releases
        .answer_json("GET", "", 200, json!({"default_branch": "main"}))
        .await;

    let (status, body) = releases
        .get(&format!("{}/options", releases.releases()))
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sources"][0]["latest_tag"], "v9.0.0");
    assert_eq!(body["sources"][0]["next_tag"], "v9.0.1");
}

#[tokio::test]
async fn a_prefixed_repository_offers_and_releases_each_line_on_its_own() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable_as("abnegate-vcs/v0.2.2").await;
    releases
        .answer_first(
            "GET",
            "/tags",
            200,
            json!([
                tag_row("abnegate-vcs/v0.2.1"),
                tag_row("abnegate-vcs/v0.2.0"),
                tag_row("abnegate-llm/v1.0.0"),
            ]),
        )
        .await;

    let (status, body) = releases
        .get(&format!("{}/options", releases.releases()))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let offered = &body["sources"][0];
    assert_eq!(offered["latest_tag"], "abnegate-vcs/v0.2.1");
    assert_eq!(offered["next_tag"], "abnegate-vcs/v0.2.2");
    assert_eq!(offered["lines"][1]["prefix"], "abnegate-llm");
    assert_eq!(offered["lines"][1]["next_tag"], "abnegate-llm/v1.0.1");

    let (status, preview) = releases
        .post(
            &format!("{}/preview", releases.releases()),
            &releases.proposal("abnegate-vcs/v0.2.2", Some("abnegate-vcs/v0.2.1")),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["preview"]["base_tag"], "abnegate-vcs/v0.2.1");
    assert_eq!(preview["preview"]["prerelease"], false);
    let notes = releases
        .sent("POST", &format!("{REPOSITORY}/releases/generate-notes"))
        .await;
    assert_eq!(
        notes[0].body_json::<Value>().expect("a JSON body")["previous_tag_name"],
        "abnegate-vcs/v0.2.1",
        "the notes run from the same line's latest tag"
    );

    let mut body = releases.proposal("abnegate-vcs/v0.2.2", Some("abnegate-vcs/v0.2.1"));
    body["prerelease"] = json!(false);
    body["digest"] = preview["preview"]["digest"].clone();
    let (status, created) = releases.post(&releases.releases(), &body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let drafted: Value = releases
        .sent("POST", &format!("{REPOSITORY}/releases"))
        .await[0]
        .body_json()
        .expect("a JSON body");
    assert_eq!(drafted["tag_name"], "abnegate-vcs/v0.2.2");
    assert_eq!(
        drafted["make_latest"], "false",
        "a line behind the repository's newest stable version is not made the latest"
    );
}

#[tokio::test]
async fn a_tag_no_newer_than_its_lines_latest_is_refused() {
    let releases = Releases::new().await;
    releases.ready().await;
    let preview = format!("{}/preview", releases.releases());

    for tag in [LATEST, "v1.3.9", "v1.4.0-rc.1"] {
        let (status, body) = releases
            .post(&preview, &releases.proposal(tag, Some(LATEST)))
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{tag}: {body}");
        assert_eq!(body["code"], "tag_not_newer");
        assert_eq!(body["tag"], tag);
        assert_eq!(body["latest_tag"], LATEST);
    }
    assert!(
        releases
            .sent("POST", &format!("{REPOSITORY}/releases/generate-notes"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_tag_that_already_has_a_release_even_a_draft_is_refused() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases
        .answer_first(
            "GET",
            "/releases",
            200,
            json!([release(95, NEXT, None, true, "")]),
        )
        .await;

    let (status, body) = releases
        .post(
            &format!("{}/preview", releases.releases()),
            &releases.proposal(NEXT, Some(LATEST)),
        )
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "release_exists");
    assert_eq!(body["tag"], NEXT);
    assert_eq!(body["release_id"], 95);
}

#[tokio::test]
async fn a_prerelease_flag_that_disagrees_with_the_tag_is_refused() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    let preview = releases.previewed().await;
    let mut body = releases.proposal(NEXT, Some(LATEST));
    body["prerelease"] = json!(true);
    body["digest"] = preview["digest"].clone();

    let (status, refused) = releases.post(&releases.releases(), &body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
    assert_eq!(refused["success"], false);
    assert!(
        releases
            .sent("POST", &format!("{REPOSITORY}/git/tags"))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn a_preview_drafts_the_notes_from_the_latest_tag_to_the_default_branch() {
    let releases = Releases::new().await;
    releases.ready().await;

    let preview = releases.previewed().await;

    assert_eq!(preview["source_id"], json!(releases.source));
    assert_eq!(preview["repository"], "acme/widgets");
    assert_eq!(preview["tag"], NEXT);
    assert_eq!(preview["name"], NEXT);
    assert_eq!(preview["body"], NOTES);
    assert_eq!(preview["base_tag"], LATEST);
    assert_eq!(preview["target"], TARGET);
    assert_eq!(preview["pulls"][0]["number"], 12);
    assert_eq!(preview["pulls"][1]["title"], "Fix gadgets");
    assert!(
        preview["digest"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "{preview}"
    );
    let asked = releases
        .sent("POST", &format!("{REPOSITORY}/releases/generate-notes"))
        .await;
    assert_eq!(
        asked[0].body_json::<Value>().expect("a JSON body"),
        json!({"tag_name": NEXT, "target_commitish": TARGET, "previous_tag_name": LATEST})
    );
    assert_eq!(
        releases.previewed().await,
        preview,
        "a preview is repeatable"
    );
    assert!(
        releases
            .requests()
            .await
            .iter()
            .all(|request| request.method.as_str() == "GET"
                || request.url.path().ends_with("/generate-notes")),
        "a preview writes nothing"
    );
}

#[tokio::test]
async fn a_preview_refuses_a_moved_latest_tag_an_existing_tag_and_a_tag_that_is_no_version() {
    let releases = Releases::new().await;
    releases.ready().await;
    let preview = format!("{}/preview", releases.releases());

    let (status, moved) = releases
        .post(&preview, &releases.proposal(NEXT, Some("v1.3.0")))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{moved}");
    assert_eq!(moved["success"], false);
    assert_eq!(moved["code"], "latest_tag_moved");

    let (status, unset) = releases
        .post(&preview, &releases.proposal(NEXT, None))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{unset}");
    assert_eq!(unset["code"], "latest_tag_moved");

    releases
        .answer_first(
            "GET",
            &format!("/git/ref/tags/{NEXT}"),
            200,
            json!({"ref": format!("refs/tags/{NEXT}"), "object": {"sha": OTHER_OBJECT, "type": "tag"}}),
        )
        .await;
    let (status, taken) = releases
        .post(&preview, &releases.proposal(NEXT, Some(LATEST)))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{taken}");
    assert_eq!(taken["code"], "tag_exists");
    assert_eq!(taken["tag"], NEXT);

    for tag in ["banana", "v1.4", "v1.4.1 "] {
        let (status, invalid) = releases
            .post(&preview, &releases.proposal(tag, Some(LATEST)))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{tag}: {invalid}");
        assert_eq!(invalid["success"], false);
    }
    assert!(
        releases
            .sent("POST", &format!("{REPOSITORY}/releases/generate-notes"))
            .await
            .is_empty(),
        "no notes are drafted for a refused preview"
    );
}

#[tokio::test]
async fn github_failing_before_anything_is_written_is_a_bad_gateway() {
    let releases = Releases::new().await;
    releases.ready().await;
    Mock::given(method("POST"))
        .and(path(format!("{REPOSITORY}/releases/generate-notes")))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({"message": "down"})))
        .with_priority(1)
        .mount(&releases.github)
        .await;

    let (status, body) = releases
        .post(
            &format!("{}/preview", releases.releases()),
            &releases.proposal(NEXT, Some(LATEST)),
        )
        .await;

    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(body["success"], false);
    assert!(body.get("code").is_none(), "{body}");
}

#[tokio::test]
async fn creating_a_release_tags_references_drafts_publishes_and_reads_it_back_in_order() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    releases
        .answer_json("GET", "/releases", 200, json!([]))
        .await;
    let (_, listed) = releases.get(&releases.releases()).await;
    assert_eq!(listed["sources"][0]["freshness"], "refreshed");
    let (_, cached) = releases.get(&releases.releases()).await;
    assert_eq!(cached["sources"][0]["freshness"], "cached");
    let preview = releases.previewed().await;

    let (status, body) = releases
        .create(preview["digest"].as_str().expect("a digest"))
        .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    let created = &body["release"];
    assert_eq!(created["source_id"], json!(releases.source));
    assert_eq!(created["id"], DRAFT_ID);
    assert_eq!(created["tag"], NEXT);
    assert_eq!(created["name"], NEXT);
    assert_eq!(created["url"], format!("{WEB}/releases/tag/{NEXT}"));
    assert!(absolute(&created["url"]));
    assert!(created["published_at"].is_string());

    let requests = releases.requests().await;
    let writes: Vec<String> = routes(&requests)
        .into_iter()
        .filter(|route| !route.starts_with("GET "))
        .collect();
    assert_eq!(
        writes,
        [
            format!("POST {REPOSITORY}/releases/generate-notes"),
            format!("POST {REPOSITORY}/releases/generate-notes"),
            format!("POST {REPOSITORY}/git/tags"),
            format!("POST {REPOSITORY}/git/refs"),
            format!("POST {REPOSITORY}/releases"),
            format!("PATCH {REPOSITORY}/releases/{DRAFT_ID}"),
        ]
    );
    assert_eq!(
        routes(&requests).last().map(String::as_str),
        Some(format!("GET {REPOSITORY}/releases/{DRAFT_ID}").as_str()),
        "the published release is read back last"
    );
    releases.never_deleted().await;

    let body_of = |route: &str| -> Value {
        requests
            .iter()
            .find(|request| request.method.as_str() != "GET" && request.url.path() == route)
            .expect("the request was sent")
            .body_json()
            .expect("a JSON body")
    };
    let tagged = body_of(&format!("{REPOSITORY}/git/tags"));
    assert_eq!(tagged["tag"], NEXT);
    assert_eq!(tagged["object"], TARGET);
    assert_eq!(tagged["type"], "commit");
    assert_eq!(
        tagged["tagger"],
        json!({"name": "Zone", "email": "zone@users.noreply.github.com"})
    );
    assert!(
        tagged["message"]
            .as_str()
            .is_some_and(|message| message.contains(MARKER)),
        "{tagged}"
    );
    assert_eq!(
        body_of(&format!("{REPOSITORY}/git/refs")),
        json!({"ref": format!("refs/tags/{NEXT}"), "sha": TAG_OBJECT})
    );
    let drafted = body_of(&format!("{REPOSITORY}/releases"));
    assert_eq!(drafted["tag_name"], NEXT);
    assert_eq!(drafted["target_commitish"], TARGET);
    assert_eq!(drafted["name"], NEXT);
    assert_eq!(drafted["draft"], true);
    assert_eq!(drafted["prerelease"], false);
    assert_eq!(drafted["make_latest"], "true");
    let marked = drafted["body"].as_str().expect("a body");
    assert!(
        marked.starts_with(NOTES) && marked.contains(MARKER),
        "{marked}"
    );
    assert_eq!(
        body_of(&format!("{REPOSITORY}/releases/{DRAFT_ID}")),
        json!({"draft": false})
    );

    let audited: Vec<(String, Value)> = sqlx::query_as(
        "SELECT resource_type, new_values FROM audit_logs WHERE workspace_id = $1 AND action = 'release.created'",
    )
    .bind(releases.workspace)
    .fetch_all(&releases.pool)
    .await
    .expect("the audit log reads");
    assert_eq!(audited.len(), 1, "{audited:?}");
    assert_eq!(audited[0].0, "release");
    assert_eq!(audited[0].1["release_id"], DRAFT_ID);
    assert_eq!(audited[0].1["tag"], NEXT);
    assert_eq!(audited[0].1["source_id"], json!(releases.source));

    let (_, after) = releases.get(&releases.releases()).await;
    assert_eq!(
        after["sources"][0]["freshness"], "refreshed",
        "publishing a release drops what was cached for its source"
    );
}

#[tokio::test]
async fn a_prerelease_is_never_made_the_latest() {
    const CANDIDATE: &str = "v1.5.0-rc.1";
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable_as(CANDIDATE).await;
    let (status, preview) = releases
        .post(
            &format!("{}/preview", releases.releases()),
            &releases.proposal(CANDIDATE, Some(LATEST)),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["preview"]["prerelease"], true);
    let mut body = releases.proposal(CANDIDATE, Some(LATEST));
    body["prerelease"] = json!(true);
    body["digest"] = preview["preview"]["digest"].clone();

    let (status, created) = releases.post(&releases.releases(), &body).await;

    assert_eq!(status, StatusCode::CREATED, "{created}");
    let drafted = releases
        .sent("POST", &format!("{REPOSITORY}/releases"))
        .await;
    let drafted: Value = drafted[0].body_json().expect("a JSON body");
    assert_eq!(drafted["prerelease"], true);
    assert_eq!(drafted["make_latest"], "false");
}

#[tokio::test]
async fn a_reference_that_already_points_at_the_new_tag_object_is_taken_as_created() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    Mock::given(method("POST"))
        .and(path(format!("{REPOSITORY}/git/refs")))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({"message": "Reference already exists"})),
        )
        .with_priority(1)
        .mount(&releases.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{REPOSITORY}/git/ref/tags/{NEXT}")))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&releases.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{REPOSITORY}/git/ref/tags/{NEXT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"ref": format!("refs/tags/{NEXT}"), "object": {"sha": TAG_OBJECT, "type": "tag"}}),
        ))
        .with_priority(2)
        .mount(&releases.github)
        .await;
    let preview = releases.previewed().await;

    let (status, body) = releases
        .create(preview["digest"].as_str().expect("a digest"))
        .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["release"]["id"], DRAFT_ID);
    assert_eq!(
        releases
            .sent("GET", &format!("{REPOSITORY}/git/ref/tags/{NEXT}"))
            .await
            .len(),
        3,
        "the existing reference was read once more to see whose it is"
    );
    assert_eq!(
        releases
            .sent("PATCH", &format!("{REPOSITORY}/releases/{DRAFT_ID}"))
            .await
            .len(),
        1
    );
    releases.never_deleted().await;
}

#[tokio::test]
async fn a_reference_another_writer_took_first_is_an_existing_tag() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    Mock::given(method("POST"))
        .and(path(format!("{REPOSITORY}/git/refs")))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({"message": "Reference already exists"})),
        )
        .with_priority(1)
        .mount(&releases.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{REPOSITORY}/git/ref/tags/{NEXT}")))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&releases.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{REPOSITORY}/git/ref/tags/{NEXT}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"ref": format!("refs/tags/{NEXT}"), "object": {"sha": OTHER_OBJECT, "type": "tag"}}),
        ))
        .with_priority(2)
        .mount(&releases.github)
        .await;
    let preview = releases.previewed().await;

    let (status, body) = releases
        .create(preview["digest"].as_str().expect("a digest"))
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "tag_exists");
    assert_eq!(body["tag"], NEXT);
    assert_eq!(body["tag_object"], TAG_OBJECT);
    assert!(
        releases
            .sent("POST", &format!("{REPOSITORY}/releases"))
            .await
            .is_empty(),
        "no release is drafted under someone else's tag"
    );
    releases.never_deleted().await;
}

#[tokio::test]
async fn a_preview_whose_notes_changed_since_is_stale_and_nothing_is_written() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    let preview = releases.previewed().await;
    Mock::given(method("POST"))
        .and(path(format!("{REPOSITORY}/releases/generate-notes")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "name": NEXT,
            "body": format!("{NOTES}\n* Late change by @octo in {WEB}/pull/14"),
        })))
        .with_priority(1)
        .mount(&releases.github)
        .await;

    let (status, body) = releases
        .create(preview["digest"].as_str().expect("a digest"))
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["success"], false);
    assert_eq!(body["code"], "preview_stale");
    assert!(
        releases
            .sent("POST", &format!("{REPOSITORY}/git/tags"))
            .await
            .is_empty(),
        "a stale preview writes nothing"
    );
}

#[tokio::test]
async fn only_one_release_is_created_for_a_source_at_a_time() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    let preview = releases.previewed().await;
    let digest = preview["digest"].as_str().expect("a digest");

    let held = releases
        .state
        .github_snapshots()
        .try_release(releases.source)
        .expect("nothing else is releasing");
    let before = releases.requests().await.len();
    let (status, body) = releases.create(digest).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "release_in_progress");
    assert_eq!(
        releases.requests().await.len(),
        before,
        "a refused create never reaches GitHub"
    );
    drop(held);

    Mock::given(method("GET"))
        .and(path(format!("{REPOSITORY}/tags")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([tag_row(LATEST)]))
                .set_delay(Duration::from_millis(500)),
        )
        .with_priority(1)
        .mount(&releases.github)
        .await;
    let (first, second) = tokio::join!(releases.create(digest), releases.create(digest));
    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::CONFLICT],
        "{} / {}",
        first.1,
        second.1
    );
    let refused = if first.0 == StatusCode::CONFLICT {
        &first.1
    } else {
        &second.1
    };
    assert_eq!(refused["code"], "release_in_progress");
    assert_eq!(
        releases
            .sent("POST", &format!("{REPOSITORY}/git/tags"))
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn a_release_github_refuses_after_the_tag_was_written_names_what_to_reconcile() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    Mock::given(method("POST"))
        .and(path(format!("{REPOSITORY}/releases")))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({
            "message": "Validation Failed",
            "errors": [{"resource": "Release", "code": "already_exists", "field": "tag_name"}],
        })))
        .with_priority(1)
        .mount(&releases.github)
        .await;
    let preview = releases.previewed().await;

    let (status, body) = releases
        .create(preview["digest"].as_str().expect("a digest"))
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["success"], false);
    assert_eq!(body["code"], "release_manual_reconciliation_required");
    assert_eq!(body["tag"], NEXT);
    assert_eq!(body["tag_object"], TAG_OBJECT);
    assert_eq!(body["reference_created"], true);
    assert_eq!(body["release_id"], Value::Null);
    assert_eq!(body["release_exists"], true);
    assert!(
        body.as_object()
            .is_some_and(|body| body.contains_key("release_id"))
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains(TAG_OBJECT)),
        "{body}"
    );
    marked_with_the_tags_marker(&releases, &body).await;
    releases.never_deleted().await;
}

async fn marked_with_the_tags_marker(releases: &Releases, body: &Value) {
    let marker = body["marker"]
        .as_str()
        .expect("the refusal names its marker");
    let tagged: Value = releases
        .sent("POST", &format!("{REPOSITORY}/git/tags"))
        .await[0]
        .body_json()
        .expect("a JSON body");
    assert!(
        tagged["message"]
            .as_str()
            .is_some_and(|message| message.contains(&format!("{MARKER}{marker} -->"))),
        "{tagged} / {marker}"
    );
}

#[tokio::test]
async fn a_reference_whose_fate_cannot_be_read_back_is_unknown() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    Mock::given(method("POST"))
        .and(path(format!("{REPOSITORY}/git/refs")))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({"message": "Reference already exists"})),
        )
        .with_priority(1)
        .mount(&releases.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{REPOSITORY}/git/ref/tags/{NEXT}")))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&releases.github)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{REPOSITORY}/git/ref/tags/{NEXT}")))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"message": "boom"})))
        .with_priority(2)
        .mount(&releases.github)
        .await;
    let preview = releases.previewed().await;

    let (status, body) = releases
        .create(preview["digest"].as_str().expect("a digest"))
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "release_manual_reconciliation_required");
    assert_eq!(body["tag_object"], TAG_OBJECT);
    assert_eq!(body["reference_created"], Value::Null);
    assert_eq!(body["release_id"], Value::Null);
    assert!(
        releases
            .sent("POST", &format!("{REPOSITORY}/releases"))
            .await
            .is_empty(),
        "no release is drafted under a reference nobody can vouch for"
    );
    releases.never_deleted().await;
}

#[tokio::test]
async fn a_draft_github_would_not_publish_is_named_for_reconciling() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    Mock::given(method("PATCH"))
        .and(path(format!("{REPOSITORY}/releases/{DRAFT_ID}")))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({"message": "boom"})))
        .with_priority(1)
        .mount(&releases.github)
        .await;
    releases
        .answer_first(
            "GET",
            &format!("/releases/{DRAFT_ID}"),
            200,
            release(DRAFT_ID, NEXT, None, true, NOTES),
        )
        .await;
    let preview = releases.previewed().await;

    let (status, body) = releases
        .create(preview["digest"].as_str().expect("a digest"))
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "release_manual_reconciliation_required");
    assert_eq!(body["tag_object"], TAG_OBJECT);
    assert_eq!(body["reference_created"], true);
    assert_eq!(body["release_id"], DRAFT_ID);
    assert_eq!(body["release_exists"], false);
    marked_with_the_tags_marker(&releases, &body).await;
    releases.never_deleted().await;
}

#[tokio::test]
async fn a_publish_whose_answer_was_lost_is_read_back_before_reconciling() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    Mock::given(method("PATCH"))
        .and(path(format!("{REPOSITORY}/releases/{DRAFT_ID}")))
        .respond_with(ResponseTemplate::new(502).set_body_json(json!({"message": "lost"})))
        .with_priority(1)
        .mount(&releases.github)
        .await;
    let preview = releases.previewed().await;

    let (status, body) = releases
        .create(preview["digest"].as_str().expect("a digest"))
        .await;

    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["release"]["id"], DRAFT_ID);
}

#[tokio::test]
async fn a_failed_create_frees_its_source_for_the_next() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    Mock::given(method("POST"))
        .and(path(format!("{REPOSITORY}/git/tags")))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({"message": "down"})))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&releases.github)
        .await;
    let preview = releases.previewed().await;
    let digest = preview["digest"].as_str().expect("a digest");

    let (status, body) = releases.create(digest).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");

    let (status, body) = releases.create(digest).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn a_create_whose_caller_hangs_up_still_finishes_and_is_audited() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases.writable().await;
    Mock::given(method("GET"))
        .and(path(releases.route(&format!("/releases/{DRAFT_ID}"))))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(release(DRAFT_ID, NEXT, Some(&ago(0)), false, NOTES))
                .set_delay(Duration::from_millis(500)),
        )
        .with_priority(1)
        .mount(&releases.github)
        .await;
    let preview = releases.previewed().await;
    let digest = preview["digest"].as_str().expect("a digest").to_string();

    let published = releases.route(&format!("/releases/{DRAFT_ID}"));
    let answered = tokio::select! {
        answered = releases.create(&digest) => Some(answered),
        () = async {
            while releases.sent("PATCH", &published).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        } => None,
    };
    assert!(
        answered.is_none(),
        "the caller hangs up while GitHub publishes"
    );

    let mut audited = 0;
    for _ in 0..100 {
        audited = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM audit_logs WHERE workspace_id = $1 AND action = 'release.created'",
        )
        .bind(releases.workspace)
        .fetch_one(&releases.pool)
        .await
        .expect("the audit log reads");
        if audited > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(audited, 1, "the release finished and was audited");
    assert_eq!(
        releases
            .sent("PATCH", &format!("{REPOSITORY}/releases/{DRAFT_ID}"))
            .await
            .len(),
        1
    );
    assert!(
        releases
            .state
            .github_snapshots()
            .try_release(releases.source)
            .is_some(),
        "the finished create let go of its source"
    );
}

#[tokio::test]
async fn an_enterprise_release_pipeline_is_read_at_its_own_origin() {
    let releases = Releases::enterprise().await;
    let web = format!("{}/acme/widgets", releases.github.uri());
    let mut published = release(PUBLISHED_ID, LATEST, Some(&ago(0)), false, NOTES);
    published["html_url"] = json!(format!("{web}/releases/tag/{LATEST}"));
    releases
        .answer_json("GET", "/releases", 200, json!([published.clone()]))
        .await;
    releases
        .answer_json("GET", &format!("/releases/{PUBLISHED_ID}"), 200, published)
        .await;
    let mut built = run(501, LATEST, "success");
    built["html_url"] = json!(format!("{web}/actions/runs/501"));
    releases.runs(vec![built]).await;

    let (status, body) = releases.get(&releases.releases()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["releases"][0]["state"], "succeeded", "{body}");
    assert_eq!(body["releases"][0]["repository_url"], json!(web));

    let (status, pipeline) = releases.get(&releases.pipeline(PUBLISHED_ID)).await;
    assert_eq!(status, StatusCode::OK, "{pipeline}");
    assert_eq!(pipeline["state"], "succeeded");
    assert_eq!(
        pipeline["pipeline"]["runs"][0]["url"],
        format!("{web}/actions/runs/501")
    );
    assert!(
        releases
            .requests()
            .await
            .iter()
            .all(|request| request.url.path().starts_with(ENTERPRISE_API)),
        "every read went to the Enterprise API"
    );
}

#[tokio::test]
async fn a_writer_without_the_release_permission_is_refused() {
    let releases = Releases::new().await;
    releases.ready().await;
    let writer = releases.writer_in_role("viewer").await;

    let (status, body) = releases
        .post_as(
            &format!("{}/preview", releases.releases()),
            &releases.proposal(NEXT, Some(LATEST)),
            &writer,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|error| error.contains("releases:create")),
        "{body}"
    );
    let (status, body) = releases.create_as(&"0".repeat(64), &writer).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(releases.requests().await.is_empty());
}

#[tokio::test]
async fn a_viewer_reads_releases_but_cannot_preview_or_create_them() {
    let releases = Releases::new().await;
    releases.ready().await;
    releases
        .answer_json("GET", "/releases", 200, json!([]))
        .await;
    let viewer = releases.viewer().await;

    let (status, body) = releases.get_as(&releases.releases(), &viewer).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = releases
        .post_as(
            &format!("{}/preview", releases.releases()),
            &releases.proposal(NEXT, Some(LATEST)),
            &viewer,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (status, body) = releases.create_as(&"0".repeat(64), &viewer).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        releases
            .requests()
            .await
            .iter()
            .all(|request| request.method.as_str() == "GET"),
        "a viewer's refused writes never reach GitHub"
    );
}

#[tokio::test]
async fn another_workspace_cannot_read_or_publish_these_releases() {
    let releases = Releases::new().await;
    let (stranger, _) = member(&releases.client).await;

    let (status, _) = releases.get_as(&releases.releases(), &stranger).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = releases
        .get_as(&releases.pipeline(PUBLISHED_ID), &stranger)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = releases.create_as(&"0".repeat(64), &stranger).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(releases.requests().await.is_empty());
}
