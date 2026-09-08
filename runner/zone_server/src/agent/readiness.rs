//! Pull request readiness: a verdict that only turns green when the evidence
//! behind it is internally consistent.
//!
//! A provider can report `success` while the check list it returned does not
//! add up, and a paginated fetch can stop early and still look complete. Both
//! read as a pass to anything that only inspects the reported state. This
//! module refuses that: counts that do not close collapse to unknown, a
//! partially fetched thread or comment list is a named blocker on its own, and
//! a review only counts when it names the current head commit.

use serde::Serialize;
use std::fmt;
use std::sync::LazyLock;

const SHA_LENGTH: usize = 40;
const SHORT_SHA_LENGTH: usize = 7;
const GREPTILE_REVIEWER: &str = "greptile-apps";
const GREPTILE_SCALE: u8 = 5;

/// A 40 character hexadecimal Git object name, lowercased.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CommitSha(String);

impl CommitSha {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        (value.len() == SHA_LENGTH && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| Self(value.to_ascii_lowercase()))
    }

    pub fn short(&self) -> &str {
        &self.0[..SHORT_SHA_LENGTH]
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The closed set of states a check summary may report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckState {
    Failure,
    None,
    Pending,
    Success,
    Unknown,
}

impl CheckState {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "failure" => Self::Failure,
            "none" => Self::None,
            "pending" => Self::Pending,
            "success" => Self::Success,
            "unknown" => Self::Unknown,
            _ => return Option::None,
        })
    }
}

/// Check tallies exactly as a provider reported them, before any validation.
///
/// The counts are signed so that an impossible report survives long enough to
/// be rejected rather than being made plausible by the type system.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckEvidence {
    pub state: String,
    pub complete: bool,
    pub total: i64,
    pub passed: i64,
    pub failed: i64,
    pub running: i64,
    pub queued: i64,
    pub in_progress: i64,
    pub unknown: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CheckCounts {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub running: u32,
    pub queued: u32,
    pub in_progress: u32,
    pub unknown: u32,
}

/// Check evidence that has been proven to add up, or collapsed to unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CheckSummary {
    pub state: CheckState,
    pub complete: bool,
    pub counts: CheckCounts,
}

impl CheckEvidence {
    pub fn normalize(&self) -> CheckSummary {
        let state = CheckState::parse(&self.state);
        let counts = self.counts();
        let complete = self.complete
            && counts.is_some()
            && matches!(state, Some(state) if state != CheckState::Unknown);
        CheckSummary {
            state: if complete {
                state.unwrap_or(CheckState::Unknown)
            } else {
                CheckState::Unknown
            },
            complete,
            counts: counts.unwrap_or_default(),
        }
    }

    fn counts(&self) -> Option<CheckCounts> {
        let total = u32::try_from(self.total).ok()?;
        let passed = u32::try_from(self.passed).ok()?;
        let failed = u32::try_from(self.failed).ok()?;
        let running = u32::try_from(self.running).ok()?;
        let queued = u32::try_from(self.queued).ok()?;
        let in_progress = u32::try_from(self.in_progress).ok()?;
        let unknown = u32::try_from(self.unknown).ok()?;
        let reported = failed
            .checked_add(passed)?
            .checked_add(running)?
            .checked_add(unknown)?;
        (reported == total && in_progress.checked_add(queued)? == running).then_some(CheckCounts {
            total,
            passed,
            failed,
            running,
            queued,
            in_progress,
            unknown,
        })
    }
}

/// A review score and the scale it was expressed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Confidence {
    pub score: u8,
    pub scale: u8,
}

impl Confidence {
    pub fn satisfies(self, required: Self) -> bool {
        self.scale == required.scale && self.score >= required.score
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.score, self.scale)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    pub id: u64,
    pub author: String,
    pub body: String,
    pub url: String,
    pub created_at: String,
}

/// The latest summary one review bot published on a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewSummary {
    pub reviewer: String,
    pub comment_id: u64,
    pub url: Option<String>,
    pub confidence: Option<Confidence>,
    pub reviewed_commit: Option<CommitSha>,
    pub created_at: String,
}

