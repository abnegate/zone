use std::num::NonZeroU64;

use abnegate_vcs::BranchName;
use chrono::DateTime;
use chrono::Utc;
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::any;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::matchers::query_param;

use super::*;
use crate::services::github::client::PER_PAGE;
use crate::services::github::evidence::CheckKind;
use crate::services::github::evidence::Depth;
use crate::services::github::pull_requests::PullRequest;
use crate::services::github::pull_requests::Repository;
use crate::services::github::readiness::BlockerCode;
use crate::services::github::readiness::Mergeability;
use crate::services::github::testing::BLOB;
use crate::services::github::testing::COMMIT;
use crate::services::github::testing::mocked;
use crate::services::github::testing::pull_detail;
use crate::services::github::testing::summary;
use abnegate_vcs::CheckRun;
use abnegate_vcs::Collected;
use abnegate_vcs::CommitSha as Revision;
use abnegate_vcs::CommitStatus;
use abnegate_vcs::IssueComment;
use abnegate_vcs::MergeableState;
use abnegate_vcs::WorkflowRun;

const NUMBER: u64 = 7;
const SOURCE: &str = "owner/repository";
const WEB: &str = "https://github.com/owner/repository";
const UPDATED_AT: &str = "2026-09-05T00:00:00Z";
const UNMATCHED: u8 = u8::MAX;
const GREPTILE: &str = "greptile-apps";
const PERSON: &str = "octocat";

#[derive(Clone)]
struct Fixture {
    name: &'static str,
    draft: bool,
    mergeable: &'static str,
    head: bool,
    threads: Vec<bool>,
    comments: Vec<IssueComment>,
    workflows: Vec<WorkflowRun>,
    checks: Vec<CheckRun>,
    statuses: Vec<CommitStatus>,
    blocker: Option<BlockerCode>,
    details: usize,
}

struct Parts {
    mergeable: MergeableState,
    threads: ReviewThreadCount,
    comments: Collected<IssueComment>,
    head: Option<HeadChecks>,
}

fn commit() -> Revision {
    Revision::parse(COMMIT).unwrap()
}

fn identifier(id: u64) -> NonZeroU64 {
    NonZeroU64::new(id).unwrap()
}

fn run(id: u64, event: &str, status: &str, conclusion: Option<&str>) -> WorkflowRun {
    let run = WorkflowRun::new(
        identifier(id),
        identifier(1),
        ".github/workflows/ci.yml",
        event,
        commit(),
        "2026-09-05T00:00:00Z",
        "2026-09-05T00:05:00Z",
        format!("{WEB}/actions/runs/{id}"),
        SOURCE,
    )
    .with_attempt(1)
    .with_name("CI")
    .with_status(status)
    .with_head_branch("feature")
    .with_started_at("2026-09-05T00:00:01Z");
    match conclusion {
        Some(conclusion) => run.with_conclusion(conclusion),
        None => run,
    }
}

fn check(id: u64, name: &str, status: &str, conclusion: Option<&str>) -> CheckRun {
    let check = CheckRun::new(status)
        .with_id(id)
        .with_name(name)
        .with_head_sha(commit())
        .with_url(format!("{WEB}/actions/runs/11/job/{id}"))
        .with_details_url(format!("{WEB}/actions/runs/11/job/{id}"))
        .with_started_at("2026-09-05T00:01:00Z");
    match conclusion {
        Some(conclusion) => check
            .with_conclusion(conclusion)
            .with_completed_at("2026-09-05T00:03:00Z"),
        None => check,
    }
}

fn status(context: &str, state: &str) -> CommitStatus {
    CommitStatus::new(context, state)
        .with_description(format!("{context} says {state}"))
        .with_target_url(format!("https://ci.example.com/{context}"))
        .with_created_at("2026-09-05T00:02:00Z")
}

fn comment(id: u64, author: &str, body: &str) -> IssueComment {
    IssueComment::new(
        id,
        author,
        body,
        format!("{WEB}/pull/{NUMBER}#issuecomment-{id}"),
        format!("2026-09-05T00:{:02}:{:02}Z", (id / 60) % 60, id % 60),
    )
}

