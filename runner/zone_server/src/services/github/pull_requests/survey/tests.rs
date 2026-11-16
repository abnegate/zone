use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;
use uuid::Uuid;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::any;
use wiremock::matchers::body_partial_json;
use wiremock::matchers::body_string_contains;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::query_param;

use super::*;
use crate::services::github::pull_requests::GRAPHQL_LIMIT;
use crate::services::github::pull_requests::Readiness;
use crate::services::github::pull_requests::SURVEY_DEADLINE;
use crate::services::github::readiness::BlockerCode;
use crate::services::github::readiness::Mergeability;
use crate::services::github::testing::mocked;

const OWNER: &str = "owner";
const NAME: &str = "repository";
const REPOSITORY_ID: &str = "R_repository";
const ELSEWHERE: &str = "R_elsewhere";
const WEB: &str = "https://github.com/owner/repository";
const UPDATED_AT: &str = "2026-11-14T03:00:00Z";
const CHANGED_AT: &str = "2026-11-14T04:00:00Z";
const LIMIT: u32 = 50;
const UNMATCHED: u8 = u8::MAX;
const LISTING: &str = "states:OPEN";
const BUDGET: Duration = Duration::from_secs(20);
const SECONDARY_LIMIT: &str =
    "You have exceeded a secondary rate limit. Please wait a few minutes before you try again.";

fn head(number: u64) -> String {
    format!("{number:040x}")
}

fn identifier(number: u64) -> String {
    format!("PR_{number}")
}

fn number_of(id: &str) -> u64 {
    id.trim_start_matches("PR_").parse().unwrap()
}

fn open_row(number: u64, updated_at: &str) -> Value {
    json!({
        "id": identifier(number),
        "number": number,
        "title": format!("Change {number}"),
        "url": format!("{WEB}/pull/{number}"),
        "state": "OPEN",
        "isDraft": false,
        "updatedAt": updated_at,
        "headRefName": format!("feature-{number}"),
        "headRefOid": head(number),
        "baseRefName": "main",
        "author": {"__typename": "User", "login": "octocat"},
    })
}

/// What GitHub reads of pull request `number` in one evidence request: no
/// threads, no comments, and one passing check run on its head.
fn node(number: u64) -> Value {
    let mut node = open_row(number, UPDATED_AT);
    let evidence = json!({
        "repository": {"id": REPOSITORY_ID},
        "mergeStateStatus": "CLEAN",
        "reviewThreads": {"pageInfo": {"hasNextPage": false}, "nodes": []},
        "comments": {"totalCount": 0, "nodes": []},
        "commits": {"nodes": [{"commit": {
            "oid": head(number),
            "status": null,
            "checkSuites": {
                "pageInfo": {"hasNextPage": false},
                "nodes": [{
                    "app": {"slug": "ci-app"},
                    "status": "COMPLETED",
                    "conclusion": "SUCCESS",
                    "createdAt": UPDATED_AT,
                    "branch": {"name": format!("feature-{number}")},
                    "repository": {"id": REPOSITORY_ID},
                    "workflowRun": null,
                    "checkRuns": {
                        "pageInfo": {"hasNextPage": false},
                        "nodes": [{
                            "databaseId": 70 + number,
                            "name": "test",
                            "status": "COMPLETED",
                            "conclusion": "SUCCESS",
                            "url": format!("https://ci.example.com/{number}"),
                            "detailsUrl": format!("https://ci.example.com/{number}"),
                            "startedAt": UPDATED_AT,
                            "completedAt": UPDATED_AT,
                        }],
                    },
                }],
            },
        }}]},
    });
    for (field, value) in evidence.as_object().unwrap() {
        node[field] = value.clone();
    }
    node
}

fn overflowing(number: u64) -> Value {
    let mut node = node(number);
    node["reviewThreads"]["pageInfo"]["hasNextPage"] = json!(true);
    node
}

fn asked_ids(request: &Request) -> Vec<String> {
    let sent: Value = serde_json::from_slice(&request.body).unwrap();
    serde_json::from_value(sent["variables"]["ids"].clone()).unwrap_or_default()
}

fn asked_number(request: &Request) -> Option<u64> {
    let sent: Value = serde_json::from_slice(&request.body).ok()?;
    sent["variables"]["number"].as_u64()
}

fn now() -> DateTime<Utc> {
    UPDATED_AT.parse::<DateTime<Utc>>().unwrap() + chrono::TimeDelta::minutes(1)
}

fn repository() -> Repository {
    Repository {
        source_id: Uuid::nil(),
        name: format!("{OWNER}/{NAME}"),
        url: WEB.to_string(),
    }
}

/// A GitHub that fails the test on any request it was not told to expect.
async fn silent() -> MockServer {
    let server = MockServer::start().await;
    unmatched(&server).await;
    server
}

async fn unmatched(server: &MockServer) {
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .with_priority(UNMATCHED)
        .expect(0)
        .mount(server)
        .await;
}

