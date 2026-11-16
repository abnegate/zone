use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::any;
use wiremock::matchers::body_partial_json;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::*;
use crate::services::github::client::FOREIGN_JOB;
use crate::services::github::diff::LineKind;
use crate::services::github::diff::PER_FILE_DIFF;
use crate::services::github::diff::Status;
use crate::services::github::testing::COMMIT;
use crate::services::github::testing::mock;
use crate::services::github::testing::mocked;

const HEAD: &str = "dddddddddddddddddddddddddddddddddddddddd";
const REPOSITORY_URL: &str = "https://github.com/owner/repository";
const DIFF_MEDIA_TYPE: &str = "application/vnd.github.diff";
const JOB: u64 = 77;
const RUN: u64 = 99;

fn revision() -> diff::Revision<'static> {
    diff::Revision {
        repository_url: REPOSITORY_URL,
        number: 7,
        head: HEAD,
        base: "main",
    }
}

fn listed_pull(number: u64) -> Value {
    json!({
        "node_id": format!("PR_{number}"),
        "number": number,
        "html_url": format!("{REPOSITORY_URL}/pull/{number}"),
        "state": "open",
        "title": format!("Change {number}"),
        "draft": false,
        "updated_at": "2026-11-14T03:00:00Z",
        "head": {"ref": "feature", "sha": HEAD},
        "base": {"ref": "main", "sha": COMMIT},
        "user": {"login": "octocat"},
    })
}

async fn requested(server: &MockServer, endpoint: &str) -> usize {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() == endpoint)
        .count()
}

#[tokio::test]
async fn open_pull_requests_are_read_up_to_the_limit_and_say_whether_that_was_all() {
    let server = MockServer::start().await;
    mock(
        &server,
        "pulls",
        json!([listed_pull(1), listed_pull(2), listed_pull(3)]),
    )
    .await;
    let github = mocked(&server);

    let (every, complete) = github.open_pulls(5).await.unwrap();
    assert_eq!(every.len(), 3);
    assert!(complete);
    assert_eq!(every[0].author_login.as_deref(), Some("octocat"));

    let (some, complete) = github.open_pulls(2).await.unwrap();
    let numbers: Vec<u64> = some
        .iter()
        .map(|row| row.reference.number().get())
        .collect();
    assert_eq!(numbers, [1, 2]);
    assert!(!complete);
}

#[tokio::test]
async fn a_diff_github_will_not_render_falls_back_to_each_files_patch() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repository/pulls/7"))
        .and(header("Accept", DIFF_MEDIA_TYPE))
        .respond_with(ResponseTemplate::new(406).set_body_json(json!({
            "message": "Sorry, the diff exceeded the maximum number of lines (20000)"
        })))
        .mount(&server)
        .await;
    mock(
        &server,
        "pulls/7/files",
        json!([
            {"filename": "src/basket.rs", "previous_filename": "src/cart.rs", "status": "renamed",
             "additions": 1, "deletions": 1, "patch": "@@ -1 +1 @@\n-cart\n+basket"},
            {"filename": "assets/logo.png", "status": "added", "additions": 0, "deletions": 0},
        ]),
    )
    .await;
    let github = mocked(&server);
    let reference = github.reference(7).unwrap();

    let refused = github
        .service()
        .fetch_diff(&reference, github.token(), budget())
        .await
        .unwrap_err();
    assert!(
        matches!(
            refused,
            PullRequestError::Refused {
                status: NOT_ACCEPTABLE,
                ..
            }
        ),
        "{refused:?}"
    );

    let diff = github.pull_diff(&reference, &revision()).await.unwrap();

    assert_eq!(
        requested(&server, "/repos/owner/repository/pulls/7/files").await,
        1
    );
    assert!(!diff.complete);
    assert_eq!(diff.warning.as_deref(), Some(PER_FILE_DIFF));
    assert_eq!(diff.files.len(), 2);
    let renamed = &diff.files[0];
    assert_eq!(renamed.status, Status::Renamed);
    assert_eq!(renamed.previous_path.as_deref(), Some("src/cart.rs"));
    let kinds: Vec<LineKind> = renamed.hunks[0]
        .lines
        .iter()
        .map(|line| line.kind)
        .collect();
    assert_eq!(kinds, [LineKind::Deletion, LineKind::Addition]);
    assert_eq!(
        renamed.blob_url,
        format!("{REPOSITORY_URL}/blob/{HEAD}/src/basket.rs")
    );
    assert!(diff.files[1].binary);
    assert!(diff.files[1].hunks.is_empty());
}

