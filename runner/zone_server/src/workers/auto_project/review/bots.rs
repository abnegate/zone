//! What the review bots already on a repository said about a change.
//!
//! CodeRabbit and Greptile review a pull request on their own schedule and
//! leave a summary comment naming the head they read; `agent::readiness`
//! already knows how to read those. Here a bot's summary on the current head
//! becomes a round of review like any other -- a review by a model that did
//! not write the change -- and its unresolved threads become findings a
//! fix-up run has to answer.

use crate::agent::readiness::{
    CommitSha, Confidence, ReviewComment, ReviewSignal, SignalKind, published_at,
};
use crate::config::AutoProjectConfig;
use crate::db::auto_projects::{Finding, ReviewRow, Verdict};
use zone_vcs::pull_request::{IssueComment, ReviewThreadRecord, SubmittedReviewRecord};

/// Characters of a thread's opening comment kept as a finding's title.
const TITLE_CHARS: usize = 140;
/// Characters of a thread's opening comment kept as a finding's detail.
const DETAIL_CHARS: usize = 800;
/// Characters of a bot's summary comment kept as the round's summary.
const SUMMARY_CHARS: usize = 600;

/// The bots this deployment reads: the configured ones, or every bot the
/// build knows. An unrecognised name is logged and skipped rather than
/// silently waited for.
pub fn recognised(config: &AutoProjectConfig) -> Vec<SignalKind> {
    if config.review_bots.is_empty() {
        return SignalKind::ALL.to_vec();
    }
    // Aliases (`coderabbit`, `coderabbitai`) parse to one kind; naming both
    // must not make the pipeline wait for, trigger or record the bot twice.
    let mut kinds: Vec<SignalKind> = Vec::new();
    for name in &config.review_bots {
        match SignalKind::parse(name) {
            Some(kind) if !kinds.contains(&kind) => kinds.push(kind),
            Some(_) => {}
            None => tracing::warn!(
                name,
                "ZONE_AUTO_REVIEW_BOTS names a review bot this build does not know"
            ),
        }
    }
    kinds
}

/// Whether a comment's author is this bot.
fn authored(kind: SignalKind, author: &str) -> bool {
    kind.as_signal().authored(author)
}

/// The bots a pull request should be waited for.
///
/// A bot the operator named is always expected. Otherwise a bot is expected
/// once it has shown itself: a row from an earlier round, a summary on the
/// conversation, or a thread on the diff.
pub fn expected(
    config: &AutoProjectConfig,
    prior: &[ReviewRow],
    comments: &[IssueComment],
    threads: &[ReviewThreadRecord],
) -> Vec<SignalKind> {
    let explicit = !config.review_bots.is_empty();
    recognised(config)
        .into_iter()
        .filter(|kind| {
            explicit
                || prior
                    .iter()
                    .any(|row| row.is_bot() && row.reviewer == kind.reviewer())
                || comments
                    .iter()
                    .any(|comment| authored(*kind, &comment.author))
                || threads.iter().any(|thread| {
                    thread
                        .comments
                        .iter()
                        .any(|comment| authored(*kind, &comment.author))
                })
        })
        .collect()
}

/// One bot's review of one head, ready to record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotRound {
    pub reviewer: &'static str,
    pub verdict: Verdict,
    pub summary: String,
    pub findings: Vec<Finding>,
    pub external_id: Option<String>,
}

/// A bot's score on one head and where it was read from.
struct Score {
    confidence: Confidence,
    text: String,
    external_id: String,
}