async fn open(server: &MockServer, first: u64, rows: Vec<Value>, next: Option<&str>) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(LISTING))
        .and(body_partial_json(
            json!({"variables": {"owner": OWNER, "name": NAME, "first": first}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "repository": {
                "id": REPOSITORY_ID,
                "pullRequests": {
                    "pageInfo": {"hasNextPage": next.is_some(), "endCursor": next},
                    "nodes": rows,
                },
            },
        }})))
        .mount(server)
        .await;
}

async fn opened(server: &MockServer, numbers: impl IntoIterator<Item = u64>) {
    let rows: Vec<Value> = numbers
        .into_iter()
        .map(|number| open_row(number, UPDATED_AT))
        .collect();
    open(server, u64::from(LIMIT), rows, None).await;
}

/// An evidence read that answers each id asked with what `answer` says of
/// it.
async fn evidence(server: &MockServer, answer: impl Fn(&str) -> Value + Send + Sync + 'static) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"ids\""))
        .respond_with(move |request: &Request| {
            let nodes: Vec<Value> = asked_ids(request).iter().map(|id| answer(id)).collect();
            ResponseTemplate::new(200).set_body_json(json!({"data": {"nodes": nodes}}))
        })
        .mount(server)
        .await;
}

async fn evidenced(server: &MockServer) {
    evidence(server, |id| node(number_of(id))).await;
}

/// Everything GitHub's REST API, and the thread count query, answer for
/// pull request `number` read on its own: what `node` reads in a batch.
async fn rested(server: &MockServer, number: u64) {
    let rest = |endpoint: String| {
        Mock::given(method("GET")).and(path(format!("/repos/{OWNER}/{NAME}/{endpoint}")))
    };
    let answer = |body: Value| ResponseTemplate::new(200).set_body_json(body);
    rest(format!("pulls/{number}"))
        .respond_with(answer(json!({
            "node_id": identifier(number), "number": number, "draft": false,
            "state": "open", "mergeable": true, "mergeable_state": "clean",
            "title": format!("Change {number}"),
            "html_url": format!("{WEB}/pull/{number}"), "updated_at": UPDATED_AT,
            "user": {"login": "octocat"},
            "head": {"ref": format!("feature-{number}"), "sha": head(number)},
            "base": {"ref": "main", "sha": head(0)},
        })))
        .mount(server)
        .await;
    rest(format!("issues/{number}/comments"))
        .respond_with(answer(json!([])))
        .mount(server)
        .await;
    rest("actions/runs".to_string())
        .and(query_param("head_sha", head(number)))
        .respond_with(answer(json!({"workflow_runs": [], "total_count": 0})))
        .mount(server)
        .await;
    rest(format!("commits/{}/check-runs", head(number)))
        .respond_with(answer(json!({"total_count": 1, "check_runs": [{
            "id": 70 + number, "name": "test", "head_sha": head(number),
            "status": "completed", "conclusion": "success",
            "html_url": format!("https://ci.example.com/{number}"),
            "details_url": format!("https://ci.example.com/{number}"),
            "started_at": UPDATED_AT, "completed_at": UPDATED_AT,
        }]})))
        .mount(server)
        .await;
    rest(format!("commits/{}/status", head(number)))
        .respond_with(answer(json!({"statuses": [], "total_count": 0})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(json!({"variables": {"number": number}})))
        .respond_with(counted(number))
        .mount(server)
        .await;
}

fn counted(number: u64) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"data": {"repository": {"pullRequest": {
        "number": number,
        "reviewThreads": {"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": []},
    }}}}))
}

async fn rest_listed(server: &MockServer, numbers: impl IntoIterator<Item = u64>) {
    rest_listed_at(
        server,
        numbers.into_iter().map(|number| (number, UPDATED_AT)),
    )
    .await;
}

/// GitHub's REST listing of each pull request, last updated when it says.
async fn rest_listed_at<'a>(server: &MockServer, rows: impl IntoIterator<Item = (u64, &'a str)>) {
    let rows: Vec<Value> = rows
        .into_iter()
        .map(|(number, updated_at)| {
            json!({"node_id": identifier(number), "number": number, "state": "open",
                   "title": format!("Change {number}"), "draft": false,
                   "html_url": format!("{WEB}/pull/{number}"), "updated_at": updated_at,
                   "user": {"login": "octocat"},
                   "head": {"ref": format!("feature-{number}"), "sha": head(number)},
                   "base": {"ref": "main", "sha": head(0)}})
        })
        .collect();
    Mock::given(method("GET"))
        .and(path(format!("/repos/{OWNER}/{NAME}/pulls")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!(rows)))
        .mount(server)
        .await;
}