/// `count` comments of people talking, then the review bot's summary, newest.
fn conversation(count: u64) -> Vec<IssueComment> {
    (1..=count)
        .map(|id| comment(id, PERSON, "Looks reasonable."))
        .chain(std::iter::once(comment(
            count + 1,
            GREPTILE,
            &summary(5, COMMIT),
        )))
        .collect()
}

fn ready() -> Fixture {
    Fixture {
        name: "ready",
        draft: false,
        mergeable: "clean",
        head: true,
        threads: vec![true],
        comments: conversation(0),
        workflows: vec![run(11, "pull_request", "completed", Some("success"))],
        checks: vec![check(21, "test", "completed", Some("success"))],
        statuses: Vec::new(),
        blocker: None,
        details: 2,
    }
}

fn concluded(name: &'static str, conclusion: &str, blocker: BlockerCode) -> Fixture {
    Fixture {
        name,
        checks: vec![
            check(21, "test", "completed", Some("success")),
            check(22, "lint", "completed", Some(conclusion)),
        ],
        blocker: Some(blocker),
        details: 3,
        ..ready()
    }
}

fn merging(name: &'static str, mergeable: &'static str, blocker: Option<BlockerCode>) -> Fixture {
    Fixture {
        name,
        mergeable,
        blocker,
        ..ready()
    }
}

fn drafted(name: &'static str, mergeable: &'static str) -> Fixture {
    Fixture {
        name,
        draft: true,
        mergeable,
        blocker: Some(BlockerCode::Draft),
        ..ready()
    }
}

fn fixtures() -> Vec<Fixture> {
    vec![
        ready(),
        Fixture {
            name: "pending",
            workflows: vec![
                run(11, "pull_request", "queued", None),
                run(12, "push", "in_progress", None),
            ],
            checks: vec![check(21, "test", "waiting", None)],
            blocker: Some(BlockerCode::ChecksPending),
            details: 3,
            ..ready()
        },
        Fixture {
            name: "failing",
            workflows: vec![run(11, "pull_request", "completed", Some("failure"))],
            blocker: Some(BlockerCode::ChecksFailed),
            ..ready()
        },
        concluded(
            "action required",
            "action_required",
            BlockerCode::ChecksFailed,
        ),
        concluded(
            "startup failure",
            "startup_failure",
            BlockerCode::ChecksFailed,
        ),
        concluded("stale", "stale", BlockerCode::ChecksFailed),
        concluded("neutral", "neutral", BlockerCode::ChecksIncomplete),
        concluded("skipped", "skipped", BlockerCode::ChecksIncomplete),
        Fixture {
            name: "unresolved threads",
            threads: vec![false, true, false],
            blocker: Some(BlockerCode::UnresolvedThreads),
            ..ready()
        },
        drafted("draft blocked", "blocked"),
        drafted("draft dirty", "dirty"),
        drafted("draft unstable", "unstable"),
        merging("dirty", "dirty", Some(BlockerCode::MergeConflict)),
        merging(
            "unknown",
            "unknown",
            Some(BlockerCode::MergeabilityUnverified),
        ),
        merging("behind", "behind", Some(BlockerCode::MergeBehind)),
        merging("blocked", "blocked", Some(BlockerCode::MergeBlocked)),
        merging("has hooks", "has_hooks", None),
        Fixture {
            name: "bot summary",
            comments: vec![comment(11, "greptile-apps[bot]", &summary(5, COMMIT))],
            ..ready()
        },
        Fixture {
            name: "account summary",
            comments: vec![comment(11, GREPTILE, &summary(4, COMMIT))],
            blocker: Some(BlockerCode::ConfidenceBelowRequirement),
            ..ready()
        },
        Fixture {
            name: "one hundred and one comments",
            comments: conversation(100),
            blocker: Some(BlockerCode::CommentsIncomplete),
            ..ready()
        },
        Fixture {
            name: "three pages of comments",
            comments: conversation(229),
            blocker: Some(BlockerCode::CommentsIncomplete),
            ..ready()
        },
        Fixture {
            name: "status contexts",
            statuses: vec![
                status("deploy", "success"),
                status("preview", "pending"),
                status("audit", "error"),
            ],
            blocker: Some(BlockerCode::ChecksFailed),
            details: 5,
            ..ready()
        },
        Fixture {
            name: "duplicate and re-run workflow runs",
            workflows: vec![
                run(13, "pull_request", "completed", Some("success")).with_attempt(2),
                run(12, "pull_request", "completed", Some("failure")),
                run(11, "push", "completed", Some("success")),
            ],
            details: 3,
            ..ready()
        },
        Fixture {
            name: "external check",
            checks: vec![
                CheckRun::new("completed")
                    .with_id(31)
                    .with_name("external")
                    .with_head_sha(commit())
                    .with_conclusion("success")
                    .with_details_url("https://ci.example.com/builds/31")
                    .with_completed_at("2026-09-05T00:03:00Z"),
            ],
            ..ready()
        },
        Fixture {
            name: "headless",
            head: false,
            blocker: Some(BlockerCode::HeadCommitUnverified),
            details: 0,
            ..ready()
        },
    ]
}

