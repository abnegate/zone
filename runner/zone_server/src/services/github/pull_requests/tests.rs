use chrono::DateTime;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::services::github::readiness::BlockerCode;
use crate::services::github::readiness::BlockerReport;
use crate::services::github::readiness::CheckCounts;
use crate::services::github::readiness::CheckState;
use crate::services::github::readiness::CheckSummary;
use crate::services::github::readiness::CommitSha;
use crate::services::github::readiness::Confidence;
use crate::services::github::readiness::ReviewSummary;

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
            id: 77,
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
    let encoded = serde_json::to_value(PullRequest {
        author: None,
        head: None,
        ..pull_request(7, Readiness::NotReady)
    })
    .unwrap();

    assert_eq!(encoded["author"], json!(null));
    assert_eq!(encoded["head"], json!(null));
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
fn the_diff_and_log_budgets_are_sixteen_and_one_mebibytes() {
    assert_eq!(DIFF_BUDGET, 16 * 1024 * 1024);
    assert_eq!(LOG_TAIL, 1024 * 1024);
}