async fn received(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

async fn opens(server: &MockServer) -> usize {
    received(server)
        .await
        .iter()
        .filter(|request| String::from_utf8_lossy(&request.body).contains(LISTING))
        .count()
}

async fn batches(server: &MockServer) -> Vec<Vec<u64>> {
    let mut batches: Vec<Vec<u64>> = received(server)
        .await
        .iter()
        .filter(|request| String::from_utf8_lossy(&request.body).contains("\"ids\""))
        .map(|request| asked_ids(request).iter().map(|id| number_of(id)).collect())
        .collect();
    batches.sort();
    batches
}

async fn reads_of(server: &MockServer, endpoint: &str) -> usize {
    let endpoint = format!("/repos/{OWNER}/{NAME}/{endpoint}");
    received(server)
        .await
        .iter()
        .filter(|request| request.url.path() == endpoint)
        .count()
}

async fn surveyed(
    server: &MockServer,
    permits: &Semaphore,
    previous: Option<&Listing>,
    limit: u32,
) -> Result<Listing, Fault> {
    surveyed_within(server, permits, previous, limit, GRAPHQL_LIMIT).await
}

/// A listing whose evidence batches GitHub has `patience` to answer.
async fn surveyed_within(
    server: &MockServer,
    permits: &Semaphore,
    previous: Option<&Listing>,
    limit: u32,
    patience: Duration,
) -> Result<Listing, Fault> {
    surveyed_by(server, permits, previous, limit, patience, SURVEY_DEADLINE).await
}

/// A listing that has `patience` for each GraphQL read and `deadline` for
/// every assessment.
async fn surveyed_by(
    server: &MockServer,
    permits: &Semaphore,
    previous: Option<&Listing>,
    limit: u32,
    patience: Duration,
    deadline: Duration,
) -> Result<Listing, Fault> {
    let repository = repository();
    let survey = Survey::new(
        mocked(server),
        permits,
        &repository,
        previous,
        now(),
        patience,
        tokio::time::Instant::now() + deadline,
    );
    tokio::time::timeout(BUDGET, survey.listing(limit))
        .await
        .expect("a listing finishes")
}

fn numbers(listing: &Listing) -> Vec<u64> {
    listing
        .pull_requests
        .iter()
        .map(|pull_request| pull_request.number)
        .collect()
}

#[tokio::test]
async fn a_cold_listing_reads_one_page_and_a_batch_of_evidence_per_ten_pull_requests() {
    let server = silent().await;
    opened(&server, 1..=23).await;
    evidenced(&server).await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);

    let listing = surveyed(&server, &permits, None, LIMIT).await.unwrap();

    assert_eq!(numbers(&listing), (1..=23).collect::<Vec<u64>>());
    assert!(listing.complete);
    assert!(listing.warnings.is_empty(), "{:?}", listing.warnings);
    assert_eq!(opens(&server).await, 1);
    let sizes: Vec<usize> = batches(&server).await.iter().map(Vec::len).collect();
    assert_eq!(sizes.iter().sum::<usize>(), 23);
    assert_eq!(sizes.len(), 23usize.div_ceil(BATCH));
    assert!(sizes.iter().all(|size| *size <= BATCH), "{sizes:?}");
    assert_eq!(received(&server).await.len(), 1 + 23usize.div_ceil(BATCH));
    assert_eq!(permits.available_permits(), CREDENTIAL_PERMITS);
    server.verify().await;
}

#[tokio::test]
async fn evidence_read_in_a_batch_reaches_the_verdict_reading_each_alone_does() {
    let batched_server = silent().await;
    opened(&batched_server, 1..=3).await;
    evidenced(&batched_server).await;
    let rest_server = silent().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(LISTING))
        .respond_with(ResponseTemplate::new(404))
        .mount(&rest_server)
        .await;
    rest_listed(&rest_server, 1..=3).await;
    for number in 1..=3 {
        rested(&rest_server, number).await;
    }
    let permits = Semaphore::new(CREDENTIAL_PERMITS);

    let batched = surveyed(&batched_server, &permits, None, LIMIT)
        .await
        .unwrap();
    let rest = surveyed(&rest_server, &permits, None, LIMIT).await.unwrap();

    assert_eq!(batched, rest);
    assert!(
        batched
            .pull_requests
            .iter()
            .all(|pull_request| pull_request.checks.complete && pull_request.head.is_some()),
        "{batched:?}"
    );
    assert!(batches(&rest_server).await.is_empty());
    batched_server.verify().await;
    rest_server.verify().await;
}

#[tokio::test]
async fn a_warm_refresh_of_settled_pull_requests_reads_only_the_listing() {
    let server = silent().await;
    opened(&server, 1..=23).await;
    evidenced(&server).await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);
    let first = surveyed(&server, &permits, None, LIMIT).await.unwrap();
    let asked = received(&server).await.len();

    let again = surveyed(&server, &permits, Some(&first), LIMIT)
        .await
        .unwrap();

    assert_eq!(again, first);
    assert_eq!(received(&server).await.len(), asked + 1);
    assert_eq!(opens(&server).await, 2);
    server.verify().await;
}

#[tokio::test]
async fn a_pull_request_that_changed_is_the_only_one_whose_evidence_is_read_again() {
    let server = silent().await;
    opened(&server, 1..=3).await;
    evidenced(&server).await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);
    let first = surveyed(&server, &permits, None, LIMIT).await.unwrap();
    server.reset().await;
    unmatched(&server).await;
    open(
        &server,
        u64::from(LIMIT),
        vec![
            open_row(1, UPDATED_AT),
            open_row(2, CHANGED_AT),
            open_row(3, UPDATED_AT),
        ],
        None,
    )
    .await;
    evidenced(&server).await;

    let again = surveyed(&server, &permits, Some(&first), LIMIT)
        .await
        .unwrap();

    assert_eq!(numbers(&again), [1, 2, 3]);
    assert_eq!(opens(&server).await, 1);
    assert_eq!(batches(&server).await, [vec![2]]);
    assert_eq!(received(&server).await.len(), 2);
    server.verify().await;
}