impl Fixture {
    fn row(&self, github: &Github) -> ListedPullRequest {
        let row = ListedPullRequest::new(
            github.reference(NUMBER).unwrap(),
            "PR_7",
            format!("{WEB}/pull/{NUMBER}"),
            BranchName::parse("feature").unwrap(),
            BranchName::parse("main").unwrap(),
        )
        .with_title("Ship the billing export")
        .with_draft(self.draft)
        .with_updated_at(UPDATED_AT)
        .with_author_login("octocat");
        if self.head {
            row.with_head_sha(commit())
        } else {
            row
        }
    }

    fn parts(&self) -> Parts {
        let resolved = self.threads.iter().filter(|resolved| **resolved).count();
        let page = usize::from(PER_PAGE.get());
        let newest = self.comments.len().saturating_sub(page);
        Parts {
            mergeable: serde_json::from_value(json!(self.mergeable)).unwrap(),
            threads: ReviewThreadCount::new(
                u32::try_from(self.threads.len() - resolved).unwrap(),
                u32::try_from(resolved).unwrap(),
                true,
            ),
            comments: Collected::new(self.comments[newest..].to_vec(), newest == 0),
            head: Some(HeadChecks::new(
                commit(),
                Collected::new(self.workflows.clone(), true),
                Collected::new(self.checks.clone(), true),
                Collected::new(self.statuses.clone(), true),
            )),
        }
    }

    /// Everything the REST reads of this pull request answer.
    async fn serve(&self, server: &MockServer) {
        let mut detail = pull_detail(COMMIT);
        detail["draft"] = json!(self.draft);
        detail["mergeable_state"] = json!(match self.draft {
            true => "draft",
            false => self.mergeable,
        });
        answer(server, &format!("pulls/{NUMBER}"), None, detail, None).await;
        self.serve_threads(server, None).await;
        self.serve_comments(server).await;
        self.serve_checks(server, None).await;
    }

