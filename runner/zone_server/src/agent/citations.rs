//! Structured citations for live GitHub observations and workspace documents.
//!
//! Tool results already carry source URLs, commit SHAs and freshness. This
//! module turns those observations into a stable message-metadata shape the
//! console can render, and it refuses to treat incomplete evidence as a pass.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CitationKind {
    GithubBuild,
    GithubDeployment,
    GithubIssue,
    GithubFile,
    WorkspaceDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CitationOutcome {
    Success,
    Failure,
    Pending,
    Incomplete,
    Observed,
}

/// One checkable source behind an agent reply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Citation {
    pub kind: CitationKind,
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub observed_at: String,
    pub complete: bool,
    pub outcome: CitationOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Citation {
    pub fn normalize(mut self) -> Self {
        if self.outcome == CitationOutcome::Success && !self.complete {
            self.outcome = CitationOutcome::Incomplete;
            if self.note.is_none() {
                self.note = Some("Incomplete evidence is not a passing result.".into());
            }
        }
        self
    }

    pub fn usable(&self) -> bool {
        !self.title.trim().is_empty() && !self.url.trim().is_empty() && !self.observed_at.is_empty()
    }

    pub fn passing(&self) -> bool {
        self.complete && self.outcome == CitationOutcome::Success
    }
}

/// Extract citations from a successful tool result.
pub fn from_tool(name: &str, output: &str) -> Vec<Citation> {
    from_tool_at(name, output, &Utc::now().to_rfc3339())
}

pub fn from_tool_at(name: &str, output: &str, observed_at: &str) -> Vec<Citation> {
    let Ok(value) = serde_json::from_str::<Value>(output) else {
        return Vec::new();
    };
    if let Some(existing) = value.get("citations").and_then(parse_citations) {
        return existing;
    }
    let citations = match name {
        "get_build_status" => vec![build_citation(&value, observed_at)],
        "list_deployments" => deployment_citations(&value, observed_at),
        "list_issues" => issue_citations(&value, observed_at),
        "read_repository_file" => vec![file_citation(&value, observed_at)],
        "read_document" | "list_documents" => document_citations(&value, observed_at),
        _ => Vec::new(),
    };
    finish(citations)
}

pub fn merge(existing: &mut Vec<Citation>, incoming: impl IntoIterator<Item = Citation>) {
    for citation in incoming {
        if !citation.usable() {
            continue;
        }
        if existing
            .iter()
            .any(|seen| seen.url == citation.url && seen.revision == citation.revision)
        {
            continue;
        }
        existing.push(citation);
    }
}

fn parse_citations(value: &Value) -> Option<Vec<Citation>> {
    let parsed: Vec<Citation> = serde_json::from_value(value.clone()).ok()?;
    let finished = finish(parsed);
    (!finished.is_empty()).then_some(finished)
}

fn finish(citations: Vec<Citation>) -> Vec<Citation> {
    citations
        .into_iter()
        .map(Citation::normalize)
        .filter(Citation::usable)
        .collect()
}

fn build_citation(value: &Value, observed_at: &str) -> Citation {
    let sha = text(value, "sha");
    let outcome = outcome_from_state(&text(value, "state"));
    let fetched = value
        .get("complete")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let complete = fetched && !matches!(outcome, CitationOutcome::Incomplete);
    let note = text(value, "assessment");
    Citation {
        kind: CitationKind::GithubBuild,
        title: build_title(value, &sha),
        url: first_http([
            commit_url(value, &sha),
            first_html_url(value.get("workflows")),
            first_html_url(value.get("checks")),
        ]),
        revision: nonempty(sha),
        observed_at: observed(value, observed_at),
        complete,
        outcome,
        note: nonempty(note),
    }
}