#[tokio::test]
async fn a_listing_past_its_limit_keeps_the_most_recently_updated_pull_requests() {
    let server = silent().await;
    open(
        &server,
        2,
        vec![open_row(9, CHANGED_AT), open_row(4, UPDATED_AT)],
        Some("Y3Vyc29y"),
    )
    .await;
    evidenced(&server).await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);

    let listing = surveyed(&server, &permits, None, 2).await.unwrap();

    assert_eq!(numbers(&listing), [9, 4]);
    assert!(!listing.complete);
    assert_eq!(opens(&server).await, 1);
    server.verify().await;
}

#[tokio::test]
async fn a_pull_request_the_batch_could_not_vouch_for_is_read_through_rest_alone() {
    let server = silent().await;
    opened(&server, 1..=5).await;
    evidence(&server, |id| match number_of(id) {
        2 => Value::Null,
        3 => {
            let mut foreign = node(3);
            foreign["repository"]["id"] = json!(ELSEWHERE);
            foreign["mergeStateStatus"] = json!("DIRTY");
            foreign
        }
        4 => {
            let mut renumbered = node(4);
            renumbered["number"] = json!(40);
            renumbered["mergeStateStatus"] = json!("DIRTY");
            renumbered
        }
        number => node(number),
    })
    .await;
    for number in [2, 3, 4] {
        rested(&server, number).await;
    }
    let permits = Semaphore::new(CREDENTIAL_PERMITS);

    let listing = surveyed(&server, &permits, None, LIMIT).await.unwrap();

    assert_eq!(numbers(&listing), [1, 2, 3, 4, 5]);
    assert!(listing.complete, "{:?}", listing.warnings);
    assert!(
        listing
            .pull_requests
            .iter()
            .all(|pull_request| pull_request.mergeable == Mergeability::Clean),
        "nothing read for another repository or pull request is trusted: {listing:?}"
    );
    for (number, reads) in [(1, 0), (2, 1), (3, 1), (4, 1), (5, 0)] {
        assert_eq!(
            reads_of(&server, &format!("pulls/{number}")).await,
            reads,
            "#{number}"
        );
    }
    assert_eq!(reads_of(&server, "pulls").await, 0);
    server.verify().await;
}

#[tokio::test]
async fn a_batch_github_refuses_is_read_through_rest_for_its_own_pull_requests() {
    for (name, refusal) in [
        (
            "an undefined field",
            json!({"data": null, "errors": [{
                "message": "Field 'mergeStateStatus' doesn't exist on type 'PullRequest'",
                "extensions": {"code": "undefinedField"},
            }]}),
        ),
        (
            "a token without read access to checks",
            json!({"data": {"nodes": [null, null]}, "errors": [{
                "type": "FORBIDDEN",
                "path": ["nodes", 0, "commits"],
                "message": "Resource not accessible by integration",
            }]}),
        ),
    ] {
        let server = silent().await;
        opened(&server, 1..=12).await;
        let refused = refusal.clone();
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(
                json!({"variables": {"ids": [identifier(11), identifier(12)]}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(refused))
            .with_priority(1)
            .mount(&server)
            .await;
        evidenced(&server).await;
        for number in [11, 12] {
            rested(&server, number).await;
        }
        let permits = Semaphore::new(CREDENTIAL_PERMITS);

        let listing = surveyed(&server, &permits, None, LIMIT)
            .await
            .unwrap_or_else(|fault| panic!("{name}: {fault}"));

        assert_eq!(numbers(&listing), (1..=12).collect::<Vec<u64>>(), "{name}");
        assert!(listing.complete, "{name}: {:?}", listing.warnings);
        for number in 1..=12 {
            let reads = usize::from(number > 10);
            assert_eq!(
                reads_of(&server, &format!("pulls/{number}")).await,
                reads,
                "{name}: #{number}"
            );
        }
        server.verify().await;
    }
}

#[tokio::test]
async fn a_listing_graphql_cannot_answer_is_read_through_rest_in_full() {
    for (name, refusal) in [
        (
            "a schema without a field",
            ResponseTemplate::new(200).set_body_json(json!({"data": null, "errors": [{
                "message": "Field 'isDraft' doesn't exist on type 'PullRequest'",
                "extensions": {"code": "undefinedField"},
            }]})),
        ),
        ("no GraphQL endpoint", ResponseTemplate::new(404)),
    ] {
        let server = silent().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_string_contains(LISTING))
            .respond_with(refusal)
            .mount(&server)
            .await;
        rest_listed(&server, 1..=2).await;
        for number in 1..=2 {
            rested(&server, number).await;
        }
        let permits = Semaphore::new(CREDENTIAL_PERMITS);

        let listing = surveyed(&server, &permits, None, LIMIT)
            .await
            .unwrap_or_else(|fault| panic!("{name}: {fault}"));

        assert_eq!(numbers(&listing), [1, 2], "{name}");
        assert!(listing.complete, "{name}: {:?}", listing.warnings);
        assert!(
            listing
                .pull_requests
                .iter()
                .all(|pull_request| pull_request.readiness != Readiness::InProgress),
            "{name}"
        );
        assert_eq!(reads_of(&server, "pulls").await, 1, "{name}");
        assert!(batches(&server).await.is_empty(), "{name}");
        server.verify().await;
    }
}

#[tokio::test]
async fn a_secondary_rate_limit_halts_the_listing() {
    let limited = || ResponseTemplate::new(403).set_body_json(json!({"message": SECONDARY_LIMIT}));
    let server = silent().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(LISTING))
        .respond_with(limited())
        .mount(&server)
        .await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);

    let listing = surveyed(&server, &permits, None, LIMIT).await;

    assert!(
        matches!(listing, Err(Fault::RateLimited { .. })),
        "{listing:?}"
    );
    assert_eq!(reads_of(&server, "pulls").await, 0);
    server.verify().await;

    let server = silent().await;
    opened(&server, 1..=3).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"ids\""))
        .respond_with(limited())
        .mount(&server)
        .await;

    let listing = surveyed(&server, &permits, None, LIMIT).await;

    assert!(
        matches!(listing, Err(Fault::RateLimited { .. })),
        "{listing:?}"
    );
    assert_eq!(reads_of(&server, "pulls/1").await, 0);
    server.verify().await;
}