    async fn serve_threads(&self, server: &MockServer, expected: Option<u64>) {
        let nodes: Vec<Value> = self
            .threads
            .iter()
            .enumerate()
            .map(|(index, resolved)| json!({"id": format!("PRRT_{index}"), "isResolved": resolved}))
            .collect();
        let mock = Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"repository": {"pullRequest": {
                    "number": NUMBER,
                    "reviewThreads": {
                        "pageInfo": {"hasNextPage": false, "endCursor": null},
                        "nodes": nodes
                    }
                }}}
            })));
        expecting(mock, expected).mount(server).await;
    }

    async fn serve_comments(&self, server: &MockServer) {
        let page = usize::from(PER_PAGE.get());
        let pages = self.comments.len() / page + 1;
        for number in 1..=pages {
            let start = ((number - 1) * page).min(self.comments.len());
            let end = (number * page).min(self.comments.len());
            let rows: Vec<Value> = self.comments[start..end].iter().map(rest_comment).collect();
            answer(
                server,
                &format!("issues/{NUMBER}/comments"),
                Some(("page", &number.to_string())),
                json!(rows),
                None,
            )
            .await;
        }
        answer(
            server,
            &format!("issues/{NUMBER}"),
            None,
            json!({"number": NUMBER, "title": "Ship the billing export", "state": "open",
                   "html_url": format!("{WEB}/pull/{NUMBER}"), "labels": [],
                   "comments": self.comments.len(), "pull_request": {},
                   "created_at": UPDATED_AT, "updated_at": UPDATED_AT}),
            None,
        )
        .await;
    }

    async fn serve_checks(&self, server: &MockServer, expected: Option<u64>) {
        let runs: Vec<Value> = self.workflows.iter().map(rest_run).collect();
        let checks: Vec<Value> = self.checks.iter().map(rest_check).collect();
        let statuses: Vec<Value> = self.statuses.iter().map(rest_status).collect();
        answer(
            server,
            "actions/runs",
            Some(("head_sha", COMMIT)),
            json!({"workflow_runs": runs, "total_count": runs.len()}),
            expected,
        )
        .await;
        answer(
            server,
            &format!("commits/{COMMIT}/check-runs"),
            None,
            json!({"check_runs": checks, "total_count": checks.len()}),
            expected,
        )
        .await;
        answer(
            server,
            &format!("commits/{COMMIT}/status"),
            None,
            json!({"statuses": statuses, "total_count": statuses.len()}),
            expected,
        )
        .await;
    }
}

fn rest_run(run: &WorkflowRun) -> Value {
    json!({"id": run.id, "workflow_id": run.workflow_id, "run_attempt": run.attempt,
           "name": run.name, "path": run.path, "event": run.event, "status": run.status,
           "conclusion": run.conclusion, "head_branch": run.head_branch,
           "head_sha": run.head_sha.as_str(), "created_at": run.created_at,
           "run_started_at": run.started_at, "updated_at": run.updated_at,
           "html_url": run.url, "repository": {"full_name": run.repository}})
}

fn rest_check(check: &CheckRun) -> Value {
    json!({"id": check.id, "name": check.name,
           "head_sha": check.head_sha.as_ref().map(Revision::as_str),
           "status": check.status, "conclusion": check.conclusion, "html_url": check.url,
           "details_url": check.details_url, "started_at": check.started_at,
           "completed_at": check.completed_at})
}

fn rest_status(status: &CommitStatus) -> Value {
    json!({"context": status.context, "state": status.state,
           "description": status.description, "target_url": status.target_url,
           "created_at": status.created_at})
}

fn rest_comment(comment: &IssueComment) -> Value {
    json!({"id": comment.id, "user": {"login": comment.author}, "body": comment.body,
           "html_url": comment.url, "created_at": comment.created_at})
}

fn expecting(mock: Mock, expected: Option<u64>) -> Mock {
    match expected {
        Some(times) => mock.expect(times),
        None => mock,
    }
}

async fn answer(
    server: &MockServer,
    endpoint: &str,
    query: Option<(&str, &str)>,
    body: Value,
    expected: Option<u64>,
) {
    let mock = Mock::given(method("GET")).and(path(format!("/repos/{SOURCE}/{endpoint}")));
    let mock = match query {
        Some((name, value)) => mock.and(query_param(name, value)),
        None => mock,
    };
    expecting(
        mock.respond_with(ResponseTemplate::new(200).set_body_json(body)),
        expected,
    )
    .mount(server)
    .await;
}

/// A GitHub that answers nothing it is not expected to be asked, and fails
/// the test when it is.
async fn silent() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(500))
        .with_priority(UNMATCHED)
        .expect(0)
        .mount(&server)
        .await;
    server
}

fn closed() -> Semaphore {
    let permits = Semaphore::new(0);
    permits.close();
    permits
}

fn repository() -> Repository {
    Repository {
        source_id: Uuid::nil(),
        name: SOURCE.to_string(),
        url: WEB.to_string(),
    }
}

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-05T01:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

