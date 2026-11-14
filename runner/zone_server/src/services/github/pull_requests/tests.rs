use chrono::DateTime;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::services::github::client::FOREIGN_JOB;
use crate::services::github::client::RATE_LIMIT_EXHAUSTED;
use crate::services::github::client::UNAUTHORIZED;
use crate::services::github::client::UNEXPECTED_RESPONSE;
use crate::services::github::client::status_error;
use crate::services::github::evidence::CheckDetail;
use crate::services::github::evidence::CheckKind;
use crate::services::github::readiness::BlockerCode;
use crate::services::github::readiness::BlockerReport;
use crate::services::github::readiness::CheckCounts;
use crate::services::github::readiness::CheckState;
use crate::services::github::readiness::CheckSummary;
use crate::services::github::readiness::CommitSha;
use crate::services::github::readiness::Confidence;
use crate::services::github::readiness::ReviewSummary;
use crate::services::github_app::Unauthenticated;

const SOURCE: &str = "6f2c1f8e-0d55-4a3c-9a43-2f1d4c1e7b10";
const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const ASSESSED_AT: &str = "2026-11-14T03:04:05Z";

fn moment(value: &str) -> DateTime<Utc> {
    value.parse().expect("an RFC 3339 moment")
}

fn pull_request(number: u64, readiness: Readiness) -> PullRequest {
    PullRequest {
        source_id: SOURCE.parse::<Uuid>().unwrap(),
        repository: "owner/repository".into(),
        repository_url: "https://github.com/owner/repository".into(),
        number,
        title: "Add the thing".into(),
        url: format!("https://github.com/owner/repository/pull/{number}"),
        author: Some("octocat".into()),
        updated_at: "2026-11-14T03:00:00Z".into(),
        draft: false,
        head: CommitSha::parse(HEAD),
        head_ref: "feature".into(),
        base_ref: "main".into(),
        readiness,
        blockers: vec![BlockerReport {
            code: BlockerCode::ChecksPending,
            detail: "2 checks are still running.".into(),
        }],
        checks: CheckSummary {
            state: CheckState::Pending,
            complete: true,
            counts: CheckCounts {
                total: 3,
                passed: 1,
                failed: 0,
                running: 1,
                queued: 1,
                in_progress: 0,
                unknown: 0,
            },
        },
        check_details: vec![CheckDetail {
            kind: CheckKind::CheckRun,
            id: Some(77),
            name: "test".into(),
            state: CheckState::Pending,
            url: None,
            updated_at: Some("2026-11-14T03:01:00Z".into()),
        }],
        reviews: vec![ReviewSummary {
            reviewer: "greptile-apps".into(),
            comment_id: 12,
            url: None,
            confidence: Some(Confidence { score: 5, scale: 5 }),
            reviewed_commit: CommitSha::parse(HEAD),
            created_at: "2026-11-14T02:00:00Z".into(),
        }],
        review_current: true,
        unresolved_threads: 2,
        threads_complete: true,
        comments_complete: false,
        assessed_at: moment(ASSESSED_AT),
        rank: 1,
    }
}

#[test]
fn a_pull_request_row_serialises_as_the_console_reads_it() {
    let encoded = serde_json::to_value(pull_request(7, Readiness::InProgress)).unwrap();

    assert_eq!(
        encoded,
        json!({
            "source_id": SOURCE,
            "repository": "owner/repository",
            "repository_url": "https://github.com/owner/repository",
            "number": 7,
            "title": "Add the thing",
            "url": "https://github.com/owner/repository/pull/7",
            "author": "octocat",
            "updated_at": "2026-11-14T03:00:00Z",
            "draft": false,
            "head": HEAD,
            "head_ref": "feature",
            "base_ref": "main",
            "readiness": "in_progress",
            "blockers": [{"code": "checks_pending", "detail": "2 checks are still running."}],
            "checks": {
                "state": "pending",
                "complete": true,
                "counts": {
                    "total": 3,
                    "passed": 1,
                    "failed": 0,
                    "running": 1,
                    "queued": 1,
                    "in_progress": 0,
                    "unknown": 0,
                },
            },
            "check_details": [{
                "kind": "check_run",
                "id": 77,
                "name": "test",
                "state": "pending",
                "url": null,
                "updated_at": "2026-11-14T03:01:00Z",
            }],
            "reviews": [{
                "reviewer": "greptile-apps",
                "comment_id": 12,
                "url": null,
                "confidence": {"score": 5, "scale": 5},
                "reviewed_commit": HEAD,
                "created_at": "2026-11-14T02:00:00Z",
            }],
            "review_current": true,
            "unresolved_threads": 2,
            "threads_complete": true,
            "comments_complete": false,
            "assessed_at": ASSESSED_AT,
            "rank": 1,
        })
    );
}