#[tokio::test]
async fn a_rendered_diff_is_parsed_whole_and_another_refusal_is_not_hidden() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repository/pulls/7"))
        .and(header("Accept", DIFF_MEDIA_TYPE))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "diff --git a/src/lib.rs b/src/lib.rs\nindex 1..2 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1,2 @@\n fn main() {}\n+fn added() {}\n",
        ))
        .mount(&server)
        .await;
    let github = mocked(&server);
    let reference = github.reference(7).unwrap();

    let diff = github.pull_diff(&reference, &revision()).await.unwrap();

    assert!(diff.complete);
    assert_eq!(diff.warning, None);
    assert_eq!(diff.files[0].path, "src/lib.rs");
    assert_eq!(diff.files[0].additions, 1);
    assert_eq!(
        requested(&server, "/repos/owner/repository/pulls/7/files").await,
        0
    );

    server.reset().await;
    Mock::given(method("GET"))
        .and(path("/repos/owner/repository/pulls/7"))
        .respond_with(ResponseTemplate::new(502))
        .mount(&server)
        .await;
    let failed = github.pull_diff(&reference, &revision()).await.unwrap_err();
    assert!(
        matches!(failed, PullRequestError::Refused { status: 502, .. }),
        "{failed:?}"
    );
    assert_eq!(
        requested(&server, "/repos/owner/repository/pulls/7/files").await,
        0
    );
}

#[tokio::test]
async fn a_pull_requests_commits_are_listed_with_their_authors() {
    let server = MockServer::start().await;
    mock(
        &server,
        "pulls/7/commits",
        json!([{
            "sha": HEAD,
            "html_url": format!("{REPOSITORY_URL}/commit/{HEAD}"),
            "commit": {
                "message": "feat: basket\n\nWhy.",
                "author": {"name": "Mona Lisa", "email": "mona@example.com", "date": "2026-11-14T01:00:00Z"},
            },
            "author": {"login": "octocat"},
        }]),
    )
    .await;
    let github = mocked(&server);
    let reference = github.reference(7).unwrap();
    let repository = Repository {
        source_id: uuid::Uuid::nil(),
        name: "owner/repository".into(),
        url: REPOSITORY_URL.into(),
    };

    let (commits, complete) = github
        .pull_commits(&reference, 1, &repository)
        .await
        .unwrap();
    assert!(complete);
    assert_eq!(
        commits,
        [Commit {
            sha: HEAD.into(),
            message: "feat: basket\n\nWhy.".into(),
            author_name: Some("Mona Lisa".into()),
            author_login: Some("octocat".into()),
            authored_at: Some("2026-11-14T01:00:00Z".into()),
            url: format!("{REPOSITORY_URL}/commit/{HEAD}"),
        }]
    );

    let (_, complete) = github
        .pull_commits(&reference, 2, &repository)
        .await
        .unwrap();
    assert!(!complete, "GitHub counted a commit it did not list");
}

async fn mock_job(server: &MockServer, head: &str) {
    mock(
        server,
        &format!("actions/jobs/{JOB}"),
        json!({
            "id": JOB,
            "run_id": RUN,
            "name": "test",
            "status": "completed",
            "conclusion": "failure",
            "head_sha": head,
            "html_url": format!("{REPOSITORY_URL}/actions/runs/{RUN}/job/{JOB}"),
        }),
    )
    .await;
}