#[tokio::test]
async fn eight_batches_of_overflowing_pull_requests_finish_under_the_credentials_permits() {
    let count: u64 = 80;
    let server = silent().await;
    open(
        &server,
        u64::from(LIMIT * 2),
        (1..=count)
            .map(|number| open_row(number, UPDATED_AT))
            .collect(),
        None,
    )
    .await;
    evidence(&server, |id| overflowing(number_of(id))).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"number\""))
        .respond_with(|request: &Request| counted(asked_number(request).unwrap_or_default()))
        .mount(&server)
        .await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);

    let listing = surveyed(&server, &permits, None, LIMIT * 2).await.unwrap();

    assert_eq!(listing.pull_requests.len(), 80);
    assert!(listing.complete, "{:?}", listing.warnings);
    assert!(
        listing
            .pull_requests
            .iter()
            .all(|pull_request| pull_request.threads_complete),
        "every overflowing thread count was read in full"
    );
    assert_eq!(batches(&server).await.len(), 8);
    assert_eq!(permits.available_permits(), CREDENTIAL_PERMITS);
    server.verify().await;
}

#[tokio::test]
async fn a_kept_verdict_on_a_failing_pull_request_is_read_again_once_it_ages() {
    let server = silent().await;
    opened(&server, [1]).await;
    evidence(&server, |id| {
        let mut failing = node(number_of(id));
        failing["commits"]["nodes"][0]["commit"]["checkSuites"]["nodes"][0]["checkRuns"]["nodes"]
            [0]["conclusion"] = json!("FAILURE");
        failing
    })
    .await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);
    let mut first = surveyed(&server, &permits, None, LIMIT).await.unwrap();
    assert!(
        first.pull_requests[0]
            .blockers
            .iter()
            .any(|blocker| blocker.code == BlockerCode::ChecksFailed),
        "{first:?}"
    );
    first.pull_requests[0].assessed_at = now() - chrono::TimeDelta::minutes(3);

    surveyed(&server, &permits, Some(&first), LIMIT)
        .await
        .unwrap();

    assert_eq!(batches(&server).await, [vec![1], vec![1]]);
    server.verify().await;
}

/// When each request of each kind reached GitHub.
#[derive(Clone, Default)]
struct Arrivals(Arc<Mutex<Vec<(&'static str, Instant)>>>);

impl Arrivals {
    fn record(&self, kind: &'static str) {
        self.0.lock().unwrap().push((kind, Instant::now()));
    }

    fn of(&self, kind: Option<&str>) -> Vec<Instant> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(arrived, _)| kind.is_none_or(|kind| kind == *arrived))
            .map(|(_, at)| *at)
            .collect()
    }

    /// The most requests that reached GitHub within `window` of one
    /// another, each held for longer than that, so all of them at once.
    fn peak(&self, kind: Option<&str>, window: Duration) -> usize {
        let arrivals = self.of(kind);
        arrivals
            .iter()
            .map(|start| {
                arrivals
                    .iter()
                    .filter(|at| **at >= *start && at.duration_since(*start) < window)
                    .count()
            })
            .max()
            .unwrap_or_default()
    }
}

const EVIDENCE: &str = "evidence";
const THREADS: &str = "threads";
const HELD: Duration = Duration::from_millis(300);
const OVERLAP: Duration = Duration::from_millis(200);
const PATIENCE: Duration = Duration::from_millis(200);
const SLOW: Duration = Duration::from_secs(3);