/// A review bot whose comments carry a confidence score and a reviewed commit.
pub trait ReviewSignal {
    fn reviewer(&self) -> &str;

    fn required(&self) -> Confidence;

    fn identifies(&self, body: &str) -> bool;

    fn confidence(&self, body: &str) -> Option<Confidence>;

    fn reviewed_commit(&self, body: &str) -> Option<CommitSha>;

    fn summarize(&self, comments: &[ReviewComment]) -> Option<ReviewSummary> {
        comments
            .iter()
            .filter(|comment| comment.author == self.reviewer() && self.identifies(&comment.body))
            .max_by_key(|comment| published_at(&comment.created_at))
            .map(|comment| ReviewSummary {
                reviewer: self.reviewer().to_string(),
                comment_id: comment.id,
                url: nonempty(&comment.url),
                confidence: self.confidence(&comment.body),
                reviewed_commit: self.reviewed_commit(&comment.body),
                created_at: comment.created_at.clone(),
            })
    }
}

static CONFIDENCE_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)Confidence\s+Score\s*:\s*(\d{1,3})\s*/\s*(\d{1,3})")
        .expect("confidence score pattern")
});
static REVIEWED_LINE_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)Last\s+reviewed\s+commit\s*:?[^\r\n]*")
        .expect("reviewed commit line pattern")
});
static REVIEWED_SHA_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)(?:^|[^0-9a-f])([0-9a-f]{40})(?:[^0-9a-f]|$)")
        .expect("reviewed commit pattern")
});
static MARKUP_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"<[^>]*>|&nbsp;|[*_`]").expect("markup pattern"));

/// Greptile publishes one summary comment carrying `Confidence Score: n/5` and
/// `Last reviewed commit: <sha>`.
pub struct Greptile;

impl ReviewSignal for Greptile {
    fn reviewer(&self) -> &str {
        GREPTILE_REVIEWER
    }

    fn required(&self) -> Confidence {
        Confidence {
            score: GREPTILE_SCALE,
            scale: GREPTILE_SCALE,
        }
    }

    fn identifies(&self, body: &str) -> bool {
        let readable = readable(body);
        CONFIDENCE_PATTERN.is_match(&readable) || REVIEWED_LINE_PATTERN.is_match(&readable)
    }

    fn confidence(&self, body: &str) -> Option<Confidence> {
        let readable = readable(body);
        let captures = CONFIDENCE_PATTERN.captures(&readable)?;
        let score: u8 = captures.get(1)?.as_str().parse().ok()?;
        let scale: u8 = captures.get(2)?.as_str().parse().ok()?;
        (scale > 0 && score <= scale).then_some(Confidence { score, scale })
    }

    fn reviewed_commit(&self, body: &str) -> Option<CommitSha> {
        let line = REVIEWED_LINE_PATTERN.find(body)?;
        let captures = REVIEWED_SHA_PATTERN.captures(line.as_str())?;
        CommitSha::parse(captures.get(1)?.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub resolved: bool,
}

/// Review threads and whether every page of them was retrieved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadEvidence {
    pub complete: bool,
    pub threads: Vec<ReviewThread>,
}

impl ThreadEvidence {
    pub fn unresolved(&self) -> u32 {
        self.threads
            .iter()
            .filter(|thread| !thread.resolved)
            .count()
            .try_into()
            .unwrap_or(u32::MAX)
    }
}

/// Bot comments and whether every page of them was retrieved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentEvidence {
    pub complete: bool,
    pub comments: Vec<ReviewComment>,
}