fn deployment_citations(value: &Value, observed_at: &str) -> Vec<Citation> {
    let Some(rows) = value.get("deployments").and_then(Value::as_array) else {
        return Vec::new();
    };
    let sha = text(value, "sha");
    let observed_at = observed(value, observed_at);
    let note = Some("Deployment records do not prove the deployed service is healthy.".into());
    rows.iter()
        .map(|row| {
            let environment = text(row, "environment");
            let status = row.get("status").cloned().unwrap_or(Value::Null);
            let state = text(&status, "state");
            let outcome = if state.is_empty() {
                CitationOutcome::Incomplete
            } else if state == "inactive" {
                CitationOutcome::Observed
            } else {
                outcome_from_state(&state)
            };
            let revision = nonempty(text(row, "sha")).or_else(|| nonempty(sha.clone()));
            Citation {
                kind: CitationKind::GithubDeployment,
                title: if environment.is_empty() {
                    "GitHub deployment".into()
                } else {
                    format!("{environment} deployment")
                },
                url: first_http([
                    text(&status, "environment_url"),
                    commit_url(value, revision.as_deref().unwrap_or_default()),
                    text(row, "url"),
                ]),
                revision,
                observed_at: observed_at.clone(),
                complete: !matches!(outcome, CitationOutcome::Incomplete),
                outcome,
                note: note.clone(),
            }
        })
        .collect()
}

fn issue_citations(value: &Value, observed_at: &str) -> Vec<Citation> {
    let Some(rows) = value.get("issues").and_then(Value::as_array) else {
        return Vec::new();
    };
    let observed_at = observed(value, observed_at);
    rows.iter()
        .map(|row| {
            let number = row
                .get("number")
                .and_then(Value::as_u64)
                .map(|number| format!("#{number}"))
                .unwrap_or_else(|| "GitHub issue".into());
            let title = text(row, "title");
            Citation {
                kind: CitationKind::GithubIssue,
                title: if title.is_empty() {
                    number
                } else {
                    format!("{number} {title}")
                },
                url: text(row, "html_url"),
                revision: nonempty(text(row, "updated_at")),
                observed_at: observed_at.clone(),
                complete: row.get("body").is_some_and(|body| !body.is_null()),
                outcome: CitationOutcome::Observed,
                note: None,
            }
        })
        .collect()
}

fn file_citation(value: &Value, observed_at: &str) -> Citation {
    let path = text(value, "path");
    let sha = text(value, "sha");
    let blob = text(value, "blob_sha");
    Citation {
        kind: CitationKind::GithubFile,
        title: if path.is_empty() {
            "Repository file".into()
        } else {
            path
        },
        url: first_http([text(value, "url"), commit_url(value, &sha)]),
        revision: nonempty(sha).or_else(|| nonempty(blob)),
        observed_at: observed(value, observed_at),
        complete: value
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|content| !content.is_empty()),
        outcome: CitationOutcome::Observed,
        note: None,
    }
}

/// Citation for a retrieved knowledge entry or indexed source chunk.
pub fn from_retrieved(title: &str, uri: &str, complete: bool, observed_at: &str) -> Citation {
    let (kind, url, revision) = indexed_uri(uri);
    Citation {
        kind,
        title: if title.trim().is_empty() {
            url.clone()
        } else {
            title.to_string()
        },
        url,
        revision,
        observed_at: observed_at.to_string(),
        complete,
        outcome: if complete {
            CitationOutcome::Observed
        } else {
            CitationOutcome::Incomplete
        },
        note: None,
    }
    .normalize()
}

fn indexed_uri(uri: &str) -> (CitationKind, String, Option<String>) {
    if let Some(rest) = uri.strip_prefix("github://") {
        if let Some((path, revision)) = rest.rsplit_once('@') {
            let mut parts = path.splitn(3, '/');
            if let (Some(owner), Some(repo), Some(file)) =
                (parts.next(), parts.next(), parts.next())
            {
                return (
                    CitationKind::GithubFile,
                    format!("https://github.com/{owner}/{repo}/blob/{revision}/{file}"),
                    Some(revision.to_string()),
                );
            }
        }
        return (CitationKind::GithubFile, uri.to_string(), None);
    }
    (CitationKind::WorkspaceDocument, uri.to_string(), None)
}

