//! What a run of an auto project is told beyond its task.
//!
//! Three blocks, each bounded: the brief the interview produced, so every task
//! honours the same platforms, stack and design; the roadmap of sibling tasks,
//! so a run knows what came before it and what depends on it; and, for a
//! fix-up run, the findings the last review left open and the check that
//! failed. A task outside an auto project renders nothing here at all.

use std::fmt::Write as _;

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::auto_projects::{self, Finding, ProjectTask};
use crate::db::tasks::{self, TaskRow};

/// Characters the brief block may take.
pub const BRIEF_CHARS: usize = 4_000;
/// Characters the roadmap block may take.
pub const ROADMAP_CHARS: usize = 2_000;
/// Characters the review block may take.
pub const REVIEW_CHARS: usize = 6_000;
/// Roadmap lines before the rest are counted.
const ROADMAP_LINES: usize = 40;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blocks {
    pub brief: String,
    pub roadmap: String,
    pub review: String,
}

impl Blocks {
    pub fn is_empty(&self) -> bool {
        self.brief.is_empty() && self.roadmap.is_empty() && self.review.is_empty()
    }
}

/// The blocks for a task, or none when it belongs to no auto project.
pub async fn blocks(pool: &PgPool, task: &TaskRow) -> Blocks {
    let project_ids = match tasks::get_task_project_ids(pool, task.id).await {
        Ok(ids) => ids,
        Err(error) => {
            tracing::warn!(task_id = %task.id, %error, "Could not read the task's projects");
            return Blocks::default();
        }
    };
    let mut project = None;
    for id in project_ids {
        if let Ok(Some(automation)) = auto_projects::automation(pool, id).await
            && automation.auto
        {
            project = Some(automation);
            break;
        }
    }
    let Some(project) = project else {
        return Blocks::default();
    };
    let siblings = auto_projects::project_tasks(pool, project.project_id)
        .await
        .unwrap_or_default();
    let findings = auto_projects::open_findings(pool, task.id)
        .await
        .unwrap_or_default();
    let reason = auto_projects::task_stage(pool, task.id)
        .await
        .ok()
        .flatten()
        .and_then(|row| row.reason);
    Blocks {
        brief: brief_block(project.brief.as_ref()),
        roadmap: roadmap_block(task.id, &siblings),
        review: review_block(&findings, reason.as_deref()),
    }
}

/// The brief as prose: one line per top-level key, nested values flattened,
/// cut at the budget with the cut named.
pub fn brief_block(brief: Option<&Value>) -> String {
    let Some(Value::Object(fields)) = brief else {
        return String::new();
    };
    let mut body = String::new();
    for (key, value) in fields {
        let rendered = render_value(value, 0);
        if rendered.trim().is_empty() {
            continue;
        }
        let _ = writeln!(body, "- {}: {}", key.replace('_', " "), rendered);
    }
    if body.trim().is_empty() {
        return String::new();
    }
    format!(
        "\n\n# Project brief\n\nThese decisions were made with the person who commissioned the \
         project and apply to every task in it. Follow them; do not revisit them.\n\n{}",
        cut(body.trim_end(), BRIEF_CHARS)
    )
}

/// Nesting past this renders as JSON: a brief is decisions, not a document tree.
const MAX_DEPTH: usize = 4;