/// Everything observed about one pull request, before any judgement.
#[derive(Debug, Clone)]
pub struct PullEvidence {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub updated_at: String,
    pub draft: bool,
    pub head: Option<CommitSha>,
    pub threads: ThreadEvidence,
    pub comments: CommentEvidence,
    pub checks: CheckEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockerCode {
    Draft,
    HeadCommitUnverified,
    ThreadsIncomplete,
    CommentsIncomplete,
    UnresolvedThreads,
    ReviewMissing,
    ReviewLinkMissing,
    ConfidenceUnreadable,
    ConfidenceBelowRequirement,
    ReviewedCommitUnreadable,
    ReviewedCommitStale,
    ChecksFailed,
    ChecksPending,
    ChecksAbsent,
    ChecksIncomplete,
}

/// One reason a pull request is not ready, carrying the detail behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blocker {
    Draft,
    HeadCommitUnverified,
    ThreadsIncomplete,
    CommentsIncomplete,
    UnresolvedThreads(u32),
    ReviewMissing {
        reviewer: String,
    },
    ReviewLinkMissing {
        reviewer: String,
    },
    ConfidenceUnreadable {
        reviewer: String,
    },
    ConfidenceBelowRequirement {
        reviewer: String,
        observed: Confidence,
        required: Confidence,
    },
    ReviewedCommitUnreadable {
        reviewer: String,
    },
    ReviewedCommitStale {
        reviewer: String,
        reviewed: CommitSha,
        head: CommitSha,
    },
    ChecksFailed(CheckCounts),
    ChecksPending(CheckCounts),
    ChecksAbsent,
    ChecksIncomplete,
}

impl Blocker {
    pub fn code(&self) -> BlockerCode {
        match self {
            Self::Draft => BlockerCode::Draft,
            Self::HeadCommitUnverified => BlockerCode::HeadCommitUnverified,
            Self::ThreadsIncomplete => BlockerCode::ThreadsIncomplete,
            Self::CommentsIncomplete => BlockerCode::CommentsIncomplete,
            Self::UnresolvedThreads(_) => BlockerCode::UnresolvedThreads,
            Self::ReviewMissing { .. } => BlockerCode::ReviewMissing,
            Self::ReviewLinkMissing { .. } => BlockerCode::ReviewLinkMissing,
            Self::ConfidenceUnreadable { .. } => BlockerCode::ConfidenceUnreadable,
            Self::ConfidenceBelowRequirement { .. } => BlockerCode::ConfidenceBelowRequirement,
            Self::ReviewedCommitUnreadable { .. } => BlockerCode::ReviewedCommitUnreadable,
            Self::ReviewedCommitStale { .. } => BlockerCode::ReviewedCommitStale,
            Self::ChecksFailed(_) => BlockerCode::ChecksFailed,
            Self::ChecksPending(_) => BlockerCode::ChecksPending,
            Self::ChecksAbsent => BlockerCode::ChecksAbsent,
            Self::ChecksIncomplete => BlockerCode::ChecksIncomplete,
        }
    }

    pub fn report(&self) -> BlockerReport {
        BlockerReport {
            code: self.code(),
            detail: self.to_string(),
        }
    }
}

impl fmt::Display for Blocker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Draft => formatter.write_str("Pull request is still a draft"),
            Self::HeadCommitUnverified => {
                formatter.write_str("Head commit could not be established from the observations")
            }
            Self::ThreadsIncomplete => {
                formatter.write_str("Review threads could not be fully checked")
            }
            Self::CommentsIncomplete => {
                formatter.write_str("Review comments could not be fully checked")
            }
            Self::UnresolvedThreads(count) => write!(
                formatter,
                "{count} unresolved review {}",
                if *count == 1 { "thread" } else { "threads" }
            ),
            Self::ReviewMissing { reviewer } => write!(formatter, "{reviewer} summary missing"),
            Self::ReviewLinkMissing { reviewer } => {
                write!(formatter, "{reviewer} review link missing")
            }
            Self::ConfidenceUnreadable { reviewer } => {
                write!(formatter, "{reviewer} confidence missing or unreadable")
            }
            Self::ConfidenceBelowRequirement {
                reviewer,
                observed,
                required,
            } => write!(
                formatter,
                "{reviewer} confidence {observed}; {required} required"
            ),
            Self::ReviewedCommitUnreadable { reviewer } => write!(
                formatter,
                "{reviewer} last reviewed commit missing or unreadable"
            ),
            Self::ReviewedCommitStale {
                reviewer,
                reviewed,
                head,
            } => write!(
                formatter,
                "{reviewer} reviewed {}; head is {}",
                reviewed.short(),
                head.short()
            ),
            Self::ChecksFailed(counts) => write!(
                formatter,
                "{} of {} checks failed",
                counts.failed, counts.total
            ),
            Self::ChecksPending(counts) => write!(
                formatter,
                "{} of {} checks still running",
                counts.running, counts.total
            ),
            Self::ChecksAbsent => formatter.write_str("No checks were observed for this commit"),
            Self::ChecksIncomplete => {
                formatter.write_str("Checks could not be fully checked or did not add up")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockerReport {
    pub code: BlockerCode,
    pub detail: String,
}

/// The verdict for one pull request and every reason it is not ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub updated_at: String,
    pub draft: bool,
    pub head: Option<CommitSha>,
    pub ready: bool,
    pub blockers: Vec<Blocker>,
    pub checks: CheckSummary,
    pub review: Option<ReviewSummary>,
    pub review_current: bool,
    pub unresolved: u32,
    pub threads_complete: bool,
    pub comments_complete: bool,
}