#[test]
fn an_unknown_author_and_head_serialise_as_null() {
    let mut unidentified = pull_request(7, Readiness::NotReady);
    unidentified.author = None;
    unidentified.head = None;
    unidentified.check_details[0].id = None;
    let encoded = serde_json::to_value(unidentified).unwrap();

    assert_eq!(encoded["author"], json!(null));
    assert_eq!(encoded["head"], json!(null));
    assert_eq!(encoded["check_details"][0]["id"], json!(null));
    assert_eq!(encoded["readiness"], json!("not_ready"));
}

#[test]
fn every_readiness_and_check_kind_is_spelled_as_the_console_reads_it() {
    for (readiness, spelled) in [
        (Readiness::Ready, "ready"),
        (Readiness::InProgress, "in_progress"),
        (Readiness::NotReady, "not_ready"),
    ] {
        assert_eq!(serde_json::to_value(readiness).unwrap(), json!(spelled));
    }
    for (kind, spelled) in [
        (CheckKind::Workflow, "workflow"),
        (CheckKind::CheckRun, "check_run"),
        (CheckKind::Status, "status"),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(spelled));
    }
}

#[test]
fn ready_sorts_before_in_progress_before_not_ready() {
    let mut buckets = [Readiness::NotReady, Readiness::Ready, Readiness::InProgress];

    buckets.sort();

    assert_eq!(
        buckets,
        [Readiness::Ready, Readiness::InProgress, Readiness::NotReady]
    );
}

#[test]
fn counts_tally_each_bucket_and_the_total() {
    let listed = [
        pull_request(1, Readiness::Ready),
        pull_request(2, Readiness::NotReady),
        pull_request(3, Readiness::InProgress),
        pull_request(4, Readiness::NotReady),
    ];

    let counts = Counts::of(&listed);

    assert_eq!(
        serde_json::to_value(counts).unwrap(),
        json!({"ready": 1, "in_progress": 1, "not_ready": 2, "total": 4})
    );
    assert_eq!(Counts::of(listed.iter().take(0)), Counts::default());
}

#[test]
fn a_thread_and_its_comments_serialise_as_the_console_reads_them() {
    let thread = Thread {
        id: "PRRT_kwDO".into(),
        path: Some("src/lib.rs".into()),
        line: None,
        outdated: true,
        url: Some("https://github.com/owner/repository/pull/7#discussion_r1".into()),
        comments: vec![ThreadComment {
            id: None,
            author: "reviewer".into(),
            body: "Why?".into(),
            url: "https://github.com/owner/repository/pull/7#discussion_r1".into(),
            created_at: "2026-11-14T01:00:00Z".into(),
        }],
    };

    assert_eq!(
        serde_json::to_value(thread).unwrap(),
        json!({
            "id": "PRRT_kwDO",
            "path": "src/lib.rs",
            "line": null,
            "outdated": true,
            "url": "https://github.com/owner/repository/pull/7#discussion_r1",
            "comments": [{
                "id": null,
                "author": "reviewer",
                "body": "Why?",
                "url": "https://github.com/owner/repository/pull/7#discussion_r1",
                "created_at": "2026-11-14T01:00:00Z",
            }],
        })
    );
}