/// Evidence on every pull request, each with threads that overflow, and
/// the thread counts read again for each, held for [`HELD`], every arrival
/// recorded.
async fn recorded(
    server: &MockServer,
    arrivals: &Arrivals,
    held: impl Fn(&[String]) -> Duration + Send + Sync + 'static,
) {
    let recording = arrivals.clone();
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"ids\""))
        .respond_with(move |request: &Request| {
            recording.record(EVIDENCE);
            let ids = asked_ids(request);
            let nodes: Vec<Value> = ids.iter().map(|id| overflowing(number_of(id))).collect();
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"nodes": nodes}}))
                .set_delay(held(&ids))
        })
        .mount(server)
        .await;
    let recording = arrivals.clone();
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"number\""))
        .respond_with(move |request: &Request| {
            recording.record(THREADS);
            counted(asked_number(request).unwrap_or_default()).set_delay(HELD)
        })
        .mount(server)
        .await;
}

#[tokio::test]
async fn no_more_requests_reach_github_at_once_than_the_listing_allows() {
    let count: u64 = 80;
    let server = silent().await;
    open(
        &server,
        u64::from(LIMIT * 2),
        (1..=count)
            .map(|number| open_row(number, UPDATED_AT))
            .collect(),
        None,
    )
    .await;
    let arrivals = Arrivals::default();
    recorded(&server, &arrivals, |_| HELD).await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);

    let listing = surveyed(&server, &permits, None, LIMIT * 2).await.unwrap();

    assert_eq!(listing.pull_requests.len(), 80);
    assert!(listing.complete, "{:?}", listing.warnings);
    let evidence = arrivals.peak(Some(EVIDENCE), OVERLAP);
    let every = arrivals.peak(None, OVERLAP);
    assert!(
        evidence <= EVIDENCE_CONCURRENCY,
        "{evidence} evidence reads at once"
    );
    assert!(every <= CREDENTIAL_PERMITS, "{every} reads at once");
    assert!(every > 1, "reads ran one at a time");
    server.verify().await;
}

#[tokio::test]
async fn a_batch_is_assessed_as_soon_as_its_evidence_arrives() {
    let server = silent().await;
    opened(&server, 1..=20).await;
    let arrivals = Arrivals::default();
    recorded(&server, &arrivals, |ids| {
        match ids.contains(&identifier(20)) {
            true => SLOW,
            false => Duration::ZERO,
        }
    })
    .await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);

    let listing = surveyed(&server, &permits, None, LIMIT).await.unwrap();

    assert_eq!(listing.pull_requests.len(), 20);
    let slow = arrivals
        .of(Some(EVIDENCE))
        .into_iter()
        .max()
        .expect("both batches were read");
    let follow_ups = arrivals.of(Some(THREADS));
    assert_eq!(follow_ups.len(), 20);
    assert!(
        follow_ups
            .iter()
            .filter(|at| at.duration_since(slow) < SLOW)
            .count()
            >= 10,
        "the fast batch's follow-ups waited out the slow batch"
    );
    server.verify().await;
}

#[tokio::test]
async fn a_batch_github_takes_too_long_over_is_read_through_rest_and_stops_the_rest() {
    let server = silent().await;
    opened(&server, 1..=12).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"ids\""))
        .respond_with(|request: &Request| {
            let nodes: Vec<Value> = asked_ids(request)
                .iter()
                .map(|id| node(number_of(id)))
                .collect();
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"nodes": nodes}}))
                .set_delay(SLOW)
        })
        .mount(&server)
        .await;
    for number in 1..=12 {
        rested(&server, number).await;
    }
    let permits = Semaphore::new(1);
    let started = Instant::now();

    let listing = surveyed_within(&server, &permits, None, LIMIT, PATIENCE)
        .await
        .unwrap();

    assert!(started.elapsed() < SLOW, "{:?}", started.elapsed());
    assert_eq!(numbers(&listing), (1..=12).collect::<Vec<u64>>());
    assert!(listing.complete, "{:?}", listing.warnings);
    assert!(
        listing
            .pull_requests
            .iter()
            .all(|pull_request| pull_request.threads_complete && pull_request.checks.complete),
        "every pull request read through REST has a whole verdict: {listing:?}"
    );
    assert_eq!(
        batches(&server).await.len(),
        1,
        "the second batch was never sent"
    );
    for number in 1..=12 {
        assert_eq!(
            reads_of(&server, &format!("pulls/{number}")).await,
            1,
            "#{number}"
        );
    }
    assert_eq!(
        thread_counts(&server).await,
        12,
        "a batch that took too long leaves thread counts to be read"
    );
    server.verify().await;
}

