//! What the review bots already on a repository said about a change.
//!
//! CodeRabbit and Greptile review a pull request on their own schedule and
//! leave a summary comment naming the head they read; `agent::readiness`
//! already knows how to read those. Here a bot's summary on the current head
//! becomes a round of review like any other -- a review by a model that did
//! not write the change -- and its unresolved threads become findings a
//! fix-up run has to answer.

use crate::agent::readiness::{CommitSha, ReviewComment, SignalKind};
use crate::config::AutoProjectConfig;
use crate::db::auto_projects::{Finding, ReviewRow, Verdict};
use zone_vcs::pull_request::{IssueComment, ReviewThreadRecord};

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
    config
        .review_bots
        .iter()
        .filter_map(|name| {
            let kind = SignalKind::parse(name);
            if kind.is_none() {
                tracing::warn!(
                    name,
                    "ZONE_AUTO_REVIEW_BOTS names a review bot this build does not know"
                );
            }
            kind
        })
        .collect()
}

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

/// The bot's round on `head`, if it has published one for that head.
pub fn round(
    kind: SignalKind,
    comments: &[IssueComment],
    threads: &[ReviewThreadRecord],
    head: &str,
) -> Option<BotRound> {
    let signal = kind.as_signal();
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
    let head = CommitSha::parse(head)?;
    if summary.reviewed_commit.as_ref() != Some(&head) {
        return None;
    }
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
    let at_bar = summary
        .confidence
        .is_some_and(|confidence| confidence.satisfies(signal.required()));
    let verdict = if at_bar && findings.is_empty() {
        Verdict::Approve
    } else {
        Verdict::RequestChanges
    };
    let text = comments
        .iter()
        .find(|comment| comment.id == summary.comment_id)
        .map(|comment| excerpt(&comment.body, SUMMARY_CHARS))
        .unwrap_or_default();
    Some(BotRound {
        reviewer: kind.reviewer(),
        verdict,
        summary: text,
        findings,
        external_id: Some(summary.comment_id.to_string()),
    })
}

fn first_line(body: &str, limit: usize) -> String {
    let line = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("<!--"))
        .unwrap_or("Review comment");
    excerpt(line, limit)
}

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
        let round = round(SignalKind::CodeRabbit, &comments, &threads, HEAD).unwrap();
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
        assert!(round(SignalKind::CodeRabbit, &stale, &[], HEAD).is_none());
        let clean = vec![comment(
            "coderabbitai",
            &format!(
                "Actionable comments posted: 0\n\nReviewed between 1111111111111111111111111111111111111111 and {HEAD}"
            ),
        )];
        let round = round(
            SignalKind::CodeRabbit,
            &clean,
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