#[test]
fn a_bot_comment_serialises_as_the_console_reads_it() {
    let comment = BotComment {
        reviewer: "coderabbitai".into(),
        body: "Walkthrough".into(),
        url: "https://github.com/owner/repository/pull/7#issuecomment-1".into(),
        created_at: "2026-11-14T01:00:00Z".into(),
    };

    assert_eq!(
        serde_json::to_value(comment).unwrap(),
        json!({
            "reviewer": "coderabbitai",
            "body": "Walkthrough",
            "url": "https://github.com/owner/repository/pull/7#issuecomment-1",
            "created_at": "2026-11-14T01:00:00Z",
        })
    );
}

#[test]
fn commits_serialise_as_the_console_reads_them() {
    let commits = Commits {
        number: 7,
        head: HEAD.into(),
        base: BASE.into(),
        complete: false,
        commits: vec![Commit {
            sha: HEAD.into(),
            message: "Add the thing".into(),
            author_name: Some("Octo Cat".into()),
            author_login: None,
            authored_at: None,
            url: format!("https://github.com/owner/repository/commit/{HEAD}"),
        }],
    };

    assert_eq!(
        serde_json::to_value(commits).unwrap(),
        json!({
            "number": 7,
            "head": HEAD,
            "base": BASE,
            "complete": false,
            "commits": [{
                "sha": HEAD,
                "message": "Add the thing",
                "author_name": "Octo Cat",
                "author_login": null,
                "authored_at": null,
                "url": format!("https://github.com/owner/repository/commit/{HEAD}"),
            }],
        })
    );
}

#[test]
fn a_job_log_serialises_as_the_console_reads_it() {
    let log = Log {
        job_id: 77,
        run_id: None,
        head: HEAD.into(),
        text: "error: failed".into(),
        truncated: true,
        fetched_at: moment(ASSESSED_AT),
    };

    assert_eq!(
        serde_json::to_value(log).unwrap(),
        json!({
            "job_id": 77,
            "run_id": null,
            "head": HEAD,
            "text": "error: failed",
            "truncated": true,
            "fetched_at": ASSESSED_AT,
        })
    );
}

#[test]
fn a_merge_serialises_as_the_console_reads_it() {
    let merged = Merged {
        number: 7,
        sha: Some(HEAD.into()),
        admin: true,
        url: "https://github.com/owner/repository/pull/7".into(),
    };

    assert_eq!(
        serde_json::to_value(merged).unwrap(),
        json!({
            "number": 7,
            "sha": HEAD,
            "admin": true,
            "url": "https://github.com/owner/repository/pull/7",
        })
    );
}

#[test]
fn the_diff_and_log_budgets_are_four_and_one_mebibytes() {
    assert_eq!(DIFF_BUDGET, 4 * 1024 * 1024);
    assert_eq!(LOG_TAIL, 1024 * 1024);
}

#[test]
fn file_by_file_patches_stop_once_the_budget_is_spent() {
    let file = |name: &str, patch: &str| {
        abnegate_vcs::ChangedFile::new(name, abnegate_vcs::FileStatus::Modified, 1, 0)
            .with_patch(patch)
    };
    let files = [
        file("a.rs", "@@ -1 +1 @@\n+one"),
        file("b.rs", "@@ -1 +1 @@\n+two two two"),
        file("c.rs", "@@ -1 +1 @@\n+3"),
    ];

    let entries = reading::budgeted(&files, 30);

    let patched: Vec<bool> = entries.iter().map(|entry| entry.patch.is_some()).collect();
    assert_eq!(patched, [true, false, false]);
    assert!(
        reading::budgeted(&files, usize::MAX)
            .iter()
            .all(|entry| entry.patch.is_some())
    );
}

const UPDATED_AT: &str = "2026-11-14T03:00:00Z";

fn settled(number: u64, state: CheckState) -> PullRequest {
    PullRequest {
        checks: CheckSummary {
            state,
            complete: true,
            counts: CheckCounts::default(),
        },
        comments_complete: true,
        ..pull_request(number, Readiness::NotReady)
    }
}