#[tokio::test]
async fn a_batch_every_later_batch_would_fail_alike_stops_them_for_that_listing_only() {
    for (name, refusal) in [
        (
            "a token refused checks",
            ResponseTemplate::new(200).set_body_json(json!({
                "data": {"nodes": []},
                "errors": [{"type": "FORBIDDEN", "path": ["nodes", 0, "commits"],
                            "message": "Resource not accessible by integration"}],
            })),
        ),
        (
            "a schema without a field",
            ResponseTemplate::new(200).set_body_json(json!({"data": null, "errors": [{
                "message": "Field 'fullDatabaseId' doesn't exist on type 'IssueComment'",
                "extensions": {"code": "undefinedField"},
            }]})),
        ),
        ("a gateway that never answers", ResponseTemplate::new(502)),
    ] {
        let server = silent().await;
        opened(&server, 1..=30).await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_string_contains("\"ids\""))
            .respond_with(refusal)
            .mount(&server)
            .await;
        for number in 1..=30 {
            rested(&server, number).await;
        }
        let permits = Semaphore::new(1);

        let listing = surveyed(&server, &permits, None, LIMIT)
            .await
            .unwrap_or_else(|fault| panic!("{name}: {fault}"));

        assert_eq!(listing.pull_requests.len(), 30, "{name}");
        assert!(listing.complete, "{name}: {:?}", listing.warnings);
        let asked: Vec<u64> = batches(&server).await.into_iter().flatten().collect();
        assert!(
            asked.iter().all(|number| *number <= 10),
            "{name}: later batches were sent: {asked:?}"
        );
        for number in 1..=30 {
            assert_eq!(
                reads_of(&server, &format!("pulls/{number}")).await,
                1,
                "{name}: #{number}"
            );
        }
        let sent = batches(&server).await.len();

        surveyed(&server, &permits, None, LIMIT)
            .await
            .unwrap_or_else(|fault| panic!("{name}: {fault}"));

        assert_eq!(
            batches(&server).await.len(),
            sent * 2,
            "{name}: the next listing tries its first batch again"
        );
        server.verify().await;
    }
}

#[tokio::test]
async fn a_listing_finishes_under_a_single_permit() {
    let server = silent().await;
    opened(&server, 1..=23).await;
    evidence(&server, |id| match number_of(id) {
        number if number % 5 == 0 => Value::Null,
        number if number % 2 == 0 => overflowing(number),
        number => node(number),
    })
    .await;
    for number in (5..=20).step_by(5) {
        rested(&server, number).await;
    }
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"number\""))
        .respond_with(|request: &Request| counted(asked_number(request).unwrap_or_default()))
        .with_priority(UNMATCHED - 1)
        .mount(&server)
        .await;
    let permits = Semaphore::new(1);

    let listing = surveyed(&server, &permits, None, LIMIT).await.unwrap();

    assert_eq!(numbers(&listing), (1..=23).collect::<Vec<u64>>());
    assert!(listing.complete, "{:?}", listing.warnings);
    assert_eq!(permits.available_permits(), 1);
    server.verify().await;
}

#[tokio::test]
async fn a_listing_under_a_closed_credential_reads_no_evidence_and_assesses_nothing() {
    let server = silent().await;
    rest_listed(&server, 1..=2).await;
    let permits = Semaphore::new(0);
    permits.close();

    let listing = surveyed(&server, &permits, None, LIMIT).await.unwrap();

    assert!(listing.pull_requests.is_empty(), "{listing:?}");
    assert!(!listing.complete);
    assert_eq!(listing.warnings.len(), 2, "{:?}", listing.warnings);
    assert_eq!(opens(&server).await, 0);
    assert!(batches(&server).await.is_empty());
    server.verify().await;
}

const HURRIED: Duration = Duration::from_millis(500);

async fn thread_counts(server: &MockServer) -> usize {
    received(server)
        .await
        .iter()
        .filter(|request| String::from_utf8_lossy(&request.body).contains("\"number\""))
        .count()
}

/// The listing of `numbers` a GitHub that answers at once reads.
async fn settled(numbers: std::ops::RangeInclusive<u64>) -> Listing {
    let fast = silent().await;
    opened(&fast, numbers.clone()).await;
    evidenced(&fast).await;
    let listing = surveyed(&fast, &Semaphore::new(CREDENTIAL_PERMITS), None, LIMIT)
        .await
        .unwrap();
    assert!(listing.complete, "{:?}", listing.warnings);
    assert_eq!(listing.pull_requests.len(), numbers.count());
    listing
}

#[tokio::test]
async fn a_listing_page_github_is_slow_over_is_read_through_rest_in_time_and_changes_are_read() {
    let first = settled(1..=2).await;
    let server = silent().await;
    let rows: Vec<Value> = (1..=2).map(|number| open_row(number, UPDATED_AT)).collect();
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(LISTING))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"data": {"repository": {
                    "id": REPOSITORY_ID,
                    "pullRequests": {
                        "pageInfo": {"hasNextPage": false, "endCursor": null},
                        "nodes": rows,
                    },
                }}}))
                .set_delay(SLOW),
        )
        .mount(&server)
        .await;
    rest_listed_at(&server, [(1, UPDATED_AT), (2, CHANGED_AT)]).await;
    rested(&server, 2).await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);
    let started = Instant::now();

    let again = surveyed_within(&server, &permits, Some(&first), LIMIT, PATIENCE)
        .await
        .unwrap();

    assert!(started.elapsed() < SLOW, "{:?}", started.elapsed());
    assert_eq!(numbers(&again), [1, 2]);
    assert!(again.complete, "{:?}", again.warnings);
    assert_eq!(again.pull_requests[0], first.pull_requests[0]);
    let changed = &again.pull_requests[1];
    assert_eq!(changed.updated_at, CHANGED_AT, "the change was read again");
    assert!(!changed.threads_complete);
    assert!(
        changed
            .blockers
            .iter()
            .any(|blocker| blocker.code == BlockerCode::ThreadsIncomplete),
        "{:?}",
        changed.blockers
    );
    assert_eq!(opens(&server).await, 1);
    assert_eq!(reads_of(&server, "pulls").await, 1);
    assert_eq!(reads_of(&server, "pulls/1").await, 0);
    assert_eq!(reads_of(&server, "pulls/2").await, 1);
    assert_eq!(
        thread_counts(&server).await,
        0,
        "GraphQL is not asked to count threads once its listing took too long"
    );
    server.verify().await;
}