fn render_value(value: &Value, depth: usize) -> String {
    if depth > MAX_DEPTH && (value.is_array() || value.is_object()) {
        return value.to_string();
    }
    match value {
        Value::Null => String::new(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.trim().to_string(),
        Value::Array(items) => items
            .iter()
            .map(|item| render_value(item, depth + 1))
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| {
                let rendered = render_value(value, depth + 1);
                if rendered.is_empty() {
                    String::new()
                } else {
                    format!("{} {rendered}", key.replace('_', " "))
                }
            })
            .filter(|item| !item.is_empty())
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// The project's tasks in order, with this one marked.
pub fn roadmap_block(task_id: Uuid, siblings: &[ProjectTask]) -> String {
    if siblings.len() < 2 {
        return String::new();
    }
    let mut body = String::new();
    for (index, sibling) in siblings.iter().enumerate().take(ROADMAP_LINES) {
        let marker = if sibling.task_id == task_id {
            " (this task)"
        } else {
            ""
        };
        let kind = sibling.kind.as_deref().unwrap_or("task");
        let _ = writeln!(
            body,
            "{}. [{}] {} — {}{marker}",
            index + 1,
            sibling.status,
            kind,
            sibling.title.trim()
        );
    }
    if siblings.len() > ROADMAP_LINES {
        let _ = writeln!(body, "… and {} more", siblings.len() - ROADMAP_LINES);
    }
    format!(
        "\n\n# Project roadmap\n\nEvery task of the project, in order. Build on what is complete, \
         stay inside this task, and do not start what a later task owns.\n\n{}",
        cut(body.trim_end(), ROADMAP_CHARS)
    )
}

/// What the last review left open, and the check that failed, for the run
/// that has to put it right.
pub fn review_block(findings: &[Finding], reason: Option<&str>) -> String {
    if findings.is_empty() && reason.is_none_or(|text| text.trim().is_empty()) {
        return String::new();
    }
    let mut body = String::new();
    if let Some(reason) = reason.filter(|text| !text.trim().is_empty()) {
        let _ = writeln!(body, "Why this run was started: {}\n", reason.trim());
    }
    for finding in findings {
        let mut line = format!("- {} [{}]", finding.id, finding.severity);
        if let Some(file) = &finding.file {
            let _ = write!(line, " {file}");
            if let Some(number) = finding.line {
                let _ = write!(line, ":{number}");
            }
        }
        let _ = write!(line, ": {}", finding.title.trim());
        if !finding.detail.trim().is_empty() {
            let _ = write!(line, " — {}", finding.detail.trim());
        }
        if let Some(reviewer) = &finding.reviewer {
            let _ = write!(line, " (raised by {reviewer})");
        }
        let _ = writeln!(body, "{line}");
    }
    format!(
        "\n\n# Review findings to address\n\nA review of this task's pull request left these open. \
         Address every one on the same branch, or say in your report, by id, why one should not \
         be changed; a fix-up run that leaves a finding unmentioned is sent back.\n\n{}",
        cut(body.trim_end(), REVIEW_CHARS)
    )
}

fn cut(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_string();
    }
    let kept: String = text.chars().take(budget).collect();
    format!("{kept}\n[cut at {budget} characters]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sibling(id: Uuid, title: &str, status: &str, kind: &str) -> ProjectTask {
        ProjectTask {
            task_id: id,
            title: title.into(),
            status: status.into(),
            is_agentic: true,
            priority: None,
            pr_url: None,
            dependencies: None,
            kind: Some(kind.into()),
            stage: None,
            reason: None,
            runs: None,
            review_rounds: None,
            head: None,
            checks: None,
            merge_sha: None,
            auto_created: None,
            reviewers: None,
        }
    }

    #[test]
    fn the_brief_renders_nested_decisions_as_lines_and_stays_inside_its_budget() {
        let brief = json!({
            "platforms": ["web", "ios"],
            "theme": {"archetype": "minimal", "primary": "#112233", "animation_style": "subtle"},
            "notes": "",
            "deployment": {"target": "vercel", "trigger": "merge_to_main"}
        });
        let block = brief_block(Some(&brief));
        assert!(block.starts_with("\n\n# Project brief"), "{block}");
        assert!(block.contains("- platforms: web, ios"), "{block}");
        // Keys render in sorted order: the value is a map, not a list.
        assert!(
            block.contains("- theme: animation style subtle; archetype minimal; primary #112233"),
            "{block}"
        );
        assert!(!block.contains("- notes"), "{block}");
        assert_eq!(brief_block(None), "");
        let long = json!({"notes": "x".repeat(BRIEF_CHARS + 50)});
        assert!(brief_block(Some(&long)).contains("[cut at"));
    }

    #[test]
    fn the_roadmap_marks_this_task_and_needs_a_sibling_to_render() {
        let me = Uuid::new_v4();
        let other = Uuid::new_v4();
        assert_eq!(
            roadmap_block(me, &[sibling(me, "Only", "created", "feature")]),
            ""
        );
        let block = roadmap_block(
            me,
            &[
                sibling(other, "Scaffold", "complete", "scaffold"),
                sibling(me, "Cart", "in_progress", "feature"),
            ],
        );
        assert!(
            block.contains("1. [complete] scaffold — Scaffold"),
            "{block}"
        );
        assert!(
            block.contains("2. [in_progress] feature — Cart (this task)"),
            "{block}"
        );
    }

    #[test]
    fn the_review_block_lists_findings_with_their_place_and_the_reason_for_the_run() {
        let findings = vec![Finding {
            id: "r1-1".into(),
            severity: "major".into(),
            file: Some("src/cart.ts".into()),
            line: Some(12),
            title: "Empty cart is not handled".into(),
            detail: "checkout() throws on []".into(),
            thread_id: None,
            reviewer: Some("CodeRabbit".into()),
        }];
        let block = review_block(&findings, Some("check test failed"));
        assert!(
            block.contains("Why this run was started: check test failed"),
            "{block}"
        );
        assert!(block.contains("- r1-1 [major] src/cart.ts:12: Empty cart is not handled — checkout() throws on [] (raised by CodeRabbit)"), "{block}");
        assert_eq!(review_block(&[], None), "");
        assert_eq!(review_block(&[], Some("  ")), "");
    }
}