fn row(number: u64, updated_at: &str, head: &str) -> ListedPullRequest {
    let repository = abnegate_vcs::Repository::new("owner", "repository").unwrap();
    ListedPullRequest::new(
        PullRequestReference::new(repository, NonZeroU64::new(number).unwrap()),
        format!("PR_{number}"),
        format!("https://github.com/owner/repository/pull/{number}"),
        abnegate_vcs::BranchName::parse("feature").unwrap(),
        abnegate_vcs::BranchName::parse("main").unwrap(),
    )
    .with_updated_at(updated_at)
    .with_head_sha(Revision::parse(head).unwrap())
}

fn listing(pull_requests: Vec<PullRequest>) -> Listing {
    Listing {
        pull_requests,
        complete: true,
        warnings: Vec::new(),
    }
}

fn kept(plan: &[Option<&PullRequest>]) -> Vec<bool> {
    plan.iter().map(Option::is_some).collect()
}

#[test]
fn a_settled_unchanged_pull_request_is_kept_and_everything_else_is_assessed_again() {
    let now = moment(ASSESSED_AT);
    let previous = listing(vec![
        settled(1, CheckState::Success),
        settled(2, CheckState::Success),
        settled(3, CheckState::Success),
        settled(4, CheckState::Pending),
        settled(5, CheckState::Unknown),
        PullRequest {
            threads_complete: false,
            ..settled(6, CheckState::Success)
        },
        PullRequest {
            checks: CheckSummary {
                complete: false,
                ..settled(7, CheckState::Success).checks
            },
            ..settled(7, CheckState::Success)
        },
    ]);
    let listed = [
        row(1, UPDATED_AT, HEAD),
        row(2, "2026-11-14T03:30:00Z", HEAD),
        row(3, UPDATED_AT, BASE),
        row(4, UPDATED_AT, HEAD),
        row(5, UPDATED_AT, HEAD),
        row(6, UPDATED_AT, HEAD),
        row(7, UPDATED_AT, HEAD),
        row(8, UPDATED_AT, HEAD),
    ];

    let plan = refresh_plan(Some(&previous), &listed, now);

    assert_eq!(
        kept(&plan),
        [true, false, false, false, false, false, false, false]
    );
    assert_eq!(plan[0].map(|kept| kept.number), Some(1));
}

#[test]
fn a_failed_pull_request_is_assessed_again_once_its_verdict_is_two_minutes_old() {
    let previous = listing(vec![settled(1, CheckState::Failure)]);
    let listed = [row(1, UPDATED_AT, HEAD)];
    let assessed_at = moment(ASSESSED_AT);

    for (elapsed, expected) in [(0, true), (120, true), (121, false)] {
        let now = assessed_at + TimeDelta::seconds(elapsed);
        assert_eq!(
            kept(&refresh_plan(Some(&previous), &listed, now)),
            [expected],
            "{elapsed} s after it was assessed"
        );
    }
}

#[test]
fn any_kept_verdict_is_assessed_again_once_it_is_five_minutes_old() {
    let assessed_at = moment(ASSESSED_AT);
    for state in [CheckState::Success, CheckState::None] {
        let previous = listing(vec![settled(1, state)]);
        let listed = [row(1, UPDATED_AT, HEAD)];
        for (elapsed, expected) in [(0, true), (300, true), (301, false)] {
            let now = assessed_at + TimeDelta::seconds(elapsed);
            assert_eq!(
                kept(&refresh_plan(Some(&previous), &listed, now)),
                [expected],
                "{state:?} {elapsed} s after it was assessed"
            );
        }
    }
}

#[test]
fn a_first_read_assesses_every_pull_request() {
    let listed = [row(1, UPDATED_AT, HEAD)];

    assert_eq!(
        kept(&refresh_plan(None, &listed, moment(ASSESSED_AT))),
        [false]
    );
}