#[tokio::test]
async fn a_thread_count_github_is_slow_over_leaves_the_pull_request_listed_with_threads_unverified()
{
    let first = settled(1..=2).await;
    let server = silent().await;
    open(
        &server,
        u64::from(LIMIT),
        vec![open_row(1, CHANGED_AT), open_row(2, CHANGED_AT)],
        None,
    )
    .await;
    evidence(&server, |id| match number_of(id) {
        1 => overflowing(1),
        _ => Value::Null,
    })
    .await;
    rested(&server, 2).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"number\""))
        .respond_with(|request: &Request| {
            counted(asked_number(request).unwrap_or_default()).set_delay(SLOW)
        })
        .with_priority(1)
        .mount(&server)
        .await;
    let permits = Semaphore::new(1);
    let started = Instant::now();

    let again = surveyed_within(&server, &permits, Some(&first), LIMIT, PATIENCE)
        .await
        .unwrap();

    assert!(started.elapsed() < SLOW, "{:?}", started.elapsed());
    assert_eq!(first.pull_requests.len(), 2);
    assert!(again.complete, "{:?}", again.warnings);
    assert_eq!(numbers(&again), [1, 2]);
    for pull_request in &again.pull_requests {
        assert_eq!(
            pull_request.updated_at, CHANGED_AT,
            "#{}",
            pull_request.number
        );
        assert!(!pull_request.threads_complete, "#{}", pull_request.number);
        assert!(
            pull_request
                .blockers
                .iter()
                .any(|blocker| blocker.code == BlockerCode::ThreadsIncomplete),
            "#{}: {:?}",
            pull_request.number,
            pull_request.blockers
        );
    }
    assert_eq!(
        thread_counts(&server).await,
        1,
        "a thread count is not asked for once one took too long"
    );
    server.verify().await;
}

#[tokio::test]
async fn a_listing_past_its_deadline_keeps_the_last_verdicts_of_what_it_had_not_assessed() {
    let first = settled(1..=3).await;
    let server = silent().await;
    open(
        &server,
        u64::from(LIMIT),
        (1..=3).map(|number| open_row(number, CHANGED_AT)).collect(),
        None,
    )
    .await;
    evidence(&server, |id| match number_of(id) {
        1 => node(1),
        number => overflowing(number),
    })
    .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("\"number\""))
        .respond_with(|request: &Request| {
            counted(asked_number(request).unwrap_or_default()).set_delay(SLOW)
        })
        .mount(&server)
        .await;
    let permits = Semaphore::new(CREDENTIAL_PERMITS);
    let started = Instant::now();

    let again = surveyed_by(
        &server,
        &permits,
        Some(&first),
        LIMIT,
        GRAPHQL_LIMIT,
        HURRIED,
    )
    .await
    .unwrap();
    let cold = surveyed_by(&server, &permits, None, LIMIT, GRAPHQL_LIMIT, HURRIED)
        .await
        .unwrap();

    assert!(started.elapsed() < SLOW * 2, "{:?}", started.elapsed());
    assert!(!again.complete);
    assert_eq!(numbers(&again), [1, 2, 3]);
    assert_eq!(again.pull_requests[0].updated_at, CHANGED_AT);
    assert_eq!(again.pull_requests[1..], first.pull_requests[1..]);
    assert_eq!(again.warnings.len(), 1, "{:?}", again.warnings);
    assert!(
        again.warnings[0].contains("2 pull requests"),
        "{:?}",
        again.warnings
    );
    assert!(!cold.complete);
    assert_eq!(numbers(&cold), [1]);
    assert_eq!(permits.available_permits(), CREDENTIAL_PERMITS);
    server.verify().await;
}

#[tokio::test]
async fn a_thread_count_github_halts_on_halts_the_listing_under_the_guard() {
    for (name, halt, expected) in [
        (
            "a secondary rate limit",
            ResponseTemplate::new(403).set_body_json(json!({"message": SECONDARY_LIMIT})),
            Fault::RateLimited {
                retry_after: None,
                exhausted: false,
            },
        ),
        (
            "a credential GitHub refuses",
            ResponseTemplate::new(401).set_body_json(json!({"message": "Bad credentials"})),
            Fault::Unauthenticated,
        ),
    ] {
        let server = silent().await;
        opened(&server, [1]).await;
        evidence(&server, |id| overflowing(number_of(id))).await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_string_contains("\"number\""))
            .respond_with(halt)
            .expect(1)
            .mount(&server)
            .await;
        let permits = Semaphore::new(CREDENTIAL_PERMITS);

        let listing = surveyed(&server, &permits, None, LIMIT).await;

        assert_eq!(listing, Err(expected), "{name}");
        server.verify().await;
    }
}
