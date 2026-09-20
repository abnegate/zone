//! What automation tells people, rendered once for every channel.
//!
//! A merge is the one event a person has to be able to act on from the notice
//! alone, so it says what the change did for the project and then what it did
//! to the code: two levels, both short, and the link to the pull request.
//! Everything here is workspace data -- titles, paths, reviewer names -- and
//! [`Notification::new`] sanitizes the title and body it is handed.

use std::fmt::Write as _;

use zone_notify::{Notification, Severity};

/// Everything the merge notice draws on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    pub project: String,
    pub task_title: String,
    pub pr_title: String,
    pub pr_url: String,
    /// What the change accomplishes for the project, in two or three sentences.
    pub high_level: String,
    pub files_changed: u32,
    pub additions: u32,
    pub deletions: u32,
    pub commits: u32,
    pub top_paths: Vec<String>,
    pub review_rounds: u32,
    pub reviewers: Vec<String>,
    pub same_model: bool,
    pub bots_absent: Vec<String>,
    pub findings_raised: usize,
    pub findings_addressed: usize,
    pub checks: String,
    pub admin_merge: bool,
    pub merge_sha: String,
    pub post_merge: Option<String>,
}

/// Paths named in the low-level summary before the rest are counted.
const TOP_PATHS: usize = 8;

pub fn merged(report: &MergeReport) -> Notification {
    let mut body = String::new();
    let _ = writeln!(body, "## High level");
    let _ = writeln!(body);
    let high = report.high_level.trim();
    let _ = writeln!(
        body,
        "{}",
        if high.is_empty() {
            "The change described by the pull request was merged."
        } else {
            high
        }
    );
    let _ = writeln!(body);
    let _ = writeln!(body, "## Low level");
    let _ = writeln!(body);
    let _ = writeln!(
        body,
        "- Files changed: {} (+{} / -{}) across {} commit{}",
        report.files_changed,
        report.additions,
        report.deletions,
        report.commits,
        if report.commits == 1 { "" } else { "s" }
    );
    if !report.top_paths.is_empty() {
        let shown: Vec<&str> = report
            .top_paths
            .iter()
            .take(TOP_PATHS)
            .map(String::as_str)
            .collect();
        let rest = report.top_paths.len().saturating_sub(shown.len());
        let _ = write!(body, "- Paths: {}", shown.join(", "));
        if rest > 0 {
            let _ = write!(body, " and {rest} more");
        }
        let _ = writeln!(body);
    }
    let mut reviewers = report.reviewers.join(", ");
    if reviewers.is_empty() {
        reviewers.push_str("none recorded");
    }
    if report.same_model {
        reviewers.push_str(" (same model as the author)");
    }
    let _ = writeln!(
        body,
        "- Review: {} round{}; reviewed by {reviewers}",
        report.review_rounds,
        if report.review_rounds == 1 { "" } else { "s" }
    );
    if !report.bots_absent.is_empty() {
        let _ = writeln!(
            body,
            "- Review bots that did not answer: {}",
            report.bots_absent.join(", ")
        );
    }
    let _ = writeln!(
        body,
        "- Findings: {} raised, {} addressed",
        report.findings_raised, report.findings_addressed
    );
    let _ = writeln!(body, "- Checks: {}", report.checks);
    let _ = writeln!(
        body,
        "- Merge: squash{} as {}",
        if report.admin_merge {
            " with administrator privileges"
        } else {
            ""
        },
        short(&report.merge_sha)
    );
    if let Some(post_merge) = &report.post_merge {
        let _ = writeln!(body, "- After merge: {post_merge}");
    }
    Notification::new(
        format!("Merged: {}", report.pr_title.trim()),
        body.trim_end(),
    )
    .severity(Severity::Success)
    .link(report.pr_url.clone())
    .field("Project", report.project.clone())
    .field("Task", report.task_title.clone())
}

pub fn paused(project: &str, reason: &str, link: Option<&str>) -> Notification {
    let notification = Notification::new(
        format!("Paused: {}", project.trim()),
        format!(
            "Automation on this project stopped and needs a person.\n\n{}\n\nResume it from the \
             project's page once the cause is dealt with.",
            reason.trim()
        ),
    )
    .severity(Severity::Warning)
    .field("Project", project.to_string());
    match link {
        Some(link) => notification.link(link.to_string()),
        None => notification,
    }
}