#[test]
fn pull_requests_rank_ready_first_then_most_recently_updated() {
    let at = |number: u64, readiness: Readiness, updated_at: &str| PullRequest {
        updated_at: updated_at.into(),
        ..pull_request(number, readiness)
    };

    let ranked = ranked(vec![
        at(1, Readiness::NotReady, "2026-11-14T05:00:00Z"),
        at(2, Readiness::Ready, "2026-11-14T01:00:00Z"),
        at(3, Readiness::InProgress, "2026-11-14T02:00:00Z"),
        at(4, Readiness::Ready, "2026-11-14T04:00:00Z"),
        at(5, Readiness::InProgress, "2026-11-14T03:00:00Z"),
    ]);

    let order: Vec<(u64, u32)> = ranked
        .iter()
        .map(|pull_request| (pull_request.number, pull_request.rank))
        .collect();
    assert_eq!(order, [(4, 1), (2, 2), (5, 3), (3, 4), (1, 5)]);
    assert_eq!(
        Counts::of(&ranked),
        Counts {
            ready: 2,
            in_progress: 2,
            not_ready: 1,
            total: 5
        }
    );
}

#[test]
fn github_failures_read_as_missing_or_upstream() {
    for (error, expected) in [
        (
            PullRequestError::PullRequestNotFound,
            Error::Missing(status_error(404)),
        ),
        (
            PullRequestError::RepositoryNotFound,
            Error::Missing(status_error(404)),
        ),
        (
            PullRequestError::GitHubApi("test-secret".into()),
            Error::Upstream(Fault::upstream(UNEXPECTED_RESPONSE)),
        ),
        (
            PullRequestError::refused(503, "test-secret"),
            Error::Upstream(Fault::Upstream(status_error(503))),
        ),
        (
            PullRequestError::AuthenticationFailed,
            Error::Upstream(Fault::Unauthenticated),
        ),
        (
            PullRequestError::RateLimited,
            Error::Upstream(Fault::RateLimited {
                retry_after: None,
                exhausted: false,
            }),
        ),
    ] {
        let read = Error::from(error);
        assert_eq!(read, expected);
        assert!(!format!("{read:?}").contains("test-secret"));
    }
    assert!(Error::from(PullRequestError::AuthenticationFailed).unauthenticated());
    assert!(!Error::Upstream(Fault::Upstream(status_error(UNAUTHORIZED))).unauthenticated());
    assert_eq!(
        Error::from(Fault::missing(FOREIGN_JOB)),
        Error::Missing(FOREIGN_JOB.into())
    );
    assert_eq!(
        Error::from(Fault::RateLimited {
            retry_after: None,
            exhausted: true,
        }),
        Error::Upstream(Fault::RateLimited {
            retry_after: None,
            exhausted: true,
        })
    );
    assert_eq!(
        Fault::RateLimited {
            retry_after: None,
            exhausted: true,
        }
        .to_string(),
        RATE_LIMIT_EXHAUSTED
    );
}

#[test]
fn every_refusal_carries_the_code_the_console_reads() {
    for (refusal, code) in [
        (
            Refusal::HeadMoved {
                current_head: HEAD.into(),
            },
            "head_moved",
        ),
        (Refusal::NotReady { blockers: vec![] }, "not_ready"),
        (Refusal::Conflict, "conflict"),
        (Refusal::Protected, "protected"),
        (Refusal::Merged, "merged"),
        (Refusal::Closed, "closed"),
    ] {
        assert_eq!(refusal.code(), code);
        assert!(!refusal.message().is_empty());
    }
}

#[test]
fn only_a_refusal_about_what_changed_on_github_outdates_the_listing() {
    for (refusal, outdates) in [
        (
            Refusal::HeadMoved {
                current_head: HEAD.into(),
            },
            true,
        ),
        (Refusal::Conflict, true),
        (Refusal::Merged, true),
        (Refusal::Closed, true),
        (Refusal::NotReady { blockers: vec![] }, false),
        (Refusal::Protected, false),
    ] {
        assert_eq!(refusal.outdates(), outdates, "{refusal:?}");
    }
}