async fn listed(
    github: &Github,
    permits: &Semaphore,
    row: &ListedPullRequest,
    parts: Parts,
) -> Result<PullRequest, Fault> {
    surveyed(github, permits, row, row.clone(), parts).await
}

/// `row` assessed from `parts`, read in the batch as `read`.
async fn surveyed(
    github: &Github,
    permits: &Semaphore,
    row: &ListedPullRequest,
    read: ListedPullRequest,
    parts: Parts,
) -> Result<PullRequest, Fault> {
    let evidence = PullRequestEvidence::new(read, parts.mergeable, parts.threads, parts.comments);
    let evidence = match parts.head {
        Some(head) => evidence.with_head(head),
        None => evidence,
    };
    github
        .assess_open(permits, row, evidence)
        .await
        .map(|assessed| PullRequest::assessed(assessed, &repository(), row, now()))
}

async fn reassessed(fixture: &Fixture) -> PullRequest {
    let server = MockServer::start().await;
    fixture.serve(&server).await;
    let github = mocked(&server);
    let row = fixture.row(&github);
    let assessed = github.assess(&row, None, Depth::Listing).await.unwrap();
    PullRequest::assessed(assessed, &repository(), &row, now())
}

#[tokio::test]
async fn evidence_read_in_a_batch_reaches_the_verdict_reading_alone_does() {
    for fixture in fixtures() {
        let rest = reassessed(&fixture).await;
        let silent = silent().await;
        let github = mocked(&silent);

        let batched = listed(&github, &closed(), &fixture.row(&github), fixture.parts())
            .await
            .unwrap_or_else(|fault| panic!("{}: {fault}", fixture.name));

        assert_eq!(batched, rest, "{}", fixture.name);
        match fixture.blocker {
            Some(code) => assert!(
                rest.blockers.iter().any(|blocker| blocker.code == code),
                "{}: {:?} lacks {code:?}",
                fixture.name,
                rest.blockers
            ),
            None => assert!(
                rest.blockers.is_empty(),
                "{}: {:?}",
                fixture.name,
                rest.blockers
            ),
        }
        assert_eq!(
            rest.check_details.len(),
            fixture.details,
            "{}",
            fixture.name
        );
        silent.verify().await;
    }
}

#[tokio::test]
async fn a_duplicate_workflow_run_is_weighed_once_however_it_was_read() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.name == "duplicate and re-run workflow runs")
        .unwrap();
    let silent = silent().await;
    let github = mocked(&silent);

    let batched = listed(&github, &closed(), &fixture.row(&github), fixture.parts())
        .await
        .unwrap();

    let workflows: Vec<&str> = batched
        .check_details
        .iter()
        .filter(|detail| detail.kind == CheckKind::Workflow)
        .filter_map(|detail| detail.url.as_deref())
        .collect();
    assert_eq!(
        workflows,
        [
            format!("{WEB}/actions/runs/13"),
            format!("{WEB}/actions/runs/11")
        ],
        "the newest run of a workflow for one event and branch stands for it"
    );
    assert!(batched.blockers.is_empty(), "{:?}", batched.blockers);
}

#[tokio::test]
async fn threads_the_batch_could_not_count_are_counted_once_more() {
    let fixture = Fixture {
        threads: vec![true, false],
        ..ready()
    };
    let rest = reassessed(&fixture).await;
    let server = silent().await;
    fixture.serve_threads(&server, Some(1)).await;
    let github = mocked(&server);
    let permits = Semaphore::new(1);
    let parts = Parts {
        threads: ReviewThreadCount::new(0, 1, false),
        ..fixture.parts()
    };

    let batched = listed(&github, &permits, &fixture.row(&github), parts)
        .await
        .unwrap();

    assert_eq!(batched, rest);
    assert_eq!(batched.unresolved_threads, 1);
    assert_eq!(permits.available_permits(), 1);
    server.verify().await;
}