pub fn completed(
    project: &str,
    merged: usize,
    manual_remaining: usize,
    post_merge: Option<&str>,
) -> Notification {
    let mut body = format!(
        "Every agentic task of this project is complete: {merged} pull request{} merged.",
        if merged == 1 { "" } else { "s" }
    );
    if manual_remaining > 0 {
        let _ = write!(
            body,
            " {manual_remaining} manual task{} remain{} for a person.",
            if manual_remaining == 1 { "" } else { "s" },
            if manual_remaining == 1 { "s" } else { "" }
        );
    }
    if let Some(post_merge) = post_merge {
        let _ = write!(body, "\n\nLast post-merge job: {post_merge}");
    }
    Notification::new(format!("Complete: {}", project.trim()), body)
        .severity(Severity::Success)
        .field("Project", project.to_string())
}

pub fn task_added(project: &str, kind: &str, title: &str, why: &str) -> Notification {
    Notification::new(
        format!("Added a {kind} task to {}", project.trim()),
        format!("{}\n\n{}", title.trim(), why.trim()),
    )
    .severity(Severity::Info)
    .field("Project", project.to_string())
}

fn short(sha: &str) -> &str {
    let trimmed = sha.trim();
    if trimmed.len() >= 12 {
        &trimmed[..12]
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> MergeReport {
        MergeReport {
            project: "Shop".into(),
            task_title: "Add the cart".into(),
            pr_title: "(feat): add a cart to the storefront".into(),
            pr_url: "https://github.com/acme/shop/pull/4".into(),
            high_level: "Shoppers can now add items to a cart and check out.".into(),
            files_changed: 6,
            additions: 240,
            deletions: 12,
            commits: 3,
            top_paths: vec!["src/cart.ts".into(), "src/cart.test.ts".into()],
            review_rounds: 2,
            reviewers: vec!["qwen3:32b (Zone)".into(), "CodeRabbit".into()],
            same_model: false,
            bots_absent: vec![],
            findings_raised: 3,
            findings_addressed: 3,
            checks: "passed".into(),
            admin_merge: true,
            merge_sha: "0123456789abcdef0123456789abcdef01234567".into(),
            post_merge: Some("deploy succeeded".into()),
        }
    }

    #[test]
    fn a_merge_notice_carries_both_levels_and_the_link() {
        let notice = merged(&report());
        assert_eq!(
            notice.title(),
            "Merged: (feat): add a cart to the storefront"
        );
        assert!(notice.body().contains("## High level"));
        assert!(notice.body().contains("Shoppers can now add items"));
        assert!(notice.body().contains("## Low level"));
        assert!(
            notice
                .body()
                .contains("Files changed: 6 (+240 / -12) across 3 commits")
        );
        assert!(
            notice
                .body()
                .contains("reviewed by qwen3:32b (Zone), CodeRabbit")
        );
        assert!(
            notice
                .body()
                .contains("with administrator privileges as 0123456789ab")
        );
        assert!(notice.body().contains("After merge: deploy succeeded"));
        assert_eq!(notice.url(), Some("https://github.com/acme/shop/pull/4"));
        assert_eq!(notice.kind(), Severity::Success);
    }

    #[test]
    fn a_same_model_review_is_labelled_and_a_silent_bot_is_named() {
        let mut same = report();
        same.same_model = true;
        same.bots_absent = vec!["greptile".into()];
        same.admin_merge = false;
        let body = merged(&same).body().to_string();
        assert!(body.contains("(same model as the author)"), "{body}");
        assert!(body.contains("did not answer: greptile"), "{body}");
        assert!(body.contains("- Merge: squash as"), "{body}");
    }

    #[test]
    fn pause_and_completion_notices_say_what_happened() {
        let paused = paused(
            "Shop",
            "task Add the cart failed 3 times: tests do not pass",
            None,
        );
        assert_eq!(paused.kind(), Severity::Warning);
        assert!(paused.body().contains("failed 3 times"));
        let done = completed("Shop", 7, 1, Some("deploy succeeded"));
        assert!(done.body().contains("7 pull requests merged"));
        assert!(done.body().contains("1 manual task remains"));
        assert!(done.body().contains("deploy succeeded"));
    }
}