fn document_citations(value: &Value, observed_at: &str) -> Vec<Citation> {
    if let Some(document) = value.get("document") {
        return vec![document_citation(document, value, observed_at)];
    }
    let Some(documents) = value.get("documents").and_then(Value::as_array) else {
        return Vec::new();
    };
    documents
        .iter()
        .map(|document| document_citation(document, value, observed_at))
        .collect()
}

fn document_citation(document: &Value, parent: &Value, observed_at: &str) -> Citation {
    let title = text(document, "title");
    let url = text(document, "uri");
    let has_content = document
        .get("content")
        .is_some_and(|content| !content.is_null());
    let complete = parent
        .get("complete")
        .and_then(Value::as_bool)
        .unwrap_or(has_content);
    Citation {
        kind: CitationKind::WorkspaceDocument,
        title: if title.is_empty() {
            if url.is_empty() {
                "Workspace document".into()
            } else {
                url.clone()
            }
        } else {
            title
        },
        url,
        revision: nonempty(text(document, "revision"))
            .or_else(|| nonempty(text(document, "updated_at")))
            .or_else(|| nonempty(text(document, "fetched_at"))),
        observed_at: observed(parent, observed_at),
        complete,
        outcome: if complete {
            CitationOutcome::Observed
        } else {
            CitationOutcome::Incomplete
        },
        note: (!complete)
            .then(|| "Stored content was unavailable; this is not a complete document.".into()),
    }
}

fn outcome_from_state(state: &str) -> CitationOutcome {
    match state {
        "success" => CitationOutcome::Success,
        "failure" | "error" | "cancelled" | "timed_out" | "action_required" | "startup_failure"
        | "stale" => CitationOutcome::Failure,
        "pending" | "queued" | "in_progress" | "waiting" | "requested" => CitationOutcome::Pending,
        _ => CitationOutcome::Incomplete,
    }
}

fn build_title(value: &Value, sha: &str) -> String {
    let repository = text(value, "repository");
    let name = repository
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or("GitHub build");
    let short = short_revision(sha);
    match nonempty(text(value, "ref")) {
        Some(reference) if !short.is_empty() => format!("{name} {reference}@{short}"),
        _ if !short.is_empty() => format!("{name} @{short}"),
        _ => name.to_string(),
    }
}

fn commit_url(value: &Value, sha: &str) -> String {
    let repository = text(value, "repository");
    if repository.is_empty() || sha.is_empty() {
        return String::new();
    }
    format!("{}/commit/{sha}", repository.trim_end_matches('/'))
}

fn first_html_url(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| text(row, "html_url"))
        .find(|url| is_http(url))
        .unwrap_or_default()
}

fn first_http<const N: usize>(candidates: [String; N]) -> String {
    candidates
        .iter()
        .find(|url| is_http(url) || url.starts_with('/'))
        .cloned()
        .or_else(|| candidates.into_iter().find(|url| !url.is_empty()))
        .unwrap_or_default()
}