/// The bot's round on `head`, if it has reviewed that head.
///
/// The score comes from the bot's summary comment when that comment names the
/// head and carries one. CodeRabbit on a paid plan scores only in its review
/// of the diff, so its latest scored review submitted on the head stands in.
/// That head is the commit the review was submitted on, never a walkthrough's
/// range: a rate-limited CodeRabbit still posts a walkthrough whose range
/// names the new head. A head with neither is one the bot has not answered.
pub fn round(
    kind: SignalKind,
    comments: &[IssueComment],
    reviews: &[SubmittedReviewRecord],
    threads: &[ReviewThreadRecord],
    head: &str,
) -> Option<BotRound> {
    let signal = kind.as_signal();
    let head = CommitSha::parse(head)?;
    let score = summarized(signal.as_ref(), comments, &head)
        .or_else(|| reviewed(kind, signal.as_ref(), reviews, &head))?;
    let findings: Vec<Finding> = threads
        .iter()
        .filter(|thread| !thread.resolved && !thread.outdated)
        .filter(|thread| {
            thread
                .comments
                .first()
                .is_some_and(|comment| signal.authored(&comment.author))
        })
        .map(|thread| {
            let body = thread
                .comments
                .first()
                .map(|comment| comment.body.as_str())
                .unwrap_or_default();
            Finding {
                id: thread.id.clone(),
                severity: "major".to_string(),
                file: thread.path.clone(),
                line: thread.line,
                title: first_line(body, TITLE_CHARS),
                detail: excerpt(body, DETAIL_CHARS),
                thread_id: Some(thread.id.clone()),
                reviewer: Some(kind.as_str().to_string()),
            }
        })
        .collect();
    let verdict = if score.confidence.satisfies(signal.required()) && findings.is_empty() {
        Verdict::Approve
    } else {
        Verdict::RequestChanges
    };
    Some(BotRound {
        reviewer: kind.reviewer(),
        verdict,
        summary: score.text,
        findings,
        external_id: Some(score.external_id),
    })
}

/// The score in the bot's latest summary comment, when it names `head`.
fn summarized(
    signal: &dyn ReviewSignal,
    comments: &[IssueComment],
    head: &CommitSha,
) -> Option<Score> {
    let evidence: Vec<ReviewComment> = comments
        .iter()
        .map(|comment| ReviewComment {
            id: comment.id,
            author: comment.author.clone(),
            body: comment.body.clone(),
            url: comment.url.clone(),
            created_at: comment.created_at.clone(),
        })
        .collect();
    let summary = signal.summarize(&evidence)?;
    if summary.reviewed_commit.as_ref() != Some(head) {
        return None;
    }
    let confidence = summary.confidence?;
    let text = comments
        .iter()
        .find(|comment| comment.id == summary.comment_id)
        .map(|comment| excerpt(&comment.body, SUMMARY_CHARS))
        .unwrap_or_default();
    Some(Score {
        confidence,
        text,
        external_id: summary.comment_id.to_string(),
    })
}

/// The score in the bot's latest review submitted on `head` that carries one.
fn reviewed(
    kind: SignalKind,
    signal: &dyn ReviewSignal,
    reviews: &[SubmittedReviewRecord],
    head: &CommitSha,
) -> Option<Score> {
    if !kind.scores_in_reviews() {
        return None;
    }
    reviews
        .iter()
        .filter(|review| {
            signal.authored(&review.author)
                && CommitSha::parse(&review.commit_id).as_ref() == Some(head)
        })
        .filter_map(|review| {
            signal
                .confidence(&review.body)
                .map(|confidence| (review, confidence))
        })
        .max_by_key(|(review, _)| (published_at(&review.submitted_at), review.id))
        .map(|(review, confidence)| Score {
            confidence,
            text: excerpt(&review.body, SUMMARY_CHARS),
            external_id: review.id.to_string(),
        })
}

/// The first line of a comment, cut to a limit.
fn first_line(body: &str, limit: usize) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("<!--"))
        .unwrap_or("Review comment");
    excerpt(line, limit)
}

/// Text cut to a limit, with the cut marked.
fn excerpt(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    let mut cut: String = trimmed.chars().take(limit).collect();
    cut.push('…');
    cut
}