impl Assessment {
    pub fn reports(&self) -> Vec<BlockerReport> {
        self.blockers.iter().map(Blocker::report).collect()
    }
}

/// Judge one pull request. `ready` is the conjunction of every positive
/// condition, so evidence that was never established can never satisfy it.
pub fn assess(evidence: PullEvidence, signal: &dyn ReviewSignal) -> Assessment {
    let checks = evidence.checks.normalize();
    let review = signal.summarize(&evidence.comments.comments);
    let unresolved = evidence.threads.unresolved();
    let threads_complete = evidence.threads.complete;
    let comments_complete = evidence.comments.complete;
    let reviewer = signal.reviewer().to_string();
    let required = signal.required();
    let reviewed = review
        .as_ref()
        .and_then(|summary| summary.reviewed_commit.clone());
    let review_current =
        matches!((&reviewed, &evidence.head), (Some(reviewed), Some(head)) if reviewed == head);
    let confidence = review.as_ref().and_then(|summary| summary.confidence);
    let review_url = review.as_ref().and_then(|summary| summary.url.clone());
    let mut blockers = Vec::new();

    if evidence.draft {
        blockers.push(Blocker::Draft);
    }
    if evidence.head.is_none() {
        blockers.push(Blocker::HeadCommitUnverified);
    }
    if !threads_complete {
        blockers.push(Blocker::ThreadsIncomplete);
    }
    if !comments_complete {
        blockers.push(Blocker::CommentsIncomplete);
    }
    if unresolved > 0 {
        blockers.push(Blocker::UnresolvedThreads(unresolved));
    }

    match &review {
        None => blockers.push(Blocker::ReviewMissing {
            reviewer: reviewer.clone(),
        }),
        Some(_) => {
            match confidence {
                None => blockers.push(Blocker::ConfidenceUnreadable {
                    reviewer: reviewer.clone(),
                }),
                Some(observed) if !observed.satisfies(required) => {
                    blockers.push(Blocker::ConfidenceBelowRequirement {
                        reviewer: reviewer.clone(),
                        observed,
                        required,
                    });
                }
                Some(_) => {}
            }
            match (&reviewed, &evidence.head) {
                (None, _) => blockers.push(Blocker::ReviewedCommitUnreadable {
                    reviewer: reviewer.clone(),
                }),
                (Some(reviewed), Some(head)) if reviewed != head => {
                    blockers.push(Blocker::ReviewedCommitStale {
                        reviewer: reviewer.clone(),
                        reviewed: reviewed.clone(),
                        head: head.clone(),
                    });
                }
                _ => {}
            }
            if review_url.is_none() {
                blockers.push(Blocker::ReviewLinkMissing {
                    reviewer: reviewer.clone(),
                });
            }
        }
    }

    match checks.state {
        CheckState::Failure => blockers.push(Blocker::ChecksFailed(checks.counts)),
        CheckState::Pending => blockers.push(Blocker::ChecksPending(checks.counts)),
        CheckState::None => blockers.push(Blocker::ChecksAbsent),
        CheckState::Unknown => blockers.push(Blocker::ChecksIncomplete),
        CheckState::Success => {}
    }

    let ready = !evidence.draft
        && evidence.head.is_some()
        && threads_complete
        && comments_complete
        && unresolved == 0
        && review.is_some()
        && review_url.is_some()
        && review_current
        && confidence.is_some_and(|observed| observed.satisfies(required))
        && checks.complete
        && checks.state == CheckState::Success;

    Assessment {
        number: evidence.number,
        title: evidence.title,
        url: evidence.url,
        updated_at: evidence.updated_at,
        draft: evidence.draft,
        head: evidence.head,
        ready,
        blockers,
        checks,
        review,
        review_current,
        unresolved,
        threads_complete,
        comments_complete,
    }
}

