use serde_json::Value;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
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
        FOREIGN_JOB
    );
}