/// The display name a notice uses for a bot.
pub fn display_name(reviewer: &str) -> String {
    match SignalKind::parse(reviewer) {
        Some(SignalKind::CodeRabbit) => "CodeRabbit".to_string(),
        Some(SignalKind::Greptile) => "Greptile".to_string(),
        None => reviewer.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zone_vcs::pull_request::ThreadComment;

    const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn aliases_of_one_bot_are_recognised_once() {
        let config = AutoProjectConfig {
            review_bots: vec![
                "coderabbit".to_string(),
                "coderabbitai".to_string(),
                "unknown-bot".to_string(),
            ],
            ..Default::default()
        };
        assert_eq!(recognised(&config), vec![SignalKind::CodeRabbit]);
        assert_eq!(
            recognised(&AutoProjectConfig::default()),
            SignalKind::ALL.to_vec(),
            "no configuration means every bot the build knows"
        );
    }

    fn comment(author: &str, body: &str) -> IssueComment {
        IssueComment {
            id: 41,
            author: author.into(),
            body: body.into(),
            url: "https://github.com/acme/shop/pull/4#issuecomment-41".into(),
            created_at: "2026-09-20T10:00:00Z".into(),
        }
    }

    fn thread(author: &str, resolved: bool) -> ReviewThreadRecord {
        ReviewThreadRecord {
            id: "PRRT_9".into(),
            resolved,
            outdated: false,
            path: Some("src/cart.ts".into()),
            line: Some(12),
            comments: vec![ThreadComment {
                database_id: Some(99),
                author: author.into(),
                body: "**Handle the empty cart.**\n\ncheckout() calls items[0] without a guard."
                    .into(),
                url: String::new(),
                created_at: "2026-09-20T10:01:00Z".into(),
            }],
        }
    }

    #[test]
    fn a_coderabbit_walkthrough_on_the_head_with_an_open_thread_requests_changes() {
        let comments = vec![comment(
            "coderabbitai[bot]",
            &format!(
                "## Walkthrough\n\nActionable comments posted: 1\n\nReviewed between 1111111111111111111111111111111111111111 and {HEAD}"
            ),
        )];
        let threads = vec![thread("coderabbitai", false)];
        let round = round(SignalKind::CodeRabbit, &comments, &[], &threads, HEAD).unwrap();
        assert_eq!(round.reviewer, "coderabbitai");
        assert_eq!(round.verdict, Verdict::RequestChanges);
        assert_eq!(round.findings.len(), 1);
        assert_eq!(round.findings[0].thread_id.as_deref(), Some("PRRT_9"));
        assert_eq!(round.findings[0].title, "**Handle the empty cart.**");
        assert_eq!(round.findings[0].reviewer.as_deref(), Some("coderabbit"));
        assert_eq!(round.external_id.as_deref(), Some("41"));
    }

    #[test]
    fn a_summary_for_another_head_is_not_a_round_and_a_clean_one_approves() {
        let stale = vec![comment(
            "coderabbitai",
            "Actionable comments posted: 0\n\nReviewed between 1111111111111111111111111111111111111111 and 2222222222222222222222222222222222222222",
        )];
        assert!(round(SignalKind::CodeRabbit, &stale, &[], &[], HEAD).is_none());
        let clean = vec![comment(
            "coderabbitai",
            &format!(
                "Actionable comments posted: 0\n\nReviewed between 1111111111111111111111111111111111111111 and {HEAD}"
            ),
        )];
        let round = round(
            SignalKind::CodeRabbit,
            &clean,
            &[],
            &[thread("coderabbitai", true)],
            HEAD,
        )
        .unwrap();
        assert_eq!(round.verdict, Verdict::Approve);
        assert!(
            round.findings.is_empty(),
            "a resolved thread is not a finding"
        );
    }

    #[test]
    fn a_coderabbit_summary_that_hit_its_review_limit_is_not_a_round() {
        let head = "76ddcd8687b7a54576043a87b3c03ec48a7e9e9a";
        let comments = vec![comment(
            "coderabbitai[bot]",
            include_str!("fixtures/coderabbit-review-limit-reached.md"),
        )];
        assert_eq!(
            round(SignalKind::CodeRabbit, &comments, &[], &[], head),
            None,
            "a summary that names the head but carries no review is not a review of the head"
        );
    }

    const PAID_HEAD: &str = "39edddf0d1abc06b84ebb74e8213c1fd41ec333e";
    const PAID_REVIEW: &str = include_str!("fixtures/coderabbit-paid-plan-review.md");

    fn unscored_walkthrough() -> IssueComment {
        comment(
            "coderabbitai[bot]",
            &format!(
                "## Walkthrough\n\nThe settings tests share one formatter.\n\nReviewed between 86a82b0f230c95b2b6c0429386bcfda031abd47b and {PAID_HEAD}"
            ),
        )
    }

    fn review(id: u64, body: &str, commit_id: &str, submitted_at: &str) -> SubmittedReviewRecord {
        SubmittedReviewRecord {
            id,
            author: "coderabbitai[bot]".into(),
            body: body.into(),
            commit_id: commit_id.into(),
            submitted_at: submitted_at.into(),
        }
    }

    fn paid_reviews(body: &str, commit_id: &str) -> Vec<SubmittedReviewRecord> {
        vec![
            review(5333502514, body, commit_id, "2026-09-28T02:51:09Z"),
            review(5333523878, "", commit_id, "2026-09-28T02:55:47Z"),
        ]
    }

    #[test]
    fn a_paid_coderabbit_review_of_the_head_with_an_open_thread_requests_changes() {
        let round = round(
            SignalKind::CodeRabbit,
            &[unscored_walkthrough()],
            &paid_reviews(PAID_REVIEW, PAID_HEAD),
            &[thread("coderabbitai", false)],
            PAID_HEAD,
        )
        .expect("the review on the head carries the score the summary lacks");
        assert_eq!(round.verdict, Verdict::RequestChanges);
        assert_eq!(round.findings.len(), 1);
        assert_eq!(round.findings[0].thread_id.as_deref(), Some("PRRT_9"));
        assert_eq!(
            round.external_id.as_deref(),
            Some("5333502514"),
            "the scored review is the round's source, not the later empty one"
        );
        assert!(round.summary.contains("Actionable comments posted: 1"));
    }

    #[test]
    fn a_paid_coderabbit_review_of_the_head_with_nothing_actionable_approves() {
        let clean = PAID_REVIEW.replace(
            "Actionable comments posted: 1",
            "Actionable comments posted: 0",
        );
        let round = round(
            SignalKind::CodeRabbit,
            &[],
            &paid_reviews(&clean, PAID_HEAD),
            &[thread("coderabbitai", true)],
            PAID_HEAD,
        )
        .expect("a review on the head is a round even without a summary comment");
        assert_eq!(round.verdict, Verdict::Approve);
        assert!(round.findings.is_empty());
    }

    #[test]
    fn a_paid_coderabbit_review_of_another_commit_is_not_a_round_of_the_head() {
        let head = "76ddcd8687b7a54576043a87b3c03ec48a7e9e9a";
        let comments = vec![comment(
            "coderabbitai[bot]",
            include_str!("fixtures/coderabbit-review-limit-reached.md"),
        )];
        assert_eq!(
            round(
                SignalKind::CodeRabbit,
                &comments,
                &paid_reviews(PAID_REVIEW, PAID_HEAD),
                &[thread("coderabbitai", false)],
                head,
            ),
            None,
            "a walkthrough whose range names the head does not make an older review one of the head"
        );
    }

    #[test]
    fn a_greptile_score_is_never_read_from_a_pull_request_review() {
        let reviews = vec![SubmittedReviewRecord {
            author: "greptile-apps[bot]".into(),
            ..review(7, "Confidence Score: 5/5", HEAD, "2026-09-28T02:51:09Z")
        }];
        assert_eq!(round(SignalKind::Greptile, &[], &reviews, &[], HEAD), None);
    }

    #[test]
    fn bots_are_expected_once_seen_or_when_named() {
        let mut config = AutoProjectConfig::default();
        assert!(
            expected(&config, &[], &[], &[]).is_empty(),
            "nothing seen, nothing expected"
        );
        let seen = vec![comment("greptile-apps[bot]", "Confidence Score: 4/5")];
        assert_eq!(
            expected(&config, &[], &seen, &[]),
            vec![SignalKind::Greptile]
        );
        config.review_bots = vec!["coderabbit".into(), "nobody".into()];
        assert_eq!(
            expected(&config, &[], &[], &[]),
            vec![SignalKind::CodeRabbit]
        );
        assert_eq!(display_name("greptile-apps"), "Greptile");
    }
}
