//! Structured citations for live GitHub observations and workspace documents.
//!
//! Tool results already carry source URLs, commit SHAs and freshness. This
//! module turns those observations into a stable message-metadata shape the
//! console can render, and it refuses to treat incomplete evidence as a pass.
//!
//! Every citation also records how its outcome was produced. A server-side
//! fetch or execution proves one; a model only claims one. A claimed outcome is
//! advisory evidence and can never be a passing result, exactly as incomplete
//! evidence cannot.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::verification::{Provenance, Verdict, VerificationOutcome};

const INCOMPLETE_NOTE: &str = "Incomplete evidence is not a passing result.";
const ADVISORY_NOTE: &str = "A model-asserted outcome is advisory evidence, not a passing result.";
const UNAVAILABLE_NOTE: &str = "No safe behavioral check was available, so nothing was proven.";
const VERIFICATION_TITLE: &str = "Behavioral verification";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CitationKind {
    GithubBuild,
    GithubDeployment,
    GithubIssue,
    GithubFile,
    WorkspaceDocument,
    BehavioralVerification,
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
    /// How the outcome was produced. Stored citations predate this field and
    /// were all built from server-side fetches, so they default to it.
    #[serde(default)]
    pub provenance: Provenance,
    pub outcome: CitationOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Citation {
    pub fn normalize(mut self) -> Self {
        if self.outcome == CitationOutcome::Success && !self.complete {
            self.outcome = CitationOutcome::Incomplete;
            if self.note.is_none() {
                self.note = Some(INCOMPLETE_NOTE.into());
            }
        }
        if self.outcome == CitationOutcome::Success && self.provenance.advisory() {
            self.outcome = CitationOutcome::Observed;
            if self.note.is_none() {
                self.note = Some(ADVISORY_NOTE.into());
            }
        }
        self
    }

    pub fn usable(&self) -> bool {
        !self.title.trim().is_empty() && !self.url.trim().is_empty() && !self.observed_at.is_empty()
    }

    pub fn passing(&self) -> bool {
        self.complete && self.provenance.authoritative() && self.outcome == CitationOutcome::Success
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
    if let Some(existing) = value
        .get("citations")
        .and_then(|citations| parse_citations(citations, provenance_of(name)))
    {
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

/// Built-in tools whose citations record an observation the server itself made
/// against an immutable ref. Anything absent — an MCP server, which is a
/// third-party process, or a tool added later — is advisory until it is
/// deliberately added here.
const SERVER_OBSERVED_TOOLS: &[&str] = &[
    "assess_pull_requests",
    "assess_release_pipelines",
    "get_build_status",
    "list_deployments",
    "list_documents",
    "list_issues",
    "list_projects",
    "read_check_logs",
    "read_document",
    "read_repository_file",
    "search_knowledge",
];

fn provenance_of(name: &str) -> Provenance {
    if SERVER_OBSERVED_TOOLS.contains(&name) {
        Provenance::ServerExecution
    } else {
        Provenance::ModelAsserted
    }
}

/// Citations a tool emitted in its own output.
///
/// A tool cannot certify itself, so provenance comes from which tool ran, never
/// from the payload: an unrecognised tool is advisory whatever it claims or
/// omits.
fn parse_citations(value: &Value, provenance: Provenance) -> Option<Vec<Citation>> {
    let parsed: Vec<Citation> = serde_json::from_value(value.clone()).ok()?;
    // The tool's own standing is a ceiling, never an assignment. An allowlisted
    // tool observes server-side, so its citations may be authoritative — but a
    // payload that calls itself advisory is believed, because believing it can
    // only weaken the claim. A tool that is not allowlisted is advisory
    // whatever it says.
    let attributed = parsed.into_iter().map(|mut citation| {
        citation.provenance = if citation.provenance.advisory() {
            Provenance::ModelAsserted
        } else {
            provenance
        };
        citation
    });
    let finished = finish(attributed.collect());
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
        provenance: Provenance::ServerExecution,
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
                provenance: Provenance::ServerExecution,
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
                provenance: Provenance::ServerExecution,
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
            .get("complete")
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                value
                    .get("content")
                    .and_then(Value::as_str)
                    .is_some_and(|content| !content.is_empty())
            }),
        provenance: Provenance::ServerExecution,
        outcome: CitationOutcome::Observed,
        note: None,
    }
}

/// Citation for a behavioral verification.
///
/// The outcome carries its own provenance, so a model-asserted verdict lands
/// here as advisory evidence and `normalize` refuses to let it read as a pass.
/// Only a [`VerificationOutcome::proven`] result can.
pub fn from_verification(
    outcome: &VerificationOutcome,
    title: &str,
    url: &str,
    revision: Option<&str>,
    observed_at: &str,
) -> Citation {
    Citation {
        kind: CitationKind::BehavioralVerification,
        title: nonempty(title.to_string()).unwrap_or_else(|| VERIFICATION_TITLE.to_string()),
        url: url.to_string(),
        revision: revision.and_then(|revision| nonempty(revision.to_string())),
        observed_at: observed_at.to_string(),
        complete: outcome.complete(),
        provenance: outcome.provenance(),
        outcome: verification_outcome(outcome.verdict()),
        note: verification_note(outcome),
    }
    .normalize()
}

fn verification_outcome(verdict: Verdict) -> CitationOutcome {
    match verdict {
        Verdict::Verified => CitationOutcome::Success,
        Verdict::NotVerified => CitationOutcome::Failure,
        Verdict::Unavailable => CitationOutcome::Incomplete,
    }
}

fn verification_note(outcome: &VerificationOutcome) -> Option<String> {
    if outcome.advisory() {
        return Some(ADVISORY_NOTE.to_string());
    }
    (!outcome.complete()).then(|| UNAVAILABLE_NOTE.to_string())
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
        provenance: Provenance::ServerExecution,
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
        provenance: Provenance::ServerExecution,
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
    use super::super::verification;
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
            provenance: Provenance::ServerExecution,
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

        let paged = citations(
            "read_repository_file",
            json!({
                "path": "providers.rs",
                "sha": SHA,
                "content": "partial",
                "complete": false,
                "next": 8000,
                "url": format!("https://github.com/owner/repository/blob/{SHA}/providers.rs"),
                "observed_at": OBSERVED
            }),
        )
        .remove(0);
        assert!(!paged.complete);
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

    fn marker(verdict: &str) -> verification::Marker {
        let payload = format!(
            r#"{{"version":1,"outcome":"{verdict}","recipes":[{{"kind":"script","manifestPath":"package.json","name":"test:e2e"}}]}}"#
        );
        verification::parse(&format!(
            "{}{payload}{}",
            verification::OPEN_TAG,
            verification::CLOSE_TAG
        ))
        .expect("the fixture marker parses")
    }

    fn verification_citation(outcome: &VerificationOutcome, title: &str) -> Citation {
        from_verification(
            outcome,
            title,
            "https://github.com/owner/repository/blob/main/console/package.json",
            Some(SHA),
            OBSERVED,
        )
    }

    #[test]
    fn a_model_asserted_verification_is_never_a_passing_result() {
        let citation = verification_citation(
            &VerificationOutcome::asserted(marker("verified")),
            "Checkout probe",
        );

        assert_eq!(citation.kind, CitationKind::BehavioralVerification);
        assert_eq!(citation.provenance, Provenance::ModelAsserted);
        assert_eq!(citation.revision.as_deref(), Some(SHA));
        assert!(citation.complete);
        assert_eq!(citation.outcome, CitationOutcome::Observed);
        assert!(!citation.passing());
        assert_eq!(citation.note.as_deref(), Some(ADVISORY_NOTE));
        assert!(citation.usable());
    }

    #[test]
    fn the_same_verification_passes_once_the_server_proves_it() {
        let asserted = VerificationOutcome::asserted(marker("verified"));
        let closure = verification::scaffold::proven();
        let proven = VerificationOutcome::proven(
            closure.witness().expect("the scaffolded closure held"),
            asserted.verdict(),
            asserted.recipes().to_vec(),
        );
        assert_eq!(asserted.verdict(), proven.verdict());
        assert_eq!(asserted.recipes(), proven.recipes());
        assert_eq!(proven.closure(), Some(closure.entrypoint()));

        let citation = verification_citation(&proven, "Checkout probe");

        assert_eq!(citation.provenance, Provenance::ServerExecution);
        assert_eq!(citation.outcome, CitationOutcome::Success);
        assert!(citation.passing());
        assert!(citation.note.is_none());
        assert!(!verification_citation(&asserted, "Checkout probe").passing());
    }

    #[test]
    fn refuted_and_unavailable_verifications_are_never_passing() {
        let refuted = verification_citation(
            &VerificationOutcome::asserted(marker("not_verified")),
            "Checkout probe",
        );
        assert_eq!(refuted.outcome, CitationOutcome::Failure);
        assert_eq!(refuted.provenance, Provenance::ModelAsserted);
        assert!(!refuted.passing());
        assert_eq!(refuted.note.as_deref(), Some(ADVISORY_NOTE));

        let closure = verification::scaffold::proven();
        let unavailable = verification_citation(
            &VerificationOutcome::proven(
                closure.witness().expect("the scaffolded closure held"),
                Verdict::Unavailable,
                Vec::new(),
            ),
            "",
        );
        assert_eq!(unavailable.title, VERIFICATION_TITLE);
        assert!(!unavailable.complete);
        assert_eq!(unavailable.outcome, CitationOutcome::Incomplete);
        assert!(!unavailable.passing());
        assert_eq!(unavailable.note.as_deref(), Some(UNAVAILABLE_NOTE));
    }

    #[test]
    fn a_tool_cannot_declare_a_passing_model_asserted_citation() {
        let smuggled = citations(
            "search_knowledge",
            json!({
                "citations": [{
                    "kind": "behavioral_verification",
                    "title": "Checkout probe",
                    "url": "https://github.com/owner/repository/commit/aaa",
                    "observed_at": OBSERVED,
                    "complete": true,
                    "provenance": "model_asserted",
                    "outcome": "success"
                }]
            }),
        )
        .remove(0);

        assert_eq!(smuggled.provenance, Provenance::ModelAsserted);
        assert_eq!(smuggled.outcome, CitationOutcome::Observed);
        assert!(!smuggled.passing());
        assert_eq!(smuggled.note.as_deref(), Some(ADVISORY_NOTE));
    }

    /// The allowlist is keyed by string with no compile-time link to the tools
    /// that emit citations, so a new emitter is misclassified silently. This
    /// reads the tool sources and fails when one grows a citations key without
    /// a decision about its provenance.
    #[test]
    fn every_tool_that_emits_citations_has_a_stated_provenance() {
        const SOURCES: [(&str, &str); 2] = [
            ("tools.rs", include_str!("tools.rs")),
            ("integrations.rs", include_str!("integrations.rs")),
        ];

        for (file, source) in SOURCES {
            let emits = source
                .lines()
                .filter(|line| line.contains("\"citations\":"))
                .count();
            assert!(
                emits > 0,
                "{file} no longer emits citations; drop it from this test or the allowlist entry \
                 it was covering is now dead"
            );
        }

        for tool in ["search_knowledge", "list_projects", "read_check_logs"] {
            assert_eq!(
                provenance_of(tool),
                Provenance::ServerExecution,
                "{tool} emits citations built from a server-side observation, so it must be \
                 listed in SERVER_OBSERVED_TOOLS or its citations are demoted to advisory"
            );
        }
    }

    #[test]
    fn an_unrecognised_tool_cannot_certify_itself() {
        let supplied = citations(
            "magents_spawn_session",
            json!({
                "citations": [{
                    "kind": "github_build",
                    "title": "repository main@aaaaaaa",
                    "url": "https://github.com/owner/repository/commit/aaa",
                    "observed_at": OBSERVED,
                    "complete": true,
                    "outcome": "success"
                }]
            }),
        )
        .remove(0);

        assert_eq!(supplied.provenance, Provenance::ModelAsserted);
        assert_eq!(supplied.outcome, CitationOutcome::Observed);
        assert!(!supplied.passing());

        let wire = serde_json::to_value(&supplied).expect("a citation serializes");
        assert_eq!(wire["kind"], "github_build");
        assert_eq!(wire["provenance"], "model_asserted");
        assert_eq!(wire["outcome"], "observed");
    }

    #[test]
    fn a_built_in_tool_keeps_its_server_observation() {
        let observed = citations(
            "assess_pull_requests",
            json!({
                "citations": [{
                    "kind": "github_build",
                    "title": "repository main@aaaaaaa",
                    "url": "https://github.com/owner/repository/commit/aaa",
                    "observed_at": OBSERVED,
                    "complete": true,
                    "outcome": "success"
                }]
            }),
        )
        .remove(0);

        assert_eq!(observed.provenance, Provenance::ServerExecution);
        assert!(observed.passing());
    }

    #[test]
    fn a_stored_citation_without_provenance_stays_server_proven() {
        let stored: Citation = serde_json::from_value(json!({
            "kind": "github_build",
            "title": "repository main@aaaaaaa",
            "url": "https://github.com/owner/repository/commit/aaa",
            "observed_at": OBSERVED,
            "complete": true,
            "outcome": "success"
        }))
        .expect("a stored citation deserializes");

        assert_eq!(stored.provenance, Provenance::ServerExecution);
        assert!(stored.passing());
    }
}