#[tokio::test]
async fn checks_the_batch_could_not_hold_are_read_again_for_the_head() {
    let fixture = ready();
    let rest = reassessed(&fixture).await;
    let partial = |workflows: bool, checks: bool, statuses: bool| {
        Some(HeadChecks::new(
            commit(),
            Collected::new(fixture.workflows.clone(), workflows),
            Collected::new(fixture.checks.clone(), checks),
            Collected::new(fixture.statuses.clone(), statuses),
        ))
    };
    let foreign = Some(HeadChecks::new(
        Revision::parse(BLOB).unwrap(),
        Collected::new(Vec::new(), true),
        Collected::new(Vec::new(), true),
        Collected::new(Vec::new(), true),
    ));
    for (name, head) in [
        ("overflowing suites", partial(false, true, true)),
        ("overflowing check runs", partial(true, false, true)),
        ("overflowing statuses", partial(true, true, false)),
        ("another head", foreign),
        ("no head", None),
    ] {
        let server = silent().await;
        fixture.serve_checks(&server, Some(1)).await;
        let github = mocked(&server);
        let permits = Semaphore::new(1);
        let parts = Parts {
            head,
            ..fixture.parts()
        };

        let batched = listed(&github, &permits, &fixture.row(&github), parts)
            .await
            .unwrap_or_else(|fault| panic!("{name}: {fault}"));

        assert_eq!(batched, rest, "{name}");
        assert_eq!(permits.available_permits(), 1, "{name}");
        server.verify().await;
    }
}

#[tokio::test]
async fn a_follow_up_read_waits_for_a_permit_and_a_whole_read_never_does() {
    let fixture = ready();
    let silent = silent().await;
    let github = mocked(&silent);
    let parts = Parts {
        threads: ReviewThreadCount::new(0, 1, false),
        ..fixture.parts()
    };

    let refused = listed(&github, &closed(), &fixture.row(&github), parts).await;
    let whole = listed(&github, &closed(), &fixture.row(&github), fixture.parts()).await;

    assert!(
        refused.is_err(),
        "an overflow is read again only under a permit"
    );
    assert!(whole.is_ok(), "{:?}", whole.err());
    silent.verify().await;
}

#[tokio::test]
async fn overflowing_threads_and_checks_share_one_permit() {
    let fixture = Fixture {
        threads: vec![false],
        ..ready()
    };
    let rest = reassessed(&fixture).await;
    let server = silent().await;
    fixture.serve_threads(&server, Some(1)).await;
    fixture.serve_checks(&server, Some(1)).await;
    let github = mocked(&server);
    let permits = Semaphore::new(1);
    let parts = Parts {
        threads: ReviewThreadCount::new(0, 0, false),
        head: None,
        ..fixture.parts()
    };

    let batched = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        listed(&github, &permits, &fixture.row(&github), parts),
    )
    .await
    .expect("one permit is enough for every follow-up a pull request needs")
    .unwrap();

    assert_eq!(batched, rest);
    assert_eq!(permits.available_permits(), 1);
    server.verify().await;
}

/// The REST verdict on `fixture` when reading it by number finds `detail`.
async fn reassessed_reading(fixture: &Fixture, detail: Value) -> PullRequest {
    let server = MockServer::start().await;
    answer(&server, &format!("pulls/{NUMBER}"), None, detail, None).await;
    fixture.serve(&server).await;
    let github = mocked(&server);
    let row = fixture.row(&github);
    let assessed = github.assess(&row, None, Depth::Listing).await.unwrap();
    PullRequest::assessed(assessed, &repository(), &row, now())
}

#[tokio::test]
async fn a_head_that_moved_after_the_listing_is_never_verified_or_checked() {
    let fixture = ready();
    let rest = reassessed_reading(&fixture, pull_detail(BLOB)).await;
    let silent = silent().await;
    let github = mocked(&silent);
    let row = fixture.row(&github);
    let moved = row.clone().with_head_sha(Revision::parse(BLOB).unwrap());
    let parts = Parts {
        head: Some(HeadChecks::new(
            Revision::parse(BLOB).unwrap(),
            Collected::new(Vec::new(), true),
            Collected::new(Vec::new(), true),
            Collected::new(Vec::new(), true),
        )),
        ..fixture.parts()
    };

    let batched = surveyed(&github, &closed(), &row, moved, parts)
        .await
        .unwrap();

    assert_eq!(batched, rest);
    assert_eq!(batched.head, None);
    assert!(
        batched
            .blockers
            .iter()
            .any(|blocker| blocker.code == BlockerCode::HeadCommitUnverified),
        "{:?}",
        batched.blockers
    );
    silent.verify().await;
}