#[tokio::test]
async fn a_job_log_is_read_for_the_pull_requests_head_only() {
    let server = MockServer::start().await;
    mock_job(&server, HEAD).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/owner/repository/actions/jobs/{JOB}/logs"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_string("error: the build failed\n"))
        .mount(&server)
        .await;
    let github = mocked(&server);
    let job = NonZeroU64::new(JOB).unwrap();
    let now: DateTime<Utc> = "2026-11-14T03:04:05Z".parse().unwrap();

    let log = github.job_log_tail(job, HEAD, now).await.unwrap();
    assert_eq!(
        log,
        Log {
            job_id: JOB,
            run_id: Some(RUN),
            head: HEAD.into(),
            text: "error: the build failed\n".into(),
            truncated: false,
            fetched_at: now,
        }
    );

    assert_eq!(
        github.job_log_tail(job, COMMIT, now).await.unwrap_err(),
        Fault::missing(FOREIGN_JOB)
    );
}

const REPOSITORY_ID: &str = "R_repository";
const CURSOR: &str = "Y3Vyc29y";
const LATER: &str = "bGF0ZXI=";

fn open_row(number: u64) -> Value {
    json!({
        "id": format!("PR_{number}"),
        "number": number,
        "title": format!("Change {number}"),
        "url": format!("{REPOSITORY_URL}/pull/{number}"),
        "state": "OPEN",
        "isDraft": false,
        "updatedAt": "2026-11-14T03:00:00Z",
        "headRefName": "feature",
        "headRefOid": HEAD,
        "baseRefName": "main",
        "author": {"__typename": "User", "login": "octocat"},
    })
}

/// A page of open pull requests `numbers` in the repository `id`, asked for
/// `first` at a time after `after`, followed by the page after `next`.
async fn open_page(
    server: &MockServer,
    id: &str,
    first: u64,
    after: Option<&str>,
    numbers: std::ops::RangeInclusive<u64>,
    next: Option<&str>,
) {
    let rows: Vec<Value> = numbers.map(open_row).collect();
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_partial_json(
            json!({"variables": {"first": first, "after": after}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {
            "repository": {
                "id": id,
                "pullRequests": {
                    "pageInfo": {"hasNextPage": next.is_some(), "endCursor": next},
                    "nodes": rows,
                },
            },
        }})))
        .expect(1)
        .mount(server)
        .await;
}

fn numbers(rows: &[ListedPullRequest]) -> Vec<u64> {
    rows.iter()
        .map(|row| row.reference.number().get())
        .collect()
}

#[tokio::test]
async fn a_second_page_asks_for_what_the_limit_leaves_after_the_first_cursor() {
    let server = MockServer::start().await;
    open_page(&server, REPOSITORY_ID, 100, None, 1..=100, Some(CURSOR)).await;
    open_page(
        &server,
        REPOSITORY_ID,
        20,
        Some(CURSOR),
        100..=120,
        Some(LATER),
    )
    .await;
    let github = mocked(&server);
    let permits = Semaphore::new(1);

    let (repository, rows, complete) = github.open_pull_requests(&permits, 120).await.unwrap();

    assert_eq!(repository, RepositoryId::new(REPOSITORY_ID));
    assert_eq!(
        numbers(&rows),
        (1..=120).collect::<Vec<u64>>(),
        "a pull request listed on both pages is listed once"
    );
    assert!(!complete, "more pages follow the limit");
    assert_eq!(permits.available_permits(), 1);
    server.verify().await;
}

#[tokio::test]
async fn a_last_page_inside_the_limit_is_every_open_pull_request() {
    let server = MockServer::start().await;
    open_page(&server, REPOSITORY_ID, 100, None, 1..=100, Some(CURSOR)).await;
    open_page(&server, REPOSITORY_ID, 50, Some(CURSOR), 101..=130, None).await;
    let github = mocked(&server);

    let (_, rows, complete) = github
        .open_pull_requests(&Semaphore::new(1), 150)
        .await
        .unwrap();

    assert_eq!(numbers(&rows), (1..=130).collect::<Vec<u64>>());
    assert!(complete);
    server.verify().await;
}

