//! The verdict a reviewer model ends its review with.
//!
//! Strict JSON between two markers, in the shape `agent::verification` reads
//! its own marker: the model says what it decided and every finding it wants
//! changed, and anything that does not parse is a failed round rather than a
//! guessed one. Findings are numbered here, not by the model, so an id is
//! stable across the rounds that refer back to it.

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use crate::db::auto_projects::Finding;

pub const OPEN_TAG: &str = "<zone-review>";
pub const CLOSE_TAG: &str = "</zone-review>";

/// Findings one round may raise; past this the review is a rewrite.
pub const MAX_FINDINGS: usize = 40;

/// Bytes the payload may take.
const MAX_PAYLOAD_BYTES: usize = 60_000;

const SEVERITIES: [&str; 4] = ["blocker", "major", "minor", "nit"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Approve,
    RequestChanges,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub outcome: Outcome,
    pub summary: String,
    pub findings: Vec<Finding>,
    /// Ids of earlier findings -- Zone's or a bot's thread -- this round found resolved.
    pub addressed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum VerdictError {
    #[error("the reply carries no <zone-review> marker")]
    Missing,
    #[error("the reply carries more than one <zone-review> marker")]
    Repeated,
    #[error("the <zone-review> tags are out of order")]
    Malformed,
    #[error("the <zone-review> payload is empty or over its size budget")]
    PayloadOutOfRange,
    #[error(
        "the <zone-review> payload is not an object with verdict, summary, findings and addressed: {0}"
    )]
    Shape(String),
    #[error("verdict must be approve or request_changes")]
    Outcome,
    #[error("finding {index} has a severity outside blocker, major, minor, nit")]
    Severity { index: usize },
    #[error("finding {index} has no title")]
    Title { index: usize },
    #[error("the review raises more than {MAX_FINDINGS} findings")]
    TooManyFindings,
}

#[derive(Deserialize)]
struct Payload {
    verdict: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    findings: Vec<RawFinding>,
    #[serde(default)]
    addressed: Vec<Value>,
}

#[derive(Deserialize)]
struct RawFinding {
    #[serde(default)]
    severity: String,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    line: Option<Value>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    detail: String,
}

/// Find and validate the marker in a reviewer's final message.
pub fn parse(content: &str, round: i32) -> Result<Verdict, VerdictError> {
    let opened = content.matches(OPEN_TAG).count();
    let closed = content.matches(CLOSE_TAG).count();
    if opened == 0 || closed == 0 {
        return Err(VerdictError::Missing);
    }
    if opened > 1 || closed > 1 {
        return Err(VerdictError::Repeated);
    }
    let start = content.find(OPEN_TAG).ok_or(VerdictError::Missing)? + OPEN_TAG.len();
    let end = content.find(CLOSE_TAG).ok_or(VerdictError::Missing)?;
    if end < start {
        return Err(VerdictError::Malformed);
    }
    let payload = content[start..end].trim();
    if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES {
        return Err(VerdictError::PayloadOutOfRange);
    }
    let parsed: Payload =
        serde_json::from_str(payload).map_err(|error| VerdictError::Shape(error.to_string()))?;
    let outcome = match parsed.verdict.trim().to_ascii_lowercase().as_str() {
        "approve" | "approved" => Outcome::Approve,
        "request_changes" | "request changes" | "changes_requested" => Outcome::RequestChanges,
        _ => return Err(VerdictError::Outcome),
    };
    if parsed.findings.len() > MAX_FINDINGS {
        return Err(VerdictError::TooManyFindings);
    }
    let mut findings = Vec::with_capacity(parsed.findings.len());
    for (index, raw) in parsed.findings.into_iter().enumerate() {
        let severity = raw.severity.trim().to_ascii_lowercase();
        if !SEVERITIES.contains(&severity.as_str()) {
            return Err(VerdictError::Severity { index: index + 1 });
        }
        if raw.title.trim().is_empty() {
            return Err(VerdictError::Title { index: index + 1 });
        }
        findings.push(Finding {
            id: format!("r{round}-{}", index + 1),
            severity,
            file: raw
                .file
                .map(|file| file.trim().to_string())
                .filter(|file| !file.is_empty()),
            line: raw.line.and_then(|line| match line {
                Value::Number(number) => number.as_u64().and_then(|n| u32::try_from(n).ok()),
                Value::String(text) => text.trim().parse().ok(),
                _ => None,
            }),
            title: raw.title.trim().to_string(),
            detail: raw.detail.trim().to_string(),
            thread_id: None,
            reviewer: None,
        });
    }
    let addressed = parsed
        .addressed
        .into_iter()
        .filter_map(|item| match item {
            Value::String(text) => Some(text.trim().to_string()),
            Value::Object(object) => object
                .get("id")
                .and_then(Value::as_str)
                .map(|id| id.trim().to_string()),
            _ => None,
        })
        .filter(|id| !id.is_empty())
        .collect();
    // A round that raises nothing and approves has decided; a round that
    // raises a blocker and approves has contradicted itself, and the
    // finding wins.
    let outcome = if outcome == Outcome::Approve
        && findings
            .iter()
            .any(|finding| matches!(finding.severity.as_str(), "blocker" | "major"))
    {
        Outcome::RequestChanges
    } else {
        outcome
    };
    Ok(Verdict {
        outcome,
        summary: parsed.summary.trim().to_string(),
        findings,
        addressed,
    })
}