fn is_http(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

fn observed(value: &Value, fallback: &str) -> String {
    nonempty(text(value, "observed_at")).unwrap_or_else(|| fallback.to_string())
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn nonempty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn short_revision(revision: &str) -> &str {
    if revision.len() >= 7 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        &revision[..7]
    } else {
        revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const OBSERVED: &str = "2026-09-05T00:00:00+00:00";
    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn citations(name: &str, value: Value) -> Vec<Citation> {
        from_tool_at(name, &value.to_string(), OBSERVED)
    }

    #[test]
    fn empty_or_unknown_build_is_incomplete_evidence_not_a_pass() {
        let citation = citations(
            "get_build_status",
            json!({
                "repository": "https://github.com/owner/repository",
                "ref": "main",
                "sha": SHA,
                "state": "unknown",
                "complete": true,
                "assessment": "Observed CI only; required branch checks and service health are not evaluated.",
                "observed_at": OBSERVED
            }),
        )
        .remove(0);

        assert_eq!(citation.kind, CitationKind::GithubBuild);
        assert_eq!(
            citation.url,
            format!("https://github.com/owner/repository/commit/{SHA}")
        );
        assert_eq!(citation.revision.as_deref(), Some(SHA));
        assert_eq!(citation.observed_at, OBSERVED);
        assert!(!citation.complete);
        assert_eq!(citation.outcome, CitationOutcome::Incomplete);
        assert!(!citation.passing());
        assert!(citation.note.unwrap().contains("Observed CI only"));
    }

    #[test]
    fn successful_complete_build_is_a_passing_result() {
        let citation = citations(
            "get_build_status",
            json!({
                "repository": "https://github.com/owner/repository",
                "ref": "main",
                "sha": SHA,
                "state": "success",
                "complete": true,
                "observed_at": OBSERVED
            }),
        )
        .remove(0);

        assert!(citation.complete);
        assert_eq!(citation.outcome, CitationOutcome::Success);
        assert!(citation.passing());
        assert_eq!(citation.title, "repository main@aaaaaaa");
    }

    #[test]
    fn claimed_success_without_complete_evidence_is_normalized_away() {
        let citation = Citation {
            kind: CitationKind::GithubBuild,
            title: "repository".into(),
            url: "https://github.com/owner/repository/commit/aaa".into(),
            revision: Some(SHA.into()),
            observed_at: OBSERVED.into(),
            complete: false,
            outcome: CitationOutcome::Success,
            note: None,
        }
        .normalize();

        assert_eq!(citation.outcome, CitationOutcome::Incomplete);
        assert!(!citation.passing());
        assert_eq!(
            citation.note.as_deref(),
            Some("Incomplete evidence is not a passing result.")
        );
    }

    #[test]
    fn pending_build_is_not_a_pass() {
        let citation = citations(
            "get_build_status",
            json!({
                "repository": "https://github.com/owner/repository",
                "sha": SHA,
                "state": "pending",
                "complete": true,
                "observed_at": OBSERVED
            }),
        )
        .remove(0);

        assert!(citation.complete);
        assert_eq!(citation.outcome, CitationOutcome::Pending);
        assert!(!citation.passing());
    }

    #[test]
    fn deployments_preserve_commit_and_distinguish_pending_from_pass() {
        let citations = citations(
            "list_deployments",
            json!({
                "repository": "https://github.com/owner/repository",
                "sha": SHA,
                "observed_at": OBSERVED,
                "deployments": [{
                    "sha": SHA,
                    "environment": "production",
                    "url": "https://api.github.com/repos/owner/repository/deployments/7",
                    "status": {"state": "pending", "environment_url": "https://prod.example"}
                }]
            }),
        );

        assert_eq!(citations[0].url, "https://prod.example");
        assert_eq!(citations[0].revision.as_deref(), Some(SHA));
        assert_eq!(citations[0].observed_at, OBSERVED);
        assert_eq!(citations[0].outcome, CitationOutcome::Pending);
        assert!(!citations[0].passing());
        assert!(citations[0].note.as_deref().unwrap().contains("healthy"));
    }

    #[test]
    fn issues_and_files_keep_source_url_revision_and_timestamp() {
        let issue = citations(
            "list_issues",
            json!({
                "observed_at": OBSERVED,
                "issues": [{
                    "number": 12,
                    "title": "Flaky deploy",
                    "body": "full body",
                    "html_url": "https://github.com/owner/repository/issues/12",
                    "updated_at": "2026-09-04T18:00:00Z"
                }]
            }),
        )
        .remove(0);
        assert_eq!(issue.title, "#12 Flaky deploy");
        assert_eq!(issue.url, "https://github.com/owner/repository/issues/12");
        assert_eq!(issue.revision.as_deref(), Some("2026-09-04T18:00:00Z"));
        assert_eq!(issue.observed_at, OBSERVED);
        assert!(issue.complete);
        assert_eq!(issue.outcome, CitationOutcome::Observed);

        let file = citations(
            "read_repository_file",
            json!({
                "repository": "https://github.com/owner/repository",
                "path": "README.md",
                "sha": SHA,
                "blob_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "content": "# Zone",
                "url": format!("https://github.com/owner/repository/blob/{SHA}/README.md"),
                "observed_at": OBSERVED
            }),
        )
        .remove(0);
        assert_eq!(
            file.url,
            format!("https://github.com/owner/repository/blob/{SHA}/README.md")
        );
        assert_eq!(file.revision.as_deref(), Some(SHA));
        assert!(file.complete);
        assert!(!file.passing());
    }

    #[test]
    fn workspace_documents_keep_uri_revision_and_mark_missing_content() {
        let listed = citations(
            "list_documents",
            json!({
                "observed_at": OBSERVED,
                "documents": [{
                    "title": "Guide",
                    "uri": "knowledge://11111111-1111-1111-1111-111111111111",
                    "revision": "content-hash",
                    "updated_at": "2026-09-01T00:00:00",
                    "content": null
                }]
            }),
        )
        .remove(0);
        assert_eq!(listed.kind, CitationKind::WorkspaceDocument);
        assert_eq!(
            listed.url,
            "knowledge://11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(listed.revision.as_deref(), Some("content-hash"));
        assert_eq!(listed.observed_at, OBSERVED);
        assert!(!listed.complete);
        assert_eq!(listed.outcome, CitationOutcome::Incomplete);
        assert!(!listed.passing());

        let read = citations(
            "read_document",
            json!({
                "complete": true,
                "content_state": "stored_text",
                "observed_at": OBSERVED,
                "document": {
                    "title": "Guide",
                    "uri": "https://docs.example/guide",
                    "revision": "content-hash",
                    "content": "full text"
                }
            }),
        )
        .remove(0);
        assert_eq!(read.url, "https://docs.example/guide");
        assert!(read.complete);
        assert_eq!(read.outcome, CitationOutcome::Observed);
        assert!(!read.passing());
    }

    #[test]
    fn merge_deduplicates_by_url_and_revision() {
        let mut citations = vec![
            citations(
                "get_build_status",
                json!({
                    "repository": "https://github.com/owner/repository",
                    "sha": SHA,
                    "state": "success",
                    "complete": true,
                    "observed_at": OBSERVED
                }),
            )
            .remove(0),
        ];
        let original = citations.clone();
        merge(&mut citations, original.clone());
        assert_eq!(citations, original);
    }

    #[test]
    fn retrieved_github_uri_becomes_a_blob_url() {
        let citation = from_retrieved(
            "mod.rs",
            "github://abnegate/zone/content/mod.rs@main",
            true,
            OBSERVED,
        );
        assert_eq!(citation.kind, CitationKind::GithubFile);
        assert_eq!(
            citation.url,
            "https://github.com/abnegate/zone/blob/main/content/mod.rs"
        );
        assert_eq!(citation.revision.as_deref(), Some("main"));
        assert!(citation.complete);
        assert!(citation.usable());
    }

    #[test]
    fn retrieved_knowledge_uri_stays_a_workspace_document() {
        let citation = from_retrieved(
            "Guide",
            "knowledge://11111111-1111-1111-1111-111111111111",
            true,
            OBSERVED,
        );
        assert_eq!(citation.kind, CitationKind::WorkspaceDocument);
        assert_eq!(
            citation.url,
            "knowledge://11111111-1111-1111-1111-111111111111"
        );
    }

    #[test]
    fn search_knowledge_json_citations_are_honored() {
        let found = citations(
            "search_knowledge",
            json!({
                "query": "should_skip_blob",
                "citations": [{
                    "kind": "github_file",
                    "title": "mod.rs",
                    "url": "https://github.com/abnegate/zone/blob/main/content/mod.rs",
                    "revision": "main",
                    "observed_at": OBSERVED,
                    "complete": true,
                    "outcome": "observed"
                }]
            }),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, CitationKind::GithubFile);
    }
}