fn readable(body: &str) -> String {
    MARKUP_PATTERN
        .replace_all(body, " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn published_at(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|moment| moment.timestamp_millis())
        .unwrap_or(i64::MIN)
}

fn nonempty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OLDER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn green_checks() -> CheckEvidence {
        CheckEvidence {
            state: "success".into(),
            complete: true,
            total: 4,
            passed: 4,
            failed: 0,
            running: 0,
            queued: 0,
            in_progress: 0,
            unknown: 0,
        }
    }

    fn summary_body(score: u8, sha: &str) -> String {
        format!("## Greptile\n**Confidence Score:** {score}/5\n_Last reviewed commit: {sha}_\n")
    }

    fn comment(body: &str) -> ReviewComment {
        ReviewComment {
            id: 11,
            author: GREPTILE_REVIEWER.into(),
            body: body.to_string(),
            url: "https://github.com/owner/repository/pull/7#issuecomment-11".into(),
            created_at: "2026-09-05T00:00:00Z".into(),
        }
    }

    fn green_pull() -> PullEvidence {
        PullEvidence {
            number: 7,
            title: "Ship the billing export".into(),
            url: "https://github.com/owner/repository/pull/7".into(),
            updated_at: "2026-09-05T00:00:00Z".into(),
            draft: false,
            head: CommitSha::parse(HEAD),
            threads: ThreadEvidence {
                complete: true,
                threads: vec![ReviewThread { resolved: true }],
            },
            comments: CommentEvidence {
                complete: true,
                comments: vec![comment(&summary_body(5, HEAD))],
            },
            checks: green_checks(),
        }
    }

    #[test]
    fn counts_that_do_not_sum_collapse_to_unknown() {
        let summary = CheckEvidence {
            total: 5,
            ..green_checks()
        }
        .normalize();
        assert_eq!(summary.state, CheckState::Unknown);
        assert!(!summary.complete);
        assert_eq!(summary.counts, CheckCounts::default());
    }

    #[test]
    fn split_running_counts_that_do_not_sum_collapse_to_unknown() {
        let summary = CheckEvidence {
            state: "pending".into(),
            complete: true,
            total: 4,
            passed: 2,
            failed: 0,
            running: 2,
            queued: 1,
            in_progress: 0,
            unknown: 0,
        }
        .normalize();
        assert_eq!(summary.state, CheckState::Unknown);
        assert!(!summary.complete);
        assert_eq!(summary.counts.running, 0);

        let coherent = CheckEvidence {
            state: "pending".into(),
            complete: true,
            total: 4,
            passed: 2,
            failed: 0,
            running: 2,
            queued: 1,
            in_progress: 1,
            unknown: 0,
        }
        .normalize();
        assert_eq!(coherent.state, CheckState::Pending);
        assert!(coherent.complete);
        assert_eq!(coherent.counts.running, 2);
    }

    #[test]
    fn negative_counts_collapse_to_unknown() {
        let summary = CheckEvidence {
            total: 3,
            passed: 5,
            failed: -2,
            ..green_checks()
        }
        .normalize();
        assert_eq!(summary.state, CheckState::Unknown);
        assert!(!summary.complete);
        assert_eq!(summary.counts.failed, 0);
    }

    #[test]
    fn unrecognized_and_declared_unknown_states_collapse() {
        for state in ["", "SUCCESS", "green", "passed", "ok", "unknown"] {
            let summary = CheckEvidence {
                state: state.into(),
                ..green_checks()
            }
            .normalize();
            assert_eq!(summary.state, CheckState::Unknown, "state {state}");
            assert!(!summary.complete, "state {state}");
        }
    }

    #[test]
    fn a_partial_fetch_is_never_complete_even_when_the_counts_close() {
        let summary = CheckEvidence {
            complete: false,
            ..green_checks()
        }
        .normalize();
        assert_eq!(summary.state, CheckState::Unknown);
        assert!(!summary.complete);
        assert_eq!(
            summary.counts.passed, 4,
            "coherent counts stay readable even when the fetch was partial"
        );
    }

    #[test]
    fn every_closed_state_survives_coherent_counts() {
        for (state, expected) in [
            ("success", CheckState::Success),
            ("failure", CheckState::Failure),
            ("pending", CheckState::Pending),
            ("none", CheckState::None),
        ] {
            let summary = CheckEvidence {
                state: state.into(),
                ..green_checks()
            }
            .normalize();
            assert_eq!(summary.state, expected);
            assert!(summary.complete);
        }
    }

    #[test]
    fn incomplete_threads_block_an_otherwise_green_pull_request() {
        let assessment = assess(
            PullEvidence {
                threads: ThreadEvidence {
                    complete: false,
                    threads: vec![ReviewThread { resolved: true }],
                },
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert_eq!(
            assessment.reports(),
            vec![BlockerReport {
                code: BlockerCode::ThreadsIncomplete,
                detail: "Review threads could not be fully checked".into(),
            }]
        );
    }

    #[test]
    fn incomplete_comments_block_an_otherwise_green_pull_request() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: false,
                    comments: vec![comment(&summary_body(5, HEAD))],
                },
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert_eq!(
            assessment.blockers,
            vec![Blocker::CommentsIncomplete],
            "a review found in a partial list is not proof there is nothing else"
        );
    }

    #[test]
    fn a_review_of_an_older_commit_blocks_and_names_both_commits() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![comment(&summary_body(5, OLDER))],
                },
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert!(!assessment.review_current);
        assert_eq!(
            assessment.reports()[0].code,
            BlockerCode::ReviewedCommitStale
        );
        assert_eq!(
            assessment.reports()[0].detail,
            "greptile-apps reviewed bbbbbbb; head is aaaaaaa"
        );
    }

    #[test]
    fn an_unverified_head_commit_blocks_even_with_a_full_score() {
        let assessment = assess(
            PullEvidence {
                head: CommitSha::parse("not-a-commit"),
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert!(assessment.blockers.contains(&Blocker::HeadCommitUnverified));
        assert!(!assessment.review_current);
    }

    #[test]
    fn a_short_score_blocks_and_reports_the_requirement() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![comment(&summary_body(4, HEAD))],
                },
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert_eq!(
            assessment.reports()[0].detail,
            "greptile-apps confidence 4/5; 5/5 required"
        );
    }

    #[test]
    fn a_score_on_another_scale_never_satisfies_the_requirement() {
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![comment(
                        "Confidence Score: 10/10\nLast reviewed commit: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    )],
                },
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert_eq!(
            assessment.reports()[0].code,
            BlockerCode::ConfidenceBelowRequirement
        );
    }

    #[test]
    fn unresolved_threads_and_missing_reviews_are_named_separately() {
        let assessment = assess(
            PullEvidence {
                threads: ThreadEvidence {
                    complete: true,
                    threads: vec![
                        ReviewThread { resolved: false },
                        ReviewThread { resolved: false },
                        ReviewThread { resolved: true },
                    ],
                },
                comments: CommentEvidence {
                    complete: true,
                    comments: Vec::new(),
                },
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert_eq!(assessment.unresolved, 2);
        assert_eq!(
            assessment
                .reports()
                .into_iter()
                .map(|report| report.detail)
                .collect::<Vec<_>>(),
            vec![
                "2 unresolved review threads".to_string(),
                "greptile-apps summary missing".to_string(),
            ]
        );
    }

    #[test]
    fn a_review_without_a_link_blocks() {
        let mut unlinked = comment(&summary_body(5, HEAD));
        unlinked.url = "  ".into();
        let assessment = assess(
            PullEvidence {
                comments: CommentEvidence {
                    complete: true,
                    comments: vec![unlinked],
                },
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert_eq!(assessment.reports()[0].code, BlockerCode::ReviewLinkMissing);
    }

    #[test]
    fn a_draft_is_never_ready() {
        let assessment = assess(
            PullEvidence {
                draft: true,
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert_eq!(assessment.blockers, vec![Blocker::Draft]);
    }

    #[test]
    fn every_failing_check_state_is_named() {
        for (state, code) in [
            ("failure", BlockerCode::ChecksFailed),
            ("pending", BlockerCode::ChecksPending),
            ("none", BlockerCode::ChecksAbsent),
            ("unknown", BlockerCode::ChecksIncomplete),
            ("nonsense", BlockerCode::ChecksIncomplete),
        ] {
            let assessment = assess(
                PullEvidence {
                    checks: CheckEvidence {
                        state: state.into(),
                        ..green_checks()
                    },
                    ..green_pull()
                },
                &Greptile,
            );
            assert!(!assessment.ready, "state {state}");
            assert_eq!(assessment.reports()[0].code, code, "state {state}");
        }
    }

    #[test]
    fn a_reported_success_over_counts_that_do_not_add_up_is_not_ready() {
        let assessment = assess(
            PullEvidence {
                checks: CheckEvidence {
                    state: "success".into(),
                    complete: true,
                    total: 9,
                    passed: 4,
                    ..green_checks()
                },
                ..green_pull()
            },
            &Greptile,
        );
        assert!(!assessment.ready);
        assert_eq!(assessment.checks.state, CheckState::Unknown);
        assert_eq!(assessment.blockers, vec![Blocker::ChecksIncomplete]);
    }

    #[test]
    fn a_fully_green_pull_request_is_ready_with_no_blockers() {
        let assessment = assess(green_pull(), &Greptile);
        assert!(assessment.ready);
        assert!(assessment.blockers.is_empty());
        assert!(assessment.review_current);
        assert_eq!(assessment.checks.state, CheckState::Success);
        assert_eq!(assessment.checks.counts.passed, 4);
        assert_eq!(
            assessment.review.as_ref().unwrap().confidence,
            Some(Confidence { score: 5, scale: 5 })
        );
        assert_eq!(
            assessment.review.unwrap().reviewed_commit,
            CommitSha::parse(HEAD)
        );
    }

    #[test]
    fn readiness_is_exactly_the_absence_of_blockers_across_every_combination() {
        let states = ["success", "failure", "pending", "none", "unknown", "bogus"];
        let bodies = [
            summary_body(5, HEAD),
            summary_body(4, HEAD),
            summary_body(5, OLDER),
            "no summary here".to_string(),
            "Confidence Score: unreadable\nLast reviewed commit: none".to_string(),
        ];
        let mut ready_cases = 0;
        for draft in [false, true] {
            for head in [CommitSha::parse(HEAD), None] {
                for threads_complete in [false, true] {
                    for comments_complete in [false, true] {
                        for resolved in [false, true] {
                            for body in &bodies {
                                for state in states {
                                    for total in [4, 5] {
                                        let assessment = assess(
                                            PullEvidence {
                                                draft,
                                                head: head.clone(),
                                                threads: ThreadEvidence {
                                                    complete: threads_complete,
                                                    threads: vec![ReviewThread { resolved }],
                                                },
                                                comments: CommentEvidence {
                                                    complete: comments_complete,
                                                    comments: vec![comment(body)],
                                                },
                                                checks: CheckEvidence {
                                                    state: state.into(),
                                                    total,
                                                    ..green_checks()
                                                },
                                                ..green_pull()
                                            },
                                            &Greptile,
                                        );
                                        assert_eq!(
                                            assessment.ready,
                                            assessment.blockers.is_empty(),
                                            "ready and blockers disagree for draft={draft} head={head:?} threads={threads_complete} comments={comments_complete} resolved={resolved} state={state} total={total} body={body:?}"
                                        );
                                        ready_cases += usize::from(assessment.ready);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(
            ready_cases, 1,
            "exactly one combination should describe a ready pull request"
        );
    }

    #[test]
    fn only_the_latest_recognized_comment_from_the_reviewer_is_used() {
        let comments = vec![
            ReviewComment {
                id: 1,
                author: GREPTILE_REVIEWER.into(),
                body: summary_body(5, OLDER),
                url: "https://example.test/1".into(),
                created_at: "2026-09-01T00:00:00Z".into(),
            },
            ReviewComment {
                id: 2,
                author: "someone-else".into(),
                body: summary_body(5, HEAD),
                url: "https://example.test/2".into(),
                created_at: "2026-09-09T00:00:00Z".into(),
            },
            ReviewComment {
                id: 3,
                author: GREPTILE_REVIEWER.into(),
                body: "Looks good to me".into(),
                url: "https://example.test/3".into(),
                created_at: "2026-09-08T00:00:00Z".into(),
            },
            ReviewComment {
                id: 4,
                author: GREPTILE_REVIEWER.into(),
                body: summary_body(3, HEAD),
                url: "https://example.test/4".into(),
                created_at: "2026-09-07T00:00:00Z".into(),
            },
        ];
        let summary = Greptile.summarize(&comments).unwrap();
        assert_eq!(summary.comment_id, 4);
        assert_eq!(summary.confidence, Some(Confidence { score: 3, scale: 5 }));
        assert_eq!(summary.reviewed_commit, CommitSha::parse(HEAD));
        assert_eq!(summary.reviewer, GREPTILE_REVIEWER);
    }

    #[test]
    fn an_unparsable_timestamp_never_outranks_a_readable_one() {
        let comments = vec![
            ReviewComment {
                id: 1,
                author: GREPTILE_REVIEWER.into(),
                body: summary_body(5, HEAD),
                url: "https://example.test/1".into(),
                created_at: "2026-09-01T00:00:00Z".into(),
            },
            ReviewComment {
                id: 2,
                author: GREPTILE_REVIEWER.into(),
                body: summary_body(1, OLDER),
                url: "https://example.test/2".into(),
                created_at: "yesterday".into(),
            },
        ];
        assert_eq!(Greptile.summarize(&comments).unwrap().comment_id, 1);
    }

    #[test]
    fn greptile_parses_scores_and_commits_through_markup() {
        let body = "<p><strong>Confidence&nbsp;Score:</strong> 5&nbsp;/&nbsp;5</p>\n\
                    <p>_Last reviewed commit: `aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa`_</p>";
        assert_eq!(
            Greptile.confidence(body),
            Some(Confidence { score: 5, scale: 5 })
        );
        assert_eq!(Greptile.reviewed_commit(body), CommitSha::parse(HEAD));
        assert!(Greptile.identifies(body));
    }

    #[test]
    fn greptile_rejects_scores_and_commits_it_cannot_read() {
        assert!(
            Greptile
                .confidence("Confidence Score: five out of five")
                .is_none()
        );
        assert!(Greptile.confidence("Confidence Score: 6/5").is_none());
        assert!(Greptile.confidence("Confidence Score: 3/0").is_none());
        assert!(
            Greptile
                .reviewed_commit("Last reviewed commit: unknown")
                .is_none()
        );
        assert!(
            Greptile
                .reviewed_commit(&format!("Last reviewed commit: {HEAD}a"))
                .is_none(),
            "a longer hex run is not a commit name"
        );
        assert!(
            Greptile
                .reviewed_commit(&format!("Merged {HEAD}\nLast reviewed commit: unknown"))
                .is_none(),
            "a commit outside the reviewed line must not be adopted"
        );
        assert!(!Greptile.identifies("Nice work!"));
    }

    #[test]
    fn commit_names_are_normalized_and_validated() {
        let upper = CommitSha::parse(&HEAD.to_ascii_uppercase()).unwrap();
        assert_eq!(upper, CommitSha::parse(HEAD).unwrap());
        assert_eq!(upper.short(), "aaaaaaa");
        assert_eq!(upper.as_str(), HEAD);
        for value in ["", "aaaaaaa", &format!("{HEAD}a"), &HEAD.replace('a', "z")] {
            assert!(CommitSha::parse(value).is_none(), "{value}");
        }
    }
}