#[tokio::test]
async fn a_pull_request_the_batch_reads_as_a_draft_is_one_though_listed_ready() {
    let fixture = ready();
    let mut detail = pull_detail(COMMIT);
    detail["draft"] = json!(true);
    detail["mergeable_state"] = json!("draft");
    let rest = reassessed_reading(&fixture, detail).await;
    let silent = silent().await;
    let github = mocked(&silent);
    let row = fixture.row(&github);
    let drafted = row.clone().with_draft(true);

    let batched = surveyed(&github, &closed(), &row, drafted, fixture.parts())
        .await
        .unwrap();

    assert_eq!(batched, rest);
    assert!(batched.draft);
    assert!(
        batched
            .blockers
            .iter()
            .any(|blocker| blocker.code == BlockerCode::Draft),
        "{:?}",
        batched.blockers
    );
    silent.verify().await;
}

#[tokio::test]
async fn a_draft_the_batch_reads_a_merge_state_for_is_blocked_only_as_a_draft() {
    for (name, state) in [
        ("blocked", MergeableState::Blocked),
        ("dirty", MergeableState::Dirty),
        ("behind", MergeableState::Behind),
        ("unknown", MergeableState::Unknown),
    ] {
        let silent = silent().await;
        let github = mocked(&silent);
        let fixture = Fixture {
            draft: true,
            ..ready()
        };
        let parts = Parts {
            mergeable: state,
            ..fixture.parts()
        };

        let batched = listed(&github, &closed(), &fixture.row(&github), parts)
            .await
            .unwrap_or_else(|fault| panic!("{name}: {fault}"));

        assert_eq!(batched.mergeable, Mergeability::Draft, "{name}");
        let codes: Vec<BlockerCode> = batched
            .blockers
            .iter()
            .map(|blocker| blocker.code)
            .collect();
        assert_eq!(codes, [BlockerCode::Draft], "{name}");
        silent.verify().await;
    }
}

#[tokio::test]
async fn checks_the_batch_read_on_another_commit_are_read_again_for_the_head() {
    let fixture = ready();
    let rest = reassessed(&fixture).await;
    let elsewhere = || Revision::parse(BLOB).unwrap();
    for (name, workflows, checks) in [
        (
            "a check run",
            fixture.workflows.clone(),
            vec![check(21, "test", "completed", Some("success")).with_head_sha(elsewhere())],
        ),
        (
            "a workflow run",
            vec![
                WorkflowRun::new(
                    identifier(11),
                    identifier(1),
                    ".github/workflows/ci.yml",
                    "pull_request",
                    elsewhere(),
                    "2026-09-05T00:00:00Z",
                    "2026-09-05T00:05:00Z",
                    format!("{WEB}/actions/runs/11"),
                    SOURCE,
                )
                .with_status("completed")
                .with_conclusion("success"),
            ],
            fixture.checks.clone(),
        ),
    ] {
        let server = silent().await;
        fixture.serve_checks(&server, Some(1)).await;
        let github = mocked(&server);
        let permits = Semaphore::new(1);
        let parts = Parts {
            head: Some(HeadChecks::new(
                commit(),
                Collected::new(workflows, true),
                Collected::new(checks, true),
                Collected::new(fixture.statuses.clone(), true),
            )),
            ..fixture.parts()
        };

        let batched = listed(&github, &permits, &fixture.row(&github), parts)
            .await
            .unwrap_or_else(|fault| panic!("{name}: {fault}"));

        assert_eq!(batched, rest, "{name}");
        assert_eq!(permits.available_permits(), 1, "{name}");
        server.verify().await;
    }
}