/// The review as GitHub shows it: the verdict, the summary, every finding.
pub fn comment(verdict: &Verdict, reviewer: &str, round: i32, same_model: bool) -> String {
    let mut body = format!(
        "**Zone review, round {round}** — {} ({reviewer}{})\n",
        match verdict.outcome {
            Outcome::Approve => "approve",
            Outcome::RequestChanges => "request changes",
        },
        if same_model {
            ", the model that wrote the change"
        } else {
            ""
        }
    );
    if !verdict.summary.is_empty() {
        body.push('\n');
        body.push_str(&verdict.summary);
        body.push('\n');
    }
    if !verdict.findings.is_empty() {
        body.push_str("\n**Findings**\n");
        for finding in &verdict.findings {
            let mut line = format!("- `{}` **{}**", finding.id, finding.severity);
            if let Some(file) = &finding.file {
                line.push_str(&format!(" `{file}"));
                if let Some(number) = finding.line {
                    line.push_str(&format!(":{number}"));
                }
                line.push('`');
            }
            line.push_str(&format!(" — {}", finding.title));
            if !finding.detail.is_empty() {
                line.push_str(&format!("\n  {}", finding.detail.replace('\n', "\n  ")));
            }
            body.push_str(&line);
            body.push('\n');
        }
    }
    if !verdict.addressed.is_empty() {
        body.push_str(&format!(
            "\nAddressed since the last round: {}\n",
            verdict.addressed.join(", ")
        ));
    }
    body.push_str("\n_Posted by Zone's automation; this is a comment, not an approval._\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = r#"I read the diff.
<zone-review>
{"verdict":"request_changes","summary":"The cart handles items but not an empty checkout.",
 "findings":[{"severity":"major","file":"src/cart.ts","line":12,"title":"Empty cart throws","detail":"checkout() calls items[0]."},
             {"severity":"nit","title":"Rename total to subtotal"}],
 "addressed":["r1-1", {"id":"PRRT_7"}]}
</zone-review>"#;

    #[test]
    fn a_verdict_is_read_with_numbered_findings_and_addressed_ids() {
        let verdict = parse(REPLY, 2).unwrap();
        assert_eq!(verdict.outcome, Outcome::RequestChanges);
        assert_eq!(verdict.findings.len(), 2);
        assert_eq!(verdict.findings[0].id, "r2-1");
        assert_eq!(verdict.findings[0].file.as_deref(), Some("src/cart.ts"));
        assert_eq!(verdict.findings[0].line, Some(12));
        assert_eq!(verdict.findings[1].id, "r2-2");
        assert_eq!(verdict.findings[1].severity, "nit");
        assert_eq!(verdict.addressed, vec!["r1-1", "PRRT_7"]);
    }

    #[test]
    fn an_approval_that_raises_a_blocker_is_read_as_a_request_for_changes() {
        let reply = r#"<zone-review>{"verdict":"approve","findings":[{"severity":"blocker","title":"SQL injection"}]}</zone-review>"#;
        assert_eq!(parse(reply, 1).unwrap().outcome, Outcome::RequestChanges);
        let clean = r#"<zone-review>{"verdict":"approve","summary":"fine","findings":[{"severity":"nit","title":"spacing"}]}</zone-review>"#;
        assert_eq!(parse(clean, 1).unwrap().outcome, Outcome::Approve);
    }

    #[test]
    fn every_malformed_reply_names_what_is_wrong() {
        assert_eq!(parse("no marker", 1), Err(VerdictError::Missing));
        assert_eq!(
            parse(
                "<zone-review>{}</zone-review><zone-review>{}</zone-review>",
                1
            ),
            Err(VerdictError::Repeated)
        );
        assert_eq!(
            parse("</zone-review>{}<zone-review>", 1),
            Err(VerdictError::Malformed)
        );
        assert_eq!(
            parse("<zone-review></zone-review>", 1),
            Err(VerdictError::PayloadOutOfRange)
        );
        assert!(matches!(
            parse("<zone-review>[1]</zone-review>", 1),
            Err(VerdictError::Shape(_))
        ));
        assert_eq!(
            parse(r#"<zone-review>{"verdict":"maybe"}</zone-review>"#, 1),
            Err(VerdictError::Outcome)
        );
        assert_eq!(
            parse(
                r#"<zone-review>{"verdict":"approve","findings":[{"severity":"huge","title":"x"}]}</zone-review>"#,
                1
            ),
            Err(VerdictError::Severity { index: 1 })
        );
        assert_eq!(
            parse(
                r#"<zone-review>{"verdict":"approve","findings":[{"severity":"nit","title":" "}]}</zone-review>"#,
                1
            ),
            Err(VerdictError::Title { index: 1 })
        );
    }

    #[test]
    fn the_github_comment_carries_verdict_findings_and_the_same_model_label() {
        let verdict = parse(REPLY, 2).unwrap();
        let rendered = comment(&verdict, "qwen3:32b", 2, true);
        assert!(
            rendered.contains(
                "round 2** — request changes (qwen3:32b, the model that wrote the change)"
            ),
            "{rendered}"
        );
        assert!(
            rendered.contains("- `r2-1` **major** `src/cart.ts:12` — Empty cart throws"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Addressed since the last round: r1-1, PRRT_7"),
            "{rendered}"
        );
        assert!(rendered.contains("not an approval"), "{rendered}");
    }
}