#[tokio::test]
async fn pages_of_another_repository_part_way_through_are_refused() {
    let server = MockServer::start().await;
    open_page(&server, REPOSITORY_ID, 100, None, 1..=100, Some(CURSOR)).await;
    open_page(&server, "R_elsewhere", 50, Some(CURSOR), 101..=130, None).await;
    let github = mocked(&server);

    let refused = github.open_pull_requests(&Semaphore::new(1), 150).await;

    assert_eq!(refused.err(), Some(Fault::upstream(MOVED_REPOSITORY)));
    server.verify().await;
}

fn listed_row(github: &Github, number: u64) -> ListedPullRequest {
    ListedPullRequest::new(
        github.reference(number).unwrap(),
        format!("PR_{number}"),
        format!("{REPOSITORY_URL}/pull/{number}"),
        abnegate_vcs::BranchName::parse("feature").unwrap(),
        abnegate_vcs::BranchName::parse("main").unwrap(),
    )
}

fn read(row: ListedPullRequest) -> EvidenceSlot {
    EvidenceSlot::Read(Box::new(PullRequestEvidence::new(
        row,
        abnegate_vcs::MergeableState::Clean,
        abnegate_vcs::ReviewThreadCount::new(0, 0, true),
        Collected::new(Vec::new(), true),
    )))
}

#[tokio::test]
async fn slots_count_only_when_github_answered_for_every_pull_request_asked() {
    let server = MockServer::start().await;
    let github = mocked(&server);
    let first = listed_row(&github, 1);
    let second = listed_row(&github, 2);
    let mut renumbered = second.clone();
    renumbered.reference = github.reference(9).unwrap();

    let short = github.vouched(vec![read(first.clone())], &[&first, &second]);
    let vouched = github.vouched(
        vec![read(first.clone()), read(renumbered)],
        &[&first, &second],
    );

    assert_eq!(short, [None, None], "a short answer vouches for none");
    assert!(vouched[0].is_some());
    assert!(vouched[1].is_none(), "a slot for another pull request");
}

#[tokio::test]
async fn a_batch_sent_once_the_listing_stopped_asks_github_for_nothing() {
    let server = MockServer::start().await;
    let github = mocked(&server);
    let row = listed_row(&github, 1);
    let stop = AtomicBool::new(true);

    let read = github
        .evidence(
            &Semaphore::new(1),
            &RepositoryId::new(REPOSITORY_ID),
            &[&row],
            &stop,
        )
        .await
        .unwrap();

    assert_eq!(read, [None]);
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// A GitHub that fails the test on any page it was not told to answer, so a
/// listing that keeps paging is caught rather than answered with a 404.
async fn paged() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .with_priority(u8::MAX)
        .expect(0)
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn a_page_that_lists_nothing_new_but_says_more_follow_stops_the_listing() {
    let empty = std::ops::RangeInclusive::new(1, 0);
    for (name, rows) in [("an empty page", empty), ("a page listed before", 1..=100)] {
        let server = paged().await;
        open_page(&server, REPOSITORY_ID, 100, None, 1..=100, Some(CURSOR)).await;
        open_page(&server, REPOSITORY_ID, 50, Some(CURSOR), rows, Some(LATER)).await;
        let github = mocked(&server);

        let stalled = github.open_pull_requests(&Semaphore::new(1), 150).await;

        assert_eq!(
            stalled.err(),
            Some(Fault::upstream(STALLED_CURSOR)),
            "{name}"
        );
        server.verify().await;
    }
}

#[tokio::test]
async fn a_page_that_names_the_cursor_it_was_read_after_stops_the_listing() {
    let server = paged().await;
    open_page(&server, REPOSITORY_ID, 100, None, 1..=100, Some(CURSOR)).await;
    open_page(
        &server,
        REPOSITORY_ID,
        50,
        Some(CURSOR),
        101..=110,
        Some(CURSOR),
    )
    .await;
    let github = mocked(&server);

    let stalled = github.open_pull_requests(&Semaphore::new(1), 150).await;

    let fault = stalled.expect_err("a cursor that never moves is never read after again");
    assert!(
        !fault.halts(),
        "the listing is read through REST instead: {fault}"
    );
    server.verify().await;
}
