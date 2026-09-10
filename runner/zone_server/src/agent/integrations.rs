//! Live GitHub observations and writes using an authorized workspace source.

use async_trait::async_trait;
use chrono::Utc;
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use uuid::Uuid;
use zone_core::tools::{
    MAX_TOOL_MESSAGE_CHARS, REASON_PARAM, Tool, ToolContext, ToolError, ToolRegistry, ToolResult,
    reason_property,
};

use super::readiness::{
    self, CheckEvidence, CommentEvidence, CommitSha, PullEvidence, ReviewComment, ReviewSignals,
    ReviewThread, ThreadEvidence,
};
use super::releases::{self, Lookup, ReleaseIdentity, ReleasePipeline, RunEvidence};
use super::tools::{WorkspaceScope, truncate};
use crate::db::{sources, workspace_members};
use crate::services::prioritisation::{
    Configuration as Prioritisation, Prioritiser, RiskSignal, pull_request,
};

const ORIGIN: &str = "https://api.github.com/";
const PAGE_SIZE: usize = 100;
const FILE_PAGE_CHARS: u64 = 8_000;
const ISSUE_BODY_CHARS: usize = 1_500;
const BUILD_RECORD_CAP: usize = 20;
const PULL_PAGE_SIZE: usize = 10;
const PULL_TITLE_CHARS: usize = 160;
const REVIEW_PAGE_SIZE: usize = 100;
const PULL_REVIEW_QUERY: &str = "query($owner:String!,$name:String!,$number:Int!,$first:Int!){\
repository(owner:$owner,name:$name){pullRequest(number:$number){number isDraft headRefOid \
reviewThreads(first:$first){pageInfo{hasNextPage}nodes{isResolved}}\
comments(last:$first){pageInfo{hasPreviousPage}nodes{databaseId body url createdAt author{login}}}\
}}}";
const READINESS_ASSESSMENT: &str = "Observed checks, review threads and review-bot comments only. Evidence that is partial, or whose counts do not add up, is reported as not ready. This is not proof that branch protection requirements are satisfied.";
const PULL_FILE_PAGES: u32 = 30;
const RELEASE_PAGE_SIZE: usize = 5;
const RELEASE_EVENT: &str = "release";
const RELEASE_ASSESSMENT: &str = "Observed release-triggered workflow runs only. A lookup that could not be completed, a run record whose identity does not add up, and a release with no observed run are all reported as unknown rather than succeeded. This is not proof that release artifacts were published or that the deployed service is healthy.";
const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;
const LOG_EXCERPT_CHARS: u64 = 5_500;
const LOG_EXCERPT_FLOOR_CHARS: u64 = 500;
const LOG_CONTEXT_LINES: usize = 40;
const LOG_TAIL_LINES: usize = 120;
const LOG_LINE_CHARS: usize = 500;
const ELISION_RESERVE_CHARS: usize = 120;
const RESULT_ENVELOPE_CHARS: usize = 256;
const ANNOTATED_ERROR: &str = "##[error]";
const ERROR_MARKERS: [&str; 10] = [
    ANNOTATED_ERROR,
    "traceback (most recent call last)",
    "segmentation fault",
    "assertion failed",
    "exception:",
    "failure:",
    "error:",
    "fatal:",
    "panic:",
    " failed",
];
const LOG_REDIRECT_HOSTS: [&str; 4] = [
    "blob.core.windows.net",
    "pipelines.actions.githubusercontent.com",
    "objects.githubusercontent.com",
    "github.com",
];
const LOG_NOTE: &str = "Excerpt of what the job printed, not proof of why it failed. Per-line timestamps, progress redraws and over-long lines are trimmed, and unshown regions are marked as omitted.";

/// Compiling the path vocabulary is the expensive part, so the prioritiser
/// behind a blast radius is built once and shared by every assessment.
static PRIORITISER: LazyLock<Prioritiser> =
    LazyLock::new(|| Prioritiser::new(Prioritisation::default()));

#[derive(Clone, Copy)]
enum Operation {
    Build,
    Deployments,
    Issues,
    File,
    PullRequests,
    ReleasePipelines,
    CheckLogs,
    CreatePull,
    Comment,
}

pub fn register(registry: &mut ToolRegistry, scope: &WorkspaceScope) {
    for operation in [
        Operation::Build,
        Operation::Deployments,
        Operation::Issues,
        Operation::File,
        Operation::PullRequests,
        Operation::ReleasePipelines,
        Operation::CheckLogs,
        Operation::CreatePull,
        Operation::Comment,
    ] {
        registry.register(Arc::new(Integration {
            scope: scope.clone(),
            operation,
        }));
    }
}

struct Integration {
    scope: WorkspaceScope,
    operation: Operation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    source_id: Uuid,
    #[serde(rename = "ref")]
    reference: Option<String>,
    path: Option<String>,
    page: Option<u32>,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    limit: Option<u64>,
    state: Option<String>,
    title: Option<String>,
    head: Option<String>,
    base: Option<String>,
    body: Option<String>,
    number: Option<u64>,
    job_id: Option<u64>,
    tag: Option<String>,
    /// Why the model made this write. Declared so `deny_unknown_fields` accepts
    /// the property the write schemas advertise; a parse failure here reaches
    /// the model as an unrepairable message, so its absence never fails a call.
    reason: Option<String>,
}

#[derive(Deserialize)]
struct Configuration {
    owner: String,
    repo: String,
    branch: Option<String>,
    path: Option<String>,
    token: Option<String>,
    /// Which review bots count as review evidence for this source. Absent
    /// means every bot this build recognises.
    review_signals: Option<Vec<String>>,
}

#[async_trait]
impl Tool for Integration {
    fn name(&self) -> &str {
        match self.operation {
            Operation::Build => "get_build_status",
            Operation::Deployments => "list_deployments",
            Operation::Issues => "list_issues",
            Operation::File => "read_repository_file",
            Operation::CheckLogs => "read_check_logs",
            Operation::PullRequests => "assess_pull_requests",
            Operation::ReleasePipelines => "assess_release_pipelines",
            Operation::CreatePull => "create_pull_request",
            Operation::Comment => "comment_on_issue",
        }
    }

    fn description(&self) -> &str {
        match self.operation {
            Operation::Build => {
                "Read live GitHub workflows, check runs and commit statuses at an immutable commit from a connected workspace source. Missing or incomplete evidence is never green; this is not proof that branch protection requirements are satisfied."
            }
            Operation::Deployments => {
                "Read live GitHub deployments and their latest statuses for a connected source at an immutable commit. Results are paginated; deployment records do not prove the deployed service is healthy."
            }
            Operation::Issues => {
                "Read live GitHub issues (excluding pull requests) from a connected workspace source. Returns titles, numbers, state, urls and bounded body snippets, plus a next page when more provider records exist."
            }
            Operation::File => {
                "Read UTF-8 content of a specific repository file from a connected GitHub source at an immutable commit, with a source URL. Returns a character page that fits the context budget; follow next to continue. Does not read host files. GitHub files over 100 MB are unsupported."
            }
            Operation::PullRequests => {
                "Assess whether pull requests on a connected GitHub source are ready to merge, returning a verdict plus every blocker behind it. Ready is the conjunction of every positive condition: no unresolved review threads, a summary from every recognised review bot that spoke, each at its own full confidence on its own scale and naming the current head commit, and checks that both passed and whose counts add up. Counts that do not reconcile, a check state outside the known set, a thread or comment list that could not be paginated in full, and a head commit the observations disagree on are all reported as not ready. Each verdict also carries a blast radius read from the changed files; that is information for a reviewer and never affects readiness, and it says so when no file list was observed. Absent evidence is never ready, this is not proof that branch protection requirements are satisfied, and nothing is merged. Reads authenticated review threads, so the source needs a credential."
            }
            Operation::ReleasePipelines => {
                "Assess the workflow pipelines behind published GitHub releases on a connected source, returning each release's pipeline state plus every reason it is not green. Succeeded is the conjunction of every positive condition: the lookup completed, at least one release-triggered workflow run was observed, every observed run names the same commit, and every one of them succeeded. A lookup that could not be completed, a run record whose identity or state does not add up, runs that disagree on the commit they built, and a release with no observed run are all reported as unknown rather than succeeded. Results are paginated. This is what the workflows reported: it is not proof that release artifacts were published, that the deployed service is healthy, or that a rerun would pass."
            }
            Operation::CheckLogs => {
                "Read a bounded excerpt of one GitHub Actions job log for a connected source, after confirming the job runs against the pull request or commit being asked about. Returns the lines around the first error and the end of the log within a character budget, never the whole file; omitted regions and capped downloads are marked. An excerpt is evidence of what the job printed, never proof of why it failed, that the log names the real cause, or that a rerun would pass."
            }
            Operation::CreatePull => {
                "Open a pull request on a connected GitHub source. Requires write access. Only do this when the user asked to open a PR."
            }
            Operation::Comment => {
                "Comment on a GitHub issue or pull request for a connected source. Requires write access. Only do this when the user asked to comment."
            }
        }
    }

    fn parameters_schema(&self) -> Value {
        let mut properties = json!({"source_id": {"type": "string", "format": "uuid"}});
        if !matches!(
            self.operation,
            Operation::Issues | Operation::PullRequests | Operation::ReleasePipelines
        ) {
            properties["ref"] = json!({"type": "string", "description": "Branch, tag or commit; defaults to the source branch or repository default branch."});
        }
        if matches!(self.operation, Operation::Deployments | Operation::Issues) {
            properties["page"] = json!({"type": "integer", "minimum": 1, "description": "Provider page (100 records), default 1. Follow next_page until null."});
        }
        if matches!(self.operation, Operation::Issues | Operation::PullRequests) {
            properties["state"] = json!({"type": "string", "enum": ["open", "closed", "all"]});
        }
        if matches!(self.operation, Operation::PullRequests) {
            properties["page"] = json!({"type": "integer", "minimum": 1, "description": "Page of 10 pull requests, most recently updated first, default 1. Follow next_page until null."});
            properties["number"] = json!({"type": "integer", "minimum": 1, "description": "Assess only this pull request instead of a page of them."});
        }
        if matches!(self.operation, Operation::ReleasePipelines) {
            properties["page"] = json!({"type": "integer", "minimum": 1, "description": "Page of 5 releases, most recently published first, default 1. Follow next_page until null."});
            properties["tag"] = json!({"type": "string", "description": "Assess only the release published under this tag instead of a page of releases."});
        }
        let mut required = vec!["source_id"];
        if matches!(self.operation, Operation::File) {
            properties["path"] = json!({"type": "string", "description": "Exact repository-relative file path, within the configured source path."});
            properties["offset"] = json!({"type": "integer", "minimum": 0, "description": "Unicode character offset into the file, default 0."});
            properties["limit"] = json!({"type": "integer", "minimum": 1, "description": "Number of characters to return, default 8000, capped at 8000 so the page fits the remaining context budget. Follow next to continue."});
            required.push("path");
        }
        if matches!(self.operation, Operation::CheckLogs) {
            properties["job_id"] = json!({"type": "integer", "minimum": 1, "description": "GitHub Actions job id, taken from the id of a check returned by get_build_status."});
            properties["number"] = json!({"type": "integer", "minimum": 1, "description": "Pull request number the job must belong to. Omit to bind the job to the commit that ref resolves to instead."});
            properties["limit"] = json!({"type": "integer", "minimum": 1, "description": "Excerpt size in characters, default 5500 and capped at 5500 so the excerpt fits the remaining context budget."});
            required.push("job_id");
        }
        if matches!(self.operation, Operation::CreatePull) {
            properties["title"] = json!({"type": "string", "description": "Pull request title."});
            properties["head"] = json!({"type": "string", "description": "Head branch, or owner:branch for a fork."});
            properties["base"] = json!({"type": "string", "description": "Base branch; defaults to the source branch or repository default."});
            properties["body"] =
                json!({"type": "string", "description": "Pull request description."});
            required.extend(["title", "head"]);
        }
        if matches!(self.operation, Operation::Comment) {
            properties["number"] = json!({"type": "integer", "minimum": 1, "description": "Issue or pull request number."});
            properties["body"] = json!({"type": "string", "description": "Comment markdown."});
            required.extend(["number", "body"]);
        }
        if self.mutating() {
            properties[REASON_PARAM] = reason_property();
            required.push(REASON_PARAM);
        }
        json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
    }

    async fn execute(&self, params: Value, _: &ToolContext) -> Result<ToolResult, ToolError> {
        Ok(match self.run(params).await {
            Ok(value) => ToolResult::success(value.to_string()),
            Err(error) => ToolResult::error(error),
        })
    }

    fn timeout(&self, _: &ToolContext) -> Duration {
        Duration::from_secs(120)
    }

    fn mutating(&self) -> bool {
        matches!(self.operation, Operation::CreatePull | Operation::Comment)
    }
}

impl Integration {
    async fn run(&self, params: Value) -> Result<Value, String> {
        let arguments: Arguments = serde_json::from_value(params)
            .map_err(|_| "Invalid integration arguments.".to_string())?;
        if arguments.page == Some(0) {
            return Err("page must be positive.".to_string());
        }
        if arguments.limit == Some(0) {
            return Err("limit must be positive.".to_string());
        }
        let write = matches!(self.operation, Operation::CreatePull | Operation::Comment);
        let allowed = if write {
            workspace_members::can_write(
                self.scope.state.db(),
                self.scope.workspace_id,
                self.scope.user_id,
            )
            .await
        } else {
            workspace_members::can_read(
                self.scope.state.db(),
                self.scope.workspace_id,
                self.scope.user_id,
            )
            .await
        }
        .map_err(|_| "Workspace authorization failed.".to_string())?;
        if !allowed {
            return Err(if write {
                "You cannot write to this workspace.".to_string()
            } else {
                "You cannot read this workspace.".to_string()
            });
        }
        let source = sources::get_source(
            self.scope.state.db(),
            arguments.source_id,
            self.scope.workspace_id,
        )
        .await
        .map_err(|_| "Could not read the source.".to_string())?
        .filter(|source| source.is_active.unwrap_or(true))
        .ok_or("Source not found in this workspace or inactive.")?;
        if source.source_type != "github" {
            return Err(
                "Live integrations currently support connected GitHub sources only.".to_string(),
            );
        }
        let mut configuration: Configuration = serde_json::from_value(source.config)
            .map_err(|_| "The GitHub source configuration is invalid.".to_string())?;
        if let Some(encrypted) = source.credentials_encrypted {
            configuration.token = Some(
                crate::crypto::decrypt(self.scope.state.encryption_key(), &encrypted)
                    .map_err(|_| "The source credentials could not be decrypted.".to_string())?,
            );
        }
        let github = Github::new(configuration)?;
        let mut result = if write {
            github.write(self.operation, &arguments).await?
        } else {
            github.observe(self.operation, &arguments).await?
        };
        result["source_id"] = json!(arguments.source_id);
        result["observed_at"] = json!(Utc::now().to_rfc3339());
        Ok(result)
    }
}

struct Github {
    client: Client,
    origin: Url,
    log_bytes: u64,
    signals: ReviewSignals,
    configuration: Configuration,
}

impl Github {
    fn new(configuration: Configuration) -> Result<Self, String> {
        if !segment(&configuration.owner) || !segment(&configuration.repo) {
            return Err("The source owner or repository name is invalid.".to_string());
        }
        let signals = match &configuration.review_signals {
            Some(names) => ReviewSignals::select(names)?,
            None => ReviewSignals::recognized(),
        };
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .user_agent("Zone-workspace-tools")
            .build()
            .map_err(|_| "Could not initialize the GitHub client.".to_string())?;
        Ok(Self {
            client,
            origin: Url::parse(ORIGIN).expect("constant GitHub origin"),
            log_bytes: MAX_LOG_BYTES,
            signals,
            configuration,
        })
    }

    /// The `owner/repo` name GitHub reports records under.
    fn full_name(&self) -> String {
        format!("{}/{}", self.configuration.owner, self.configuration.repo)
    }

    fn url(&self, parts: &[&str]) -> Url {
        let mut url = self.origin.clone();
        let mut segments = url
            .path_segments_mut()
            .expect("HTTP origin supports path segments");
        segments.pop_if_empty().extend([
            "repos",
            &self.configuration.owner,
            &self.configuration.repo,
        ]);
        segments.extend(parts.iter().copied());
        drop(segments);
        url
    }

    async fn response(
        &self,
        method: reqwest::Method,
        parts: &[&str],
        query: &[(&str, String)],
        raw: bool,
        body: Option<&Value>,
    ) -> Result<reqwest::Response, String> {
        let mut url = self.url(parts);
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(key, value)| (*key, value.as_str())));
        self.dispatch(method, url, raw, body).await
    }

    async fn dispatch(
        &self,
        method: reqwest::Method,
        url: Url,
        raw: bool,
        body: Option<&Value>,
    ) -> Result<reqwest::Response, String> {
        let mut request = self
            .client
            .request(method, url)
            .header(
                "Accept",
                if raw {
                    "application/vnd.github.raw+json"
                } else {
                    "application/vnd.github+json"
                },
            )
            .header("X-GitHub-Api-Version", "2026-03-10");
        if let Some(token) = &self.configuration.token {
            request = request.bearer_auth(token);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request
            .send()
            .await
            .map_err(|_| "GitHub request failed or timed out.".to_string())?;
        if !response.status().is_success() {
            // Provider response bodies and request errors can contain secrets.
            return Err(format!(
                "GitHub returned HTTP {}. Check source access, permissions, rate limits and the requested resource.",
                response.status().as_u16()
            ));
        }
        Ok(response)
    }

    async fn get(&self, parts: &[&str], query: &[(&str, String)]) -> Result<Value, String> {
        self.response(reqwest::Method::GET, parts, query, false, None)
            .await?
            .json()
            .await
            .map_err(|_| "GitHub returned an invalid response.".to_string())
    }

    async fn post(&self, parts: &[&str], body: &Value) -> Result<Value, String> {
        self.response(reqwest::Method::POST, parts, &[], false, Some(body))
            .await?
            .json()
            .await
            .map_err(|_| "GitHub returned an invalid response.".to_string())
    }

    async fn graphql(&self, query: &str, variables: Value) -> Result<Value, String> {
        let url = self
            .origin
            .join("graphql")
            .map_err(|_| "Could not build the GitHub GraphQL URL.".to_string())?;
        let response: Value = self
            .dispatch(
                reqwest::Method::POST,
                url,
                false,
                Some(&json!({"query": query, "variables": variables})),
            )
            .await?
            .json()
            .await
            .map_err(|_| "GitHub returned an invalid response.".to_string())?;
        if response
            .get("errors")
            .is_some_and(|errors| errors.as_array().is_none_or(|rows| !rows.is_empty()))
        {
            return Err(
                "GitHub rejected the review query. Check source access, permissions and rate limits."
                    .into(),
            );
        }
        Ok(response["data"].clone())
    }

    async fn write(&self, operation: Operation, arguments: &Arguments) -> Result<Value, String> {
        let repository = format!(
            "https://github.com/{}/{}",
            self.configuration.owner, self.configuration.repo
        );
        match operation {
            Operation::CreatePull => {
                let title = arguments
                    .title
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or("title is required.")?;
                let head = arguments
                    .head
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or("head is required.")?;
                if !git_ref(head) {
                    return Err("head is invalid.".to_string());
                }
                let base = arguments
                    .base
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| self.configuration.branch.clone())
                    .ok_or("base is required (or set the source branch).")?;
                if !git_ref(&base) {
                    return Err("base is invalid.".to_string());
                }
                let created = self
                    .post(
                        &["pulls"],
                        &json!({
                            "title": title,
                            "head": head,
                            "base": base,
                            "body": arguments.body.clone().unwrap_or_default(),
                        }),
                    )
                    .await?;
                Ok(json!({
                    "repository": repository,
                    "pull_request": project(&created, &["number", "html_url", "title", "state"]),
                }))
            }
            Operation::Comment => {
                let number = arguments.number.ok_or("number is required.")?;
                if number == 0 {
                    return Err("number must be positive.".to_string());
                }
                let body = arguments
                    .body
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or("body is required.")?;
                let created = self
                    .post(
                        &["issues", &number.to_string(), "comments"],
                        &json!({ "body": body }),
                    )
                    .await?;
                Ok(json!({
                    "repository": repository,
                    "comment": project(&created, &["id", "html_url", "body", "created_at"]),
                }))
            }
            _ => Err("This operation is read-only.".to_string()),
        }
    }

    async fn resolve(&self, reference: Option<&str>) -> Result<(String, String), String> {
        let reference = match reference.or(self.configuration.branch.as_deref()) {
            Some(value) if !value.trim().is_empty() => value.to_string(),
            Some(_) => return Err("ref must not be empty.".to_string()),
            None => self.get(&[], &[]).await?["default_branch"]
                .as_str()
                .ok_or("GitHub did not return a default branch.")?
                .to_string(),
        };
        let commit = self.get(&["commits", &reference], &[]).await?;
        let sha = commit["sha"]
            .as_str()
            .filter(|value| valid_sha(value))
            .ok_or("GitHub did not return an immutable commit SHA.")?
            .to_string();
        Ok((reference, sha))
    }

    async fn observe(&self, operation: Operation, arguments: &Arguments) -> Result<Value, String> {
        let repository = format!(
            "https://github.com/{}/{}",
            self.configuration.owner, self.configuration.repo
        );
        if matches!(operation, Operation::PullRequests) {
            let mut result = self.readiness(arguments).await?;
            result["repository"] = json!(repository);
            return Ok(result);
        }
        if matches!(operation, Operation::ReleasePipelines) {
            let mut result = self.release_pipelines(arguments).await?;
            result["repository"] = json!(repository);
            return Ok(result);
        }
        if matches!(operation, Operation::Issues) {
            let state = arguments.state.as_deref().unwrap_or("open");
            if !matches!(state, "open" | "closed" | "all") {
                return Err("Issue state must be open, closed or all.".to_string());
            }
            let page = arguments.page.unwrap_or(1);
            let records = self
                .get(
                    &["issues"],
                    &[
                        ("state", state.into()),
                        ("per_page", PAGE_SIZE.to_string()),
                        ("page", page.to_string()),
                    ],
                )
                .await?;
            let records = array(&records)?;
            let next = next_page(records.len(), page)?;
            let issues: Vec<Value> = records
                .iter()
                .filter(|row| row.get("pull_request").is_none())
                .map(issue_record)
                .collect();
            return Ok(json!({"repository": repository, "issues": issues, "next_page": next}));
        }
        if matches!(operation, Operation::CheckLogs) {
            let mut result = self.check_logs(arguments).await?;
            result["repository"] = json!(repository);
            return Ok(result);
        }
        let (reference, sha) = self.resolve(arguments.reference.as_deref()).await?;
        let mut result = match operation {
            Operation::Build => self.build(&sha).await?,
            Operation::Deployments => self.deployments(&sha, arguments.page.unwrap_or(1)).await?,
            Operation::File => {
                self.file(
                    &sha,
                    arguments.path.as_deref().ok_or("path is required.")?,
                    arguments.offset.unwrap_or(0),
                    arguments.limit.unwrap_or(FILE_PAGE_CHARS),
                )
                .await?
            }
            Operation::Issues
            | Operation::PullRequests
            | Operation::ReleasePipelines
            | Operation::CheckLogs
            | Operation::CreatePull
            | Operation::Comment => unreachable!(),
        };
        result["repository"] = json!(repository);
        result["ref"] = json!(reference);
        result["sha"] = json!(sha);
        Ok(result)
    }

    async fn pages(
        &self,
        parts: &[&str],
        key: Option<&str>,
        query: &[(&str, String)],
    ) -> Result<Vec<Value>, String> {
        let mut page = 1;
        let mut records = Vec::new();
        loop {
            let mut query = query.to_vec();
            query.extend([
                ("per_page", PAGE_SIZE.to_string()),
                ("page", page.to_string()),
            ]);
            let response = self.get(parts, &query).await?;
            let rows = array(key.map_or(&response, |key| &response[key]))?;
            let next = next_page(rows.len(), page)?;
            records.extend(rows.iter().cloned());
            // Workflow searches are capped by GitHub at 1,000 records.
            if key == Some("workflow_runs") && response["total_count"].as_u64().unwrap_or(0) > 1000
            {
                return Err("GitHub's 1,000-result workflow search limit prevents a complete build assessment.".into());
            }
            match next {
                Some(next) => page = next,
                None => return Ok(records),
            }
        }
    }

    async fn ci_rows(&self, sha: &str) -> Result<(Vec<Value>, Vec<Value>, Vec<Value>), String> {
        let workflows = self
            .pages(
                &["actions", "runs"],
                Some("workflow_runs"),
                &[("head_sha", sha.into())],
            )
            .await?;
        let checks = self
            .pages(
                &["commits", sha, "check-runs"],
                Some("check_runs"),
                &[("filter", "latest".into())],
            )
            .await?;
        let statuses = self.pages(&["commits", sha, "statuses"], None, &[]).await?;
        if workflows
            .iter()
            .any(|row| row["workflow_id"].as_u64().is_none())
            || statuses
                .iter()
                .any(|row| row["context"].as_str().is_none_or(str::is_empty))
        {
            return Err("GitHub returned CI records without their identities.".into());
        }
        let mut identities = std::collections::HashSet::new();
        let workflows: Vec<Value> = workflows
            .into_iter()
            .filter(|row| {
                identities.insert((
                    row["workflow_id"].to_string(),
                    row["event"].to_string(),
                    row["head_branch"].to_string(),
                ))
            })
            .collect();
        let statuses = latest(statuses, "context");
        if workflows
            .iter()
            .chain(checks.iter())
            .any(|row| row["head_sha"].as_str() != Some(sha))
        {
            return Err("GitHub returned checks for a different or missing commit SHA.".into());
        }
        Ok((workflows, checks, statuses))
    }

    async fn build(&self, sha: &str) -> Result<Value, String> {
        let (workflows, checks, statuses) = self.ci_rows(sha).await?;
        let conclusions: Vec<&str> = workflows
            .iter()
            .chain(checks.iter())
            .chain(statuses.iter())
            .map(ci_token)
            .collect();
        let state = assessment(&conclusions);
        Ok(bound_build(json!({"state": state, "complete": true,
            "assessment": "Observed CI only; required branch checks and service health are not evaluated.",
            "workflows": workflows.iter().map(|row| project(row, &["id", "name", "head_sha", "status", "conclusion", "html_url", "updated_at"])).collect::<Vec<_>>(),
            "checks": checks.iter().map(|row| project(row, &["id", "name", "head_sha", "status", "conclusion", "html_url", "details_url", "completed_at"])).collect::<Vec<_>>(),
            "statuses": statuses.iter().map(|row| project(row, &["context", "state", "description", "target_url", "created_at"])).collect::<Vec<_>>() })))
    }

    async fn check_evidence(&self, sha: &str) -> Result<CheckEvidence, String> {
        let (workflows, checks, statuses) = self.ci_rows(sha).await?;
        Ok(tally(
            workflows
                .iter()
                .chain(checks.iter())
                .chain(statuses.iter())
                .map(ci_token),
        ))
    }

    async fn readiness(&self, arguments: &Arguments) -> Result<Value, String> {
        let (rows, next) = match arguments.number {
            Some(number) => {
                if number == 0 {
                    return Err("number must be positive.".to_string());
                }
                (
                    vec![self.get(&["pulls", &number.to_string()], &[]).await?],
                    None,
                )
            }
            None => {
                let state = arguments.state.as_deref().unwrap_or("open");
                if !matches!(state, "open" | "closed" | "all") {
                    return Err("Pull request state must be open, closed or all.".to_string());
                }
                let page = arguments.page.unwrap_or(1);
                let response = self
                    .get(
                        &["pulls"],
                        &[
                            ("state", state.into()),
                            ("sort", "updated".into()),
                            ("direction", "desc".into()),
                            ("per_page", PULL_PAGE_SIZE.to_string()),
                            ("page", page.to_string()),
                        ],
                    )
                    .await?;
                let records = array(&response)?.clone();
                let next = next_page_of(records.len(), page, PULL_PAGE_SIZE)?;
                (records, next)
            }
        };
        let mut assessed = Vec::with_capacity(rows.len());
        for row in &rows {
            let (assessment, risk) = self.assess(row).await?;
            assessed.push(assessment_record(&assessment, &risk));
        }
        Ok(bound_readiness(
            json!({
                "assessed": assessed.len(),
                "ready": assessed.iter().filter(|row| row["ready"] == true).count(),
                "next_page": next,
                "complete": next.is_none(),
                "assessment": READINESS_ASSESSMENT,
            }),
            &assessed,
            &Utc::now().to_rfc3339(),
        ))
    }

    async fn assess(&self, row: &Value) -> Result<(readiness::Assessment, RiskSignal), String> {
        let number = row["number"]
            .as_u64()
            .filter(|number| *number > 0)
            .ok_or("GitHub returned a pull request without a number.")?;
        let draft = row["draft"]
            .as_bool()
            .ok_or("GitHub returned a pull request without its draft state.")?;
        let review = self.review(number).await?;
        let head = row["head"]["sha"]
            .as_str()
            .and_then(CommitSha::parse)
            .filter(|listed| review.head.as_ref() == Some(listed));
        let checks = match &head {
            Some(head) => self.check_evidence(head.as_str()).await?,
            None => CheckEvidence::default(),
        };
        let risk = pull_request::risk(&PRIORITISER, &self.changed_paths(number).await);
        Ok((
            readiness::assess(
                PullEvidence {
                    number,
                    title: truncate(row["title"].as_str().unwrap_or_default(), PULL_TITLE_CHARS),
                    url: text(row, "html_url"),
                    updated_at: text(row, "updated_at"),
                    draft: draft || review.draft,
                    head,
                    threads: review.threads,
                    comments: review.comments,
                    checks,
                },
                &self.signals,
            ),
            risk,
        ))
    }

    /// The repository-relative paths a pull request touches.
    ///
    /// A list that could not be retrieved in full is returned as no paths at
    /// all. The blast radius that follows then reads as assumed rather than
    /// measured, which is the honest answer: a partial diff would produce a
    /// confident tier from evidence that was never complete.
    async fn changed_paths(&self, number: u64) -> Vec<String> {
        let number = number.to_string();
        let mut paths = Vec::new();
        let mut page = 1;
        loop {
            let Ok(response) = self
                .get(
                    &["pulls", &number, "files"],
                    &[
                        ("per_page", PAGE_SIZE.to_string()),
                        ("page", page.to_string()),
                    ],
                )
                .await
            else {
                return Vec::new();
            };
            let Ok(rows) = array(&response) else {
                return Vec::new();
            };
            for row in rows {
                match row["filename"].as_str() {
                    Some(filename) if !filename.trim().is_empty() => {
                        paths.push(filename.to_string());
                    }
                    _ => return Vec::new(),
                }
            }
            match next_page(rows.len(), page) {
                Ok(None) => return paths,
                Ok(Some(next)) if next <= PULL_FILE_PAGES => page = next,
                Ok(Some(_)) | Err(_) => return Vec::new(),
            }
        }
    }

    async fn review(&self, number: u64) -> Result<Review, String> {
        let data = self
            .graphql(
                PULL_REVIEW_QUERY,
                json!({
                    "owner": self.configuration.owner,
                    "name": self.configuration.repo,
                    "number": number,
                    "first": REVIEW_PAGE_SIZE,
                }),
            )
            .await?;
        let pull = &data["repository"]["pullRequest"];
        if pull["number"].as_u64() != Some(number) {
            return Err(
                "GitHub returned review threads for a different or missing pull request.".into(),
            );
        }
        let threads = &pull["reviewThreads"];
        let comments = &pull["comments"];
        Ok(Review {
            draft: pull["isDraft"].as_bool().unwrap_or(true),
            head: pull["headRefOid"].as_str().and_then(CommitSha::parse),
            threads: ThreadEvidence {
                complete: threads["pageInfo"]["hasNextPage"].as_bool() == Some(false),
                threads: array(&threads["nodes"])?
                    .iter()
                    .map(|node| ReviewThread {
                        resolved: node["isResolved"].as_bool().unwrap_or(false),
                    })
                    .collect(),
            },
            comments: CommentEvidence {
                complete: comments["pageInfo"]["hasPreviousPage"].as_bool() == Some(false),
                comments: array(&comments["nodes"])?
                    .iter()
                    .map(review_comment)
                    .collect(),
            },
        })
    }

    async fn release_pipelines(&self, arguments: &Arguments) -> Result<Value, String> {
        let page = arguments.page.unwrap_or(1);
        let (rows, next) = match arguments.tag.as_deref() {
            Some(tag) => {
                if !git_ref(tag) {
                    return Err("tag is invalid.".to_string());
                }
                (vec![self.get(&["releases", "tags", tag], &[]).await?], None)
            }
            None => {
                let response = self
                    .get(
                        &["releases"],
                        &[
                            ("per_page", RELEASE_PAGE_SIZE.to_string()),
                            ("page", page.to_string()),
                        ],
                    )
                    .await?;
                let records = array(&response)?.clone();
                let next = next_page_of(records.len(), page, RELEASE_PAGE_SIZE)?;
                (records, next)
            }
        };
        let releases: Vec<ReleaseIdentity> = rows
            .iter()
            .map(|row| self.release_identity(row))
            .collect::<Result<_, _>>()?;
        let checked_at = Utc::now().to_rfc3339();
        let sweep = self.pipeline_sweep(&releases).await;
        let mut assessed = Vec::with_capacity(releases.len());
        for release in releases {
            let observed = match &sweep {
                Some(records) => ReleasePipeline::observe(release.clone(), records, &checked_at),
                None => ReleasePipeline::unavailable(release.clone(), &checked_at),
            };
            let pipeline = if observed.lookup == Lookup::Complete {
                observed
            } else {
                releases::merge(
                    Some(observed),
                    self.exact_pipeline(&release, &checked_at).await,
                )
            };
            assessed.push(pipeline_record(&pipeline));
        }
        Ok(bound_releases(
            json!({
                "assessed": assessed.len(),
                "succeeded": assessed.iter().filter(|row| row["succeeded"] == true).count(),
                "next_page": next,
                "complete": next.is_none(),
                "assessment": RELEASE_ASSESSMENT,
            }),
            &assessed,
            &checked_at,
        ))
    }

    fn release_identity(&self, row: &Value) -> Result<ReleaseIdentity, String> {
        let release = ReleaseIdentity {
            repository: self.full_name(),
            id: row["id"].as_u64().unwrap_or_default(),
            tag: text(row, "tag_name"),
            published_at: text(row, "published_at"),
            url: text(row, "html_url"),
        };
        release
            .valid()
            .then_some(release)
            .ok_or_else(|| "GitHub returned a release without a usable identity.".to_string())
    }

    /// One pass over the repository's release-triggered runs, bounded to the
    /// window the listed releases were published in.
    ///
    /// `None` means the sweep could not be completed, which is not the same as
    /// finding nothing: every release then falls back to its own lookup rather
    /// than reading an empty sweep as an absent pipeline.
    async fn pipeline_sweep(&self, releases: &[ReleaseIdentity]) -> Option<Vec<RunEvidence>> {
        let earliest = releases
            .iter()
            .min_by_key(|release| release.published())
            .map(|release| release.published_at.as_str())?;
        self.pipeline_runs(&[("created", format!(">={earliest}"))])
            .await
            .ok()
    }

    /// The runs GitHub itself attributes to one release's tag.
    async fn exact_pipeline(&self, release: &ReleaseIdentity, checked_at: &str) -> ReleasePipeline {
        match self.pipeline_runs(&[("branch", release.tag.clone())]).await {
            Ok(records) => ReleasePipeline::observe(release.clone(), &records, checked_at),
            Err(_) => ReleasePipeline::unavailable(release.clone(), checked_at),
        }
    }

    async fn pipeline_runs(&self, query: &[(&str, String)]) -> Result<Vec<RunEvidence>, String> {
        let mut query = query.to_vec();
        query.push(("event", RELEASE_EVENT.to_string()));
        Ok(self
            .pages(&["actions", "runs"], Some("workflow_runs"), &query)
            .await?
            .iter()
            .map(|row| self.run_evidence(row))
            .collect())
    }

    fn run_evidence(&self, row: &Value) -> RunEvidence {
        RunEvidence {
            id: row["id"].as_u64().unwrap_or_default(),
            workflow_id: row["workflow_id"].as_u64().unwrap_or_default(),
            attempt: row["run_attempt"]
                .as_u64()
                .and_then(|attempt| u32::try_from(attempt).ok())
                .unwrap_or_default(),
            name: text(row, "name"),
            path: text(row, "path"),
            event: text(row, "event"),
            status: text(row, "status"),
            conclusion: row["conclusion"].as_str().map(str::to_string),
            head_branch: text(row, "head_branch"),
            head_sha: text(row, "head_sha"),
            created_at: text(row, "created_at"),
            started_at: row["run_started_at"].as_str().map(str::to_string),
            updated_at: text(row, "updated_at"),
            url: text(row, "html_url"),
            repository: text(&row["repository"], "full_name"),
        }
    }

    async fn deployments(&self, sha: &str, page: u32) -> Result<Value, String> {
        let records = self
            .get(
                &["deployments"],
                &[
                    ("sha", sha.into()),
                    ("per_page", PAGE_SIZE.to_string()),
                    ("page", page.to_string()),
                ],
            )
            .await?;
        let records = array(&records)?;
        let mut deployments = Vec::new();
        for row in records {
            if row["sha"].as_str() != Some(sha) {
                return Err(
                    "GitHub returned a deployment for a different or missing commit SHA.".into(),
                );
            }
            let id = row["id"]
                .as_u64()
                .ok_or("GitHub returned an invalid deployment ID.")?
                .to_string();
            let response = self
                .get(
                    &["deployments", &id, "statuses"],
                    &[("per_page", "1".into())],
                )
                .await?;
            let status = array(&response)?.first().cloned().unwrap_or(Value::Null);
            let mut deployment = project(
                row,
                &[
                    "id",
                    "sha",
                    "ref",
                    "environment",
                    "created_at",
                    "updated_at",
                ],
            );
            deployment["status"] = project(
                &status,
                &[
                    "state",
                    "description",
                    "environment_url",
                    "log_url",
                    "created_at",
                ],
            );
            deployment["url"] = json!(self.url(&["deployments", &id]).to_string());
            deployments.push(deployment);
        }
        Ok(json!({"deployments": deployments, "next_page": next_page(records.len(), page)?}))
    }

    async fn file(&self, sha: &str, path: &str, offset: u64, limit: u64) -> Result<Value, String> {
        validate_path(path, self.configuration.path.as_deref())?;
        let mut tree = sha.to_string();
        let mut metadata = Value::Null;
        let components: Vec<&str> = path.split('/').collect();
        for (index, component) in components.iter().enumerate() {
            let response = self.get(&["git", "trees", &tree], &[]).await?;
            if response["truncated"] != false {
                return Err("GitHub returned an incomplete repository tree.".into());
            }
            metadata = array(&response["tree"])?
                .iter()
                .find(|entry| entry["path"].as_str() == Some(component))
                .cloned()
                .ok_or("The file path was not found in this commit.")?;
            let last = index + 1 == components.len();
            if (last
                && (metadata["type"] != "blob"
                    || !matches!(metadata["mode"].as_str(), Some("100644" | "100755"))))
                || (!last && metadata["type"] != "tree")
            {
                return Err("The path must identify a regular repository file without symlinks or submodules.".into());
            }
            tree = metadata["sha"]
                .as_str()
                .filter(|value| valid_sha(value))
                .ok_or("GitHub did not return a valid tree or file SHA.")?
                .to_string();
        }
        let blob = tree;
        let bytes = self
            .response(
                reqwest::Method::GET,
                &["git", "blobs", &blob],
                &[],
                true,
                None,
            )
            .await?
            .bytes()
            .await
            .map_err(|_| "Could not read the complete repository file.".to_string())?;
        if metadata["size"].as_u64() != Some(bytes.len() as u64) {
            return Err("The file response size did not match its metadata.".into());
        }
        let content =
            std::str::from_utf8(&bytes).map_err(|_| "The file is not UTF-8 text.".to_string())?;
        let (page, total, next) = text_page(content, offset, limit)?;
        Ok(
            json!({"path": path, "content": page, "bytes": bytes.len(), "blob_sha": blob,
            "offset": offset, "next": next, "total": total, "complete": next.is_none(),
            "url": format!("https://github.com/{}/{}/blob/{}/{}", self.configuration.owner, self.configuration.repo, sha, path.split('/').map(|part| urlencoding::encode(part).into_owned()).collect::<Vec<_>>().join("/"))}),
        )
    }
    async fn check_logs(&self, arguments: &Arguments) -> Result<Value, String> {
        let job_id = arguments.job_id.ok_or("job_id is required.")?;
        if job_id == 0 {
            return Err("job_id must be positive.".to_string());
        }
        let (reference, sha, pull) = self.log_target(arguments).await?;
        let job = self
            .get(&["actions", "jobs", &job_id.to_string()], &[])
            .await?;
        let run_id = self.verify_job(&job, job_id, &sha)?;
        let (bytes, capped) = self.collect_log(self.log_response(job_id).await?).await?;
        let lines = log_lines(&String::from_utf8_lossy(&bytes));
        let log = JobLog {
            job,
            run_id,
            sha,
            reference,
            pull,
            downloaded: bytes.len(),
            capped,
            observed_at: Utc::now().to_rfc3339(),
        };
        let mut budget = arguments
            .limit
            .unwrap_or(LOG_EXCERPT_CHARS)
            .clamp(LOG_EXCERPT_FLOOR_CHARS, LOG_EXCERPT_CHARS);
        let envelope = MAX_TOOL_MESSAGE_CHARS.saturating_sub(RESULT_ENVELOPE_CHARS);
        loop {
            let result = log.result(&lines, budget);
            if json_chars(&result) <= envelope || budget <= LOG_EXCERPT_FLOOR_CHARS {
                return Ok(result);
            }
            budget = (budget / 2).max(LOG_EXCERPT_FLOOR_CHARS);
        }
    }

    async fn log_target(
        &self,
        arguments: &Arguments,
    ) -> Result<(String, String, Option<Value>), String> {
        let Some(number) = arguments.number else {
            let (reference, sha) = self.resolve(arguments.reference.as_deref()).await?;
            return Ok((reference, sha, None));
        };
        if number == 0 {
            return Err("number must be positive.".to_string());
        }
        let pull = self.get(&["pulls", &number.to_string()], &[]).await?;
        if pull["number"].as_u64() != Some(number) {
            return Err("GitHub returned a different pull request.".to_string());
        }
        let sha = pull["head"]["sha"]
            .as_str()
            .filter(|value| valid_sha(value))
            .ok_or("GitHub did not return an immutable pull request head SHA.")?
            .to_ascii_lowercase();
        let reference = pull["head"]["ref"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("refs/pull/{number}/head"));
        Ok((
            reference,
            sha,
            Some(project(
                &pull,
                &["number", "title", "state", "draft", "html_url"],
            )),
        ))
    }

    fn verify_job(&self, job: &Value, job_id: u64, sha: &str) -> Result<u64, String> {
        let run_id = job["run_id"]
            .as_u64()
            .filter(|value| *value > 0)
            .ok_or("GitHub returned a job without its workflow run identity.")?;
        let head_sha = job["head_sha"]
            .as_str()
            .filter(|value| valid_sha(value))
            .ok_or("GitHub returned a job without a commit SHA.")?;
        if job["id"].as_u64() != Some(job_id)
            || !head_sha.eq_ignore_ascii_case(sha)
            || !is_job_url(
                job["html_url"].as_str().unwrap_or_default(),
                &self.configuration.owner,
                &self.configuration.repo,
                run_id,
                job_id,
            )
        {
            return Err(
                "That job does not belong to the pull request or commit being asked about."
                    .to_string(),
            );
        }
        Ok(run_id)
    }

    async fn log_response(&self, job_id: u64) -> Result<reqwest::Response, String> {
        let mut request = self
            .client
            .get(self.url(&["actions", "jobs", &job_id.to_string(), "logs"]))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2026-03-10");
        if let Some(token) = &self.configuration.token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|_| "GitHub request failed or timed out.".to_string())?;
        if response.status().is_success() {
            return Ok(response);
        }
        if !response.status().is_redirection() {
            return Err(format!(
                "GitHub returned HTTP {}. Check source access, permissions, rate limits and the requested resource.",
                response.status().as_u16()
            ));
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or("GitHub redirected the job log without a destination.")?;
        let target = self.log_redirect(location)?;
        // The signed destination carries its own authorization; forwarding the
        // source token off the API origin would hand it to storage.
        let redirected = self
            .client
            .get(target)
            .send()
            .await
            .map_err(|_| "The job log download failed or timed out.".to_string())?;
        if !redirected.status().is_success() {
            return Err(format!(
                "The job log download returned HTTP {}.",
                redirected.status().as_u16()
            ));
        }
        Ok(redirected)
    }

    fn log_redirect(&self, location: &str) -> Result<Url, String> {
        let unexpected = "GitHub redirected the job log to an unexpected host.";
        let target = self
            .origin
            .join(location)
            .map_err(|_| unexpected.to_string())?;
        let Some(host) = target.host_str() else {
            return Err(unexpected.to_string());
        };
        if !target.username().is_empty() || target.password().is_some() {
            return Err(unexpected.to_string());
        }
        let same_origin = target.scheme() == self.origin.scheme()
            && Some(host) == self.origin.host_str()
            && target.port_or_known_default() == self.origin.port_or_known_default();
        let published = target.scheme() == "https"
            && LOG_REDIRECT_HOSTS
                .iter()
                .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")));
        if same_origin || published {
            Ok(target)
        } else {
            Err(unexpected.to_string())
        }
    }

    async fn collect_log(
        &self,
        mut response: reqwest::Response,
    ) -> Result<(Vec<u8>, bool), String> {
        let mut collected: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "The job log download failed part way through.".to_string())?
        {
            let room = (self.log_bytes as usize).saturating_sub(collected.len());
            if chunk.len() > room {
                collected.extend_from_slice(&chunk[..room]);
                return Ok((collected, true));
            }
            collected.extend_from_slice(&chunk);
        }
        Ok((collected, false))
    }
}

struct JobLog {
    job: Value,
    run_id: u64,
    sha: String,
    reference: String,
    pull: Option<Value>,
    downloaded: usize,
    capped: bool,
    observed_at: String,
}

impl JobLog {
    fn result(&self, lines: &[String], budget: u64) -> Value {
        let error_line = find_error(lines);
        let selected = select(lines, budget as usize, error_line);
        let complete = !self.capped && selected.len() == lines.len();
        let mut result = json!({
            "job": project(&self.job, &["id", "run_id", "name", "status", "conclusion", "html_url", "started_at", "completed_at"]),
            "run_id": self.run_id,
            "ref": self.reference,
            "sha": self.sha,
            "log": {
                "excerpt": render_excerpt(lines, &selected),
                "total_lines": lines.len(),
                "shown_lines": selected.len(),
                "first_error_line": error_line.map(|index| index + 1),
                "downloaded_bytes": self.downloaded,
                "download_capped": self.capped,
                "complete": complete,
            },
            "note": LOG_NOTE,
            "citations": [self.citation(complete)],
        });
        if let Some(pull) = &self.pull {
            result["pull_request"] = pull.clone();
        }
        result
    }

    fn citation(&self, complete: bool) -> Value {
        let conclusion = if self.job["status"].as_str() == Some("completed") {
            self.job["conclusion"].as_str().unwrap_or("unknown")
        } else {
            "pending"
        };
        json!({
            "kind": "github_build",
            "title": format!("{} job log excerpt", self.job["name"].as_str().unwrap_or("Check")),
            "url": self.job["html_url"].as_str().unwrap_or_default(),
            "revision": self.sha,
            "observed_at": self.observed_at,
            "complete": complete,
            "outcome": match assessment(&[conclusion]) {
                "failure" => "failure",
                "pending" => "pending",
                "success" => "success",
                _ => "observed",
            },
            "note": LOG_NOTE,
        })
    }
}

fn is_job_url(value: &str, owner: &str, repo: &str, run_id: u64, job_id: u64) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().eq_ignore_ascii_case(&format!(
            "/{owner}/{repo}/actions/runs/{run_id}/job/{job_id}"
        ))
}

/// Strip the RFC 3339 stamp GitHub prefixes to every line of a job log.
fn strip_timestamp(line: &str) -> &str {
    let bytes = line.as_bytes();
    if bytes.len() < 21
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || !bytes[..4].iter().all(u8::is_ascii_digit)
    {
        return line;
    }
    match line.find("Z ") {
        Some(offset) if offset <= 32 => &line[offset + 2..],
        _ => line,
    }
}

/// Split a job log into lines a budget can be spent on: one entry per printed
/// line, carriage-return redraws collapsed to the state that survived them, and
/// no single line wide enough to crowd out the rest.
fn log_lines(log: &str) -> Vec<String> {
    let mut lines: Vec<String> = log
        .split('\n')
        .map(|line| {
            let settled = line.trim_end_matches('\r');
            let settled = settled.rsplit('\r').next().unwrap_or(settled);
            truncate(strip_timestamp(settled).trim_end(), LOG_LINE_CHARS)
        })
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn find_error(lines: &[String]) -> Option<usize> {
    let mut fallback = None;
    for (index, line) in lines.iter().enumerate() {
        let lowered = line.to_lowercase();
        if lowered.contains(ANNOTATED_ERROR) {
            return Some(index);
        }
        if fallback.is_none() && ERROR_MARKERS.iter().any(|marker| lowered.contains(marker)) {
            fallback = Some(index);
        }
    }
    fallback
}

/// Lines worth spending the budget on, most valuable first: the first error and
/// the context that explains it, then the end of the log. Each step widens a
/// region by one line, so whatever the budget affords stays contiguous.
fn candidates(lines: &[String], error_line: Option<usize>) -> Vec<usize> {
    let last = lines.len() - 1;
    let mut order = Vec::new();
    if let Some(index) = error_line {
        order.push(index);
        for offset in 1..=LOG_CONTEXT_LINES {
            if index + offset <= last {
                order.push(index + offset);
            }
            if let Some(before) = index.checked_sub(offset) {
                order.push(before);
            }
        }
    }
    order.extend((0..LOG_TAIL_LINES.min(lines.len())).map(|offset| last - offset));
    order
}

fn select(
    lines: &[String],
    budget: usize,
    error_line: Option<usize>,
) -> std::collections::BTreeSet<usize> {
    let mut selected = std::collections::BTreeSet::new();
    if lines.is_empty() {
        return selected;
    }
    let mut spent = ELISION_RESERVE_CHARS;
    for index in candidates(lines, error_line) {
        if selected.contains(&index) {
            continue;
        }
        let cost = lines[index].chars().count() + 1;
        // Stopping rather than skipping keeps each region contiguous, so the
        // excerpt reads as the log did instead of as scattered lines.
        if !selected.is_empty() && spent + cost > budget {
            break;
        }
        spent += cost;
        selected.insert(index);
    }
    selected
}

fn ranges(selected: &std::collections::BTreeSet<usize>) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for index in selected {
        match ranges.last_mut() {
            Some(last) if last.1 + 1 == *index => last.1 = *index,
            _ => ranges.push((*index, *index)),
        }
    }
    ranges
}

fn render_excerpt(lines: &[String], selected: &std::collections::BTreeSet<usize>) -> String {
    let mut rendered: Vec<String> = Vec::new();
    let mut cursor = 0;
    for (start, end) in ranges(selected) {
        if start > cursor {
            rendered.push(elision(start - cursor));
        }
        rendered.extend_from_slice(&lines[start..=end]);
        cursor = end + 1;
    }
    if cursor < lines.len() {
        rendered.push(elision(lines.len() - cursor));
    }
    rendered.join("\n")
}

fn elision(count: usize) -> String {
    format!("… {count} lines omitted …")
}

fn text_page(content: &str, offset: u64, limit: u64) -> Result<(String, u64, Option<u64>), String> {
    let total = content.chars().count() as u64;
    if limit == 0 || offset > total {
        return Err("File page offset or length is invalid.".into());
    }
    let count = limit.min(FILE_PAGE_CHARS).min(total.saturating_sub(offset));
    let page: String = content
        .chars()
        .skip(offset as usize)
        .take(count as usize)
        .collect();
    let end = offset + count;
    Ok((page, total, (end < total).then_some(end)))
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

fn git_ref(value: &str) -> bool {
    if let Some((owner, branch)) = value.split_once(':') {
        segment(owner) && segment(branch)
    } else {
        segment(value)
    }
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_path(path: &str, root: Option<&str>) -> Result<(), String> {
    if path.is_empty()
        || path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || path.contains(['\\', '\0'])
    {
        return Err("Use an exact repository-relative file path without traversal.".into());
    }
    if let Some(root) = root
        .map(|root| root.trim_matches('/'))
        .filter(|root| !root.is_empty())
        && path != root
        && !path.starts_with(&format!("{root}/"))
    {
        return Err("The file is outside the connected source path.".into());
    }
    Ok(())
}

fn array(value: &Value) -> Result<&Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| "GitHub returned an invalid record list.".to_string())
}

fn next_page(length: usize, page: u32) -> Result<Option<u32>, String> {
    next_page_of(length, page, PAGE_SIZE)
}

fn next_page_of(length: usize, page: u32, size: usize) -> Result<Option<u32>, String> {
    if length < size {
        Ok(None)
    } else {
        page.checked_add(1)
            .map(Some)
            .ok_or_else(|| "GitHub pagination overflowed.".into())
    }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn project(value: &Value, keys: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for key in keys {
        if let Some(value) = value.get(key) {
            result.insert((*key).into(), value.clone());
        }
    }
    Value::Object(result)
}

fn latest(rows: Vec<Value>, key: &str) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .filter(|row| seen.insert(row[key].to_string()))
        .collect()
}

fn assessment(conclusions: &[&str]) -> &'static str {
    if conclusions.is_empty() {
        return "unknown";
    }
    if conclusions.iter().any(|value| {
        matches!(
            *value,
            "failure"
                | "error"
                | "cancelled"
                | "timed_out"
                | "action_required"
                | "startup_failure"
                | "stale"
        )
    }) {
        return "failure";
    }
    if conclusions.iter().any(|value| {
        matches!(
            *value,
            "pending" | "queued" | "in_progress" | "waiting" | "requested"
        )
    }) {
        return "pending";
    }
    if conclusions.iter().all(|value| *value == "success") {
        "success"
    } else {
        "unknown"
    }
}

fn issue_record(row: &Value) -> Value {
    let mut issue = project(
        row,
        &[
            "number",
            "title",
            "body",
            "state",
            "html_url",
            "created_at",
            "updated_at",
            "closed_at",
            "labels",
            "assignees",
        ],
    );
    if let Some(body) = issue
        .get("body")
        .and_then(Value::as_str)
        .map(|body| truncate(body, ISSUE_BODY_CHARS))
    {
        issue["body"] = json!(body);
    }
    issue
}

fn bound_build(mut result: Value) -> Value {
    let budget = FILE_PAGE_CHARS as usize;
    if json_chars(&result) <= budget {
        return result;
    }
    let workflows = take_array(&result, "workflows");
    let checks = take_array(&result, "checks");
    let statuses = take_array(&result, "statuses");
    let mut cap = BUILD_RECORD_CAP.max(1);
    loop {
        apply_record_cap(&mut result, "workflows", &workflows, cap, ci_priority);
        apply_record_cap(&mut result, "checks", &checks, cap, ci_priority);
        apply_record_cap(&mut result, "statuses", &statuses, cap, ci_priority);
        if json_chars(&result) <= budget || cap == 1 {
            return result;
        }
        cap = (cap / 2).max(1);
    }
}

/// Trim the assessed page, and the citations derived from it, together so the
/// whole payload fits the context budget.
fn bound_readiness(mut result: Value, pulls: &[Value], observed_at: &str) -> Value {
    let budget = FILE_PAGE_CHARS as usize;
    let mut cap = pulls.len().max(1);
    loop {
        apply_record_cap(&mut result, "pull_requests", pulls, cap, pull_priority);
        let listed = take_array(&result, "pull_requests");
        result["citations"] = json!(
            listed
                .iter()
                .map(|row| readiness_citation(row, observed_at))
                .collect::<Vec<_>>()
        );
        if json_chars(&result) <= budget || cap == 1 {
            return result;
        }
        cap /= 2;
    }
}

/// Trim the assessed releases, and the citations derived from them, together
/// so the whole payload fits the context budget.
fn bound_releases(mut result: Value, pipelines: &[Value], observed_at: &str) -> Value {
    let budget = FILE_PAGE_CHARS as usize;
    let mut cap = pipelines.len().max(1);
    loop {
        apply_record_cap(&mut result, "releases", pipelines, cap, release_priority);
        let listed = take_array(&result, "releases");
        result["citations"] = json!(
            listed
                .iter()
                .map(|row| release_citation(row, observed_at))
                .collect::<Vec<_>>()
        );
        if json_chars(&result) <= budget || cap == 1 {
            return result;
        }
        cap /= 2;
    }
}

fn json_chars(value: &Value) -> usize {
    value.to_string().chars().count()
}

fn take_array(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn apply_record_cap(
    result: &mut Value,
    key: &str,
    rows: &[Value],
    cap: usize,
    priority: fn(&Value) -> u8,
) {
    let (capped, omitted) = cap_records(rows, cap, priority);
    result[key] = Value::Array(capped);
    let omitted_key = format!("{key}_omitted");
    if omitted > 0 {
        result[omitted_key] = json!(omitted);
    } else if let Some(object) = result.as_object_mut() {
        object.remove(&omitted_key);
    }
}

fn cap_records(rows: &[Value], cap: usize, priority: fn(&Value) -> u8) -> (Vec<Value>, usize) {
    let total = rows.len();
    if total <= cap {
        return (rows.to_vec(), 0);
    }
    let mut ranked: Vec<&Value> = rows.iter().collect();
    ranked.sort_by_key(|row| priority(row));
    (ranked.into_iter().take(cap).cloned().collect(), total - cap)
}

fn ci_token(row: &Value) -> &str {
    if row.get("status").is_some() {
        if row["status"].as_str() == Some("completed") {
            row["conclusion"].as_str().unwrap_or("unknown")
        } else {
            row["status"].as_str().unwrap_or("pending")
        }
    } else {
        row["state"].as_str().unwrap_or("unknown")
    }
}

fn ci_priority(row: &Value) -> u8 {
    match assessment(&[ci_token(row)]) {
        "failure" => 0,
        "pending" => 1,
        "unknown" => 2,
        _ => 3,
    }
}

fn pull_priority(row: &Value) -> u8 {
    u8::from(row["ready"] == true)
}

fn release_priority(row: &Value) -> u8 {
    u8::from(row["succeeded"] == true)
}

struct Review {
    draft: bool,
    head: Option<CommitSha>,
    threads: ThreadEvidence,
    comments: CommentEvidence,
}

fn review_comment(node: &Value) -> ReviewComment {
    ReviewComment {
        id: node["databaseId"].as_u64().unwrap_or_default(),
        author: text(&node["author"], "login"),
        body: text(node, "body"),
        url: text(node, "url"),
        created_at: text(node, "createdAt"),
    }
}

/// Count observed checks into buckets that add up by construction, so a state
/// that disagrees with them can only come from the provider.
fn tally<'a>(tokens: impl Iterator<Item = &'a str>) -> CheckEvidence {
    let mut evidence = CheckEvidence {
        complete: true,
        ..CheckEvidence::default()
    };
    for token in tokens {
        evidence.total += 1;
        match assessment(&[token]) {
            "failure" => evidence.failed += 1,
            "success" => evidence.passed += 1,
            "pending" => {
                evidence.running += 1;
                if matches!(token, "queued" | "requested" | "waiting") {
                    evidence.queued += 1;
                } else {
                    evidence.in_progress += 1;
                }
            }
            _ => evidence.unknown += 1,
        }
    }
    evidence.state = if evidence.total == 0 {
        "none"
    } else if evidence.failed > 0 {
        "failure"
    } else if evidence.running > 0 {
        "pending"
    } else if evidence.unknown > 0 {
        "unknown"
    } else {
        "success"
    }
    .to_string();
    evidence
}

/// The blast radius sits beside the blockers, never among them: how far a
/// change reaches is information a reviewer weighs, not a reason to withhold a
/// merge, so `ready` is built without it.
fn assessment_record(assessment: &readiness::Assessment, risk: &RiskSignal) -> Value {
    json!({
        "number": assessment.number,
        "title": assessment.title,
        "url": assessment.url,
        "updated_at": assessment.updated_at,
        "draft": assessment.draft,
        "head": assessment.head.as_ref().map(CommitSha::as_str),
        "ready": assessment.ready,
        "blockers": assessment.reports(),
        "blast_radius": {
            "tier": risk.blast_radius,
            "touched": risk.touched,
            "drivers": risk.drivers,
            "presumed": risk.presumed(),
            "summary": risk.to_string(),
        },
        "checks": assessment.checks,
        "reviews": assessment.reviews,
        "review_current": assessment.review_current,
        "unresolved_threads": assessment.unresolved,
        "threads_complete": assessment.threads_complete,
        "comments_complete": assessment.comments_complete,
    })
}

fn pipeline_record(pipeline: &ReleasePipeline) -> Value {
    json!({
        "tag": pipeline.release.tag,
        "release_id": pipeline.release.id,
        "url": pipeline.release.url,
        "published_at": pipeline.release.published_at,
        "state": pipeline.state(),
        "succeeded": pipeline.succeeded(),
        "complete": pipeline.complete(),
        "lookup": pipeline.lookup,
        "commit": pipeline.commit(),
        "blockers": pipeline.reports(),
        "runs": pipeline.runs,
        "checked_at": pipeline.checked_at,
    })
}

fn release_citation(record: &Value, observed_at: &str) -> Value {
    let complete = record["complete"] == true;
    let state = text(record, "state");
    let outcome = if record["succeeded"] == true {
        "success"
    } else if matches!(state.as_str(), "failed" | "timed-out" | "cancelled") {
        "failure"
    } else if matches!(state.as_str(), "running" | "queued") {
        "pending"
    } else if !complete {
        "incomplete"
    } else {
        "observed"
    };
    let blockers: Vec<String> = take_array(record, "blockers")
        .iter()
        .map(|blocker| text(blocker, "detail"))
        .collect();
    json!({
        "kind": "github_build",
        "title": format!("Release {} pipeline", text(record, "tag")),
        "url": text(record, "url"),
        "revision": record["commit"],
        "observed_at": observed_at,
        "complete": complete,
        "outcome": outcome,
        "note": if blockers.is_empty() {
            RELEASE_ASSESSMENT.to_string()
        } else {
            truncate(&blockers.join("; "), ISSUE_BODY_CHARS)
        },
    })
}

fn readiness_citation(record: &Value, observed_at: &str) -> Value {
    let ready = record["ready"] == true;
    let complete = record["checks"]["complete"] == true
        && record["threads_complete"] == true
        && record["comments_complete"] == true;
    let state = text(&record["checks"], "state");
    let outcome = if ready {
        "success"
    } else if state == "failure" {
        "failure"
    } else if state == "pending" {
        "pending"
    } else if !complete {
        "incomplete"
    } else {
        "observed"
    };
    let blockers: Vec<String> = take_array(record, "blockers")
        .iter()
        .map(|blocker| text(blocker, "detail"))
        .collect();
    json!({
        "kind": "github_issue",
        "title": format!("#{} {}", record["number"], text(record, "title")).trim().to_string(),
        "url": text(record, "url"),
        "revision": record["head"],
        "observed_at": observed_at,
        "complete": complete,
        "outcome": outcome,
        "note": if blockers.is_empty() {
            READINESS_ASSESSMENT.to_string()
        } else {
            truncate(&blockers.join("; "), ISSUE_BODY_CHARS)
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zone_core::tools::REASON_DESCRIPTION;

    const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const BLOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const WHY: &str = "The user asked for this to be opened.";

    fn tool(operation: Operation) -> Integration {
        Integration {
            scope: WorkspaceScope {
                state: AppState::for_tests(),
                workspace_id: Uuid::new_v4(),
                chat_id: None,
                user_id: Uuid::new_v4(),
            },
            operation,
        }
    }

    fn github(server: &MockServer) -> Github {
        let mut github = Github::new(Configuration {
            owner: "owner".into(),
            repo: "repository".into(),
            branch: Some("main".into()),
            path: None,
            token: Some("test-secret".into()),
            review_signals: None,
        })
        .unwrap();
        github.origin = Url::parse(&format!("{}/", server.uri())).unwrap();
        github
    }

    async fn mock(server: &MockServer, endpoint: &str, response: Value) {
        Mock::given(method("GET"))
            .and(path(format!("/repos/owner/repository/{endpoint}")))
            .and(header("Authorization", "Bearer test-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn writes_ask_for_a_reason_the_arguments_accept() {
        let source = Uuid::new_v4();
        for (operation, mut call) in [
            (
                Operation::CreatePull,
                json!({"source_id": source, "title": "Fix", "head": "feature"}),
            ),
            (
                Operation::Comment,
                json!({"source_id": source, "number": 7, "body": "ship it"}),
            ),
        ] {
            let schema = tool(operation).parameters_schema();
            assert_eq!(
                schema["properties"][REASON_PARAM]["description"],
                REASON_DESCRIPTION
            );
            assert!(
                schema["required"]
                    .as_array()
                    .expect("required is a list")
                    .contains(&json!(REASON_PARAM)),
                "reason must be advertised as required: {schema}"
            );
            call[REASON_PARAM] = json!(WHY);
            let given: Arguments = serde_json::from_value(call.clone())
                .expect("deny_unknown_fields must accept every property the schema advertises");
            assert_eq!(given.reason.as_deref(), Some(WHY));

            call.as_object_mut()
                .expect("a call is an object")
                .remove(REASON_PARAM);
            let omitted: Arguments =
                serde_json::from_value(call).expect("a missing reason must never fail the call");
            assert!(omitted.reason.is_none(), "an absent reason stays absent");
        }
    }

    #[tokio::test]
    async fn only_writes_ask_for_a_reason() {
        for operation in [
            Operation::Build,
            Operation::Deployments,
            Operation::Issues,
            Operation::File,
            Operation::PullRequests,
            Operation::ReleasePipelines,
            Operation::CheckLogs,
        ] {
            let tool = tool(operation);
            let schema = tool.parameters_schema();
            assert!(
                schema["properties"].get(REASON_PARAM).is_none(),
                "{} must not ask for a reason: {schema}",
                tool.name()
            );
        }
    }

    #[test]
    fn absent_neutral_unknown_and_pending_results_are_not_green() {
        for conclusions in [
            vec![],
            vec!["neutral"],
            vec!["skipped"],
            vec!["unknown"],
            vec!["success", "unexpected"],
        ] {
            assert_eq!(assessment(&conclusions), "unknown");
        }
        assert_eq!(assessment(&["success"]), "success");
        assert_eq!(assessment(&["success", "pending"]), "pending");
        assert_eq!(assessment(&["pending", "failure"]), "failure");
        assert_eq!(assessment(&["cancelled"]), "failure");
    }

    #[test]
    fn source_repository_and_paths_cannot_escape_scope() {
        for value in ["", "..", ".", "owner/repository", "//evil.example", "%2f"] {
            assert!(!segment(value));
        }
        for value in [
            "../secret",
            "/secret",
            "docs/../secret",
            "docs//file",
            "docs\\file",
        ] {
            assert!(validate_path(value, None).is_err());
        }
        assert!(validate_path("docs-other/file", Some("docs")).is_err());
        assert!(validate_path("docs/file", Some("docs")).is_ok());
        assert!(validate_path("docs/file", Some("/docs/")).is_ok());
    }

    #[tokio::test]
    async fn resolves_ref_once_and_observes_exact_commit() {
        let server = MockServer::start().await;
        mock(&server, "commits/main", json!({"sha": COMMIT})).await;
        Mock::given(path("/repos/owner/repository/actions/runs")).and(query_param("head_sha", COMMIT))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"workflow_runs": [{"workflow_id": 1,"head_sha": COMMIT,"status": "completed","conclusion":"success"}], "total_count":1}))).expect(1).mount(&server).await;
        mock(&server, &format!("commits/{COMMIT}/check-runs"), json!({"check_runs": [{"head_sha": COMMIT,"status":"completed","conclusion":"success"}]})).await;
        mock(&server, &format!("commits/{COMMIT}/statuses"), json!([])).await;
        let result = github(&server)
            .observe(
                Operation::Build,
                &Arguments {
                    source_id: Uuid::new_v4(),
                    reference: None,
                    path: None,
                    page: None,
                    offset: None,
                    limit: None,
                    state: None,
                    title: None,
                    head: None,
                    base: None,
                    body: None,
                    number: None,
                    job_id: None,
                    tag: None,
                    reason: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(result["sha"], COMMIT);
        assert_eq!(result["ref"], "main");
        assert_eq!(result["state"], "success");
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.url.path().ends_with("/commits/main"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn check_pagination_cannot_hide_a_failure() {
        let server = MockServer::start().await;
        mock(
            &server,
            "actions/runs",
            json!({"workflow_runs": [], "total_count":0}),
        )
        .await;
        let checks: Vec<Value> = (0..100)
            .map(
                |id| json!({"id":id,"head_sha":COMMIT,"status":"completed","conclusion":"success"}),
            )
            .collect();
        Mock::given(path(format!(
            "/repos/owner/repository/commits/{COMMIT}/check-runs"
        )))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"check_runs":checks})))
        .mount(&server)
        .await;
        Mock::given(path(format!(
            "/repos/owner/repository/commits/{COMMIT}/check-runs"
        )))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"check_runs":[{"head_sha":COMMIT,"status":"completed","conclusion":"failure"}]}),
        ))
        .mount(&server)
        .await;
        mock(&server, &format!("commits/{COMMIT}/statuses"), json!([])).await;
        let result = github(&server).build(COMMIT).await.unwrap();
        let checks = result["checks"].as_array().unwrap();
        assert_eq!(result["state"], "failure");
        assert!(
            checks.iter().any(|check| check["conclusion"] == "failure"),
            "truncated build history must still include the failing check"
        );
        assert!(checks.len() < 101);
        assert_eq!(result["checks_omitted"], 101 - checks.len());
        assert!(json_chars(&result) <= FILE_PAGE_CHARS as usize);
    }

    #[tokio::test]
    async fn mismatched_sha_and_provider_limit_fail_closed() {
        let server = MockServer::start().await;
        mock(
            &server,
            "actions/runs",
            json!({"workflow_runs": [],"total_count":1001}),
        )
        .await;
        assert!(
            github(&server)
                .build(COMMIT)
                .await
                .unwrap_err()
                .contains("1,000")
        );
        server.reset().await;
        mock(
            &server,
            "actions/runs",
            json!({"workflow_runs": [],"total_count":0}),
        )
        .await;
        mock(
            &server,
            &format!("commits/{COMMIT}/check-runs"),
            json!({"check_runs":[{"head_sha":BLOB,"status":"completed","conclusion":"success"}]}),
        )
        .await;
        mock(&server, &format!("commits/{COMMIT}/statuses"), json!([])).await;
        assert!(
            github(&server)
                .build(COMMIT)
                .await
                .unwrap_err()
                .contains("different")
        );
    }

    #[tokio::test]
    async fn empty_build_is_unknown_and_latest_status_replaces_old_failure() {
        let server = MockServer::start().await;
        mock(
            &server,
            "actions/runs",
            json!({"workflow_runs":[],"total_count":0}),
        )
        .await;
        mock(
            &server,
            &format!("commits/{COMMIT}/check-runs"),
            json!({"check_runs":[]}),
        )
        .await;
        mock(&server, &format!("commits/{COMMIT}/statuses"), json!([])).await;
        let empty = github(&server).build(COMMIT).await.unwrap();
        assert_eq!(empty["state"], "unknown");
        let citation = crate::agent::citations::from_tool_at(
            "get_build_status",
            &json!({
                "repository": "https://github.com/owner/repository",
                "sha": COMMIT,
                "state": empty["state"],
                "complete": empty["complete"],
                "assessment": empty["assessment"],
                "observed_at": "2026-09-05T00:00:00+00:00"
            })
            .to_string(),
            "2026-09-05T00:00:00+00:00",
        )
        .remove(0);
        assert!(!citation.passing());
        assert_eq!(citation.outcome, crate::agent::CitationOutcome::Incomplete);
        assert_eq!(
            latest(
                vec![
                    json!({"context":"test","state":"success"}),
                    json!({"context":"test","state":"failure"})
                ],
                "context"
            )
            .len(),
            1
        );
    }

    #[tokio::test]
    async fn provider_errors_never_echo_secret_or_follow_redirects() {
        let server = MockServer::start().await;
        let external = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", external.uri())
                    .set_body_string("test-secret"),
            )
            .mount(&server)
            .await;
        let error = github(&server).get(&[], &[]).await.unwrap_err();
        assert!(error.contains("302"));
        assert!(!error.contains("test-secret"));
        assert!(external.received_requests().await.unwrap().is_empty());
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("test-secret"))
            .mount(&server)
            .await;
        assert!(
            !github(&server)
                .get(&[], &[])
                .await
                .unwrap_err()
                .contains("test-secret")
        );
    }

    #[test]
    fn text_page_caps_and_continues_by_character() {
        let content = "α".repeat(10);
        let (page, total, next) = text_page(&content, 0, 4).unwrap();
        assert_eq!(page, "αααα");
        assert_eq!(total, 10);
        assert_eq!(next, Some(4));
        let (rest, _, next) = text_page(&content, 4, FILE_PAGE_CHARS).unwrap();
        assert_eq!(rest, "α".repeat(6));
        assert_eq!(next, None);
        assert!(text_page(&content, 0, 0).is_err());
        assert!(text_page(&content, 11, 1).is_err());
    }

    #[tokio::test]
    async fn file_is_complete_and_symlinks_are_rejected() {
        let server = MockServer::start().await;
        let content = "Short file 🦀\n";
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"100644","sha":BLOB,"size":content.len()}]})).await;
        Mock::given(path(format!("/repos/owner/repository/git/blobs/{BLOB}")))
            .and(header("Accept", "application/vnd.github.raw+json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(content))
            .mount(&server)
            .await;
        let result = github(&server)
            .file(COMMIT, "README.md", 0, FILE_PAGE_CHARS)
            .await
            .unwrap();
        assert_eq!(result["content"], content);
        assert_eq!(result["blob_sha"], BLOB);
        assert_eq!(result["complete"], true);
        assert_eq!(result["next"], Value::Null);
        server.reset().await;
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"120000","sha":BLOB,"size":9}]})).await;
        assert!(
            github(&server)
                .file(COMMIT, "README.md", 0, FILE_PAGE_CHARS)
                .await
                .unwrap_err()
                .contains("symlinks")
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn large_file_returns_a_budgeted_page_instead_of_the_whole_blob() {
        let server = MockServer::start().await;
        let content = "Long document 🦀\n".repeat(2000);
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"100644","sha":BLOB,"size":content.len()}]})).await;
        Mock::given(path(format!("/repos/owner/repository/git/blobs/{BLOB}")))
            .and(header("Accept", "application/vnd.github.raw+json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(content.clone()))
            .mount(&server)
            .await;
        let result = github(&server)
            .file(COMMIT, "README.md", 0, u64::MAX)
            .await
            .unwrap();
        let page = result["content"].as_str().unwrap();
        assert_eq!(page.chars().count() as u64, FILE_PAGE_CHARS);
        assert_eq!(result["complete"], false);
        assert_eq!(result["next"], FILE_PAGE_CHARS);
        assert_eq!(result["offset"], 0);
        assert_eq!(result["total"], content.chars().count() as u64);
        assert!(page.chars().count() < content.chars().count());
        let continued = github(&server)
            .file(COMMIT, "README.md", FILE_PAGE_CHARS, FILE_PAGE_CHARS)
            .await
            .unwrap();
        assert_eq!(continued["offset"], FILE_PAGE_CHARS);
        assert_eq!(
            continued["content"].as_str().unwrap(),
            &content
                .chars()
                .skip(FILE_PAGE_CHARS as usize)
                .take(FILE_PAGE_CHARS as usize)
                .collect::<String>()
        );
    }

    #[tokio::test]
    async fn deployments_include_latest_provider_status() {
        let server = MockServer::start().await;
        mock(
            &server,
            "deployments",
            json!([{"id":7,"sha":COMMIT,"environment":"production"}]),
        )
        .await;
        Mock::given(path("/repos/owner/repository/deployments/7/statuses"))
            .and(query_param("per_page", "1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!([{"state":"pending","created_at":"2026-09-05T00:00:00Z"}]),
                ),
            )
            .mount(&server)
            .await;
        let result = github(&server).deployments(COMMIT, 1).await.unwrap();
        assert_eq!(result["deployments"][0]["status"]["state"], "pending");
        assert_eq!(result["next_page"], Value::Null);
    }

    #[tokio::test]
    async fn issues_keep_full_body_exclude_pulls_and_report_provider_page() {
        let server = MockServer::start().await;
        let mut issues = vec![json!({"number":1,"pull_request":{}}); 98];
        let body = "full issue ".repeat(2000);
        issues.push(json!({"number":99,"title":"Tiny","body":"ok"}));
        issues.push(json!({"number":100,"title":"Bug","body":body}));
        mock(&server, "issues", json!(issues)).await;
        let result = github(&server)
            .observe(
                Operation::Issues,
                &Arguments {
                    source_id: Uuid::new_v4(),
                    reference: None,
                    path: None,
                    page: Some(1),
                    offset: None,
                    limit: None,
                    state: Some("all".into()),
                    title: None,
                    head: None,
                    base: None,
                    body: None,
                    number: None,
                    job_id: None,
                    tag: None,
                    reason: None,
                },
            )
            .await
            .unwrap();
        let listed = result["issues"].as_array().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0]["number"], 99);
        assert_eq!(listed[0]["title"], "Tiny");
        assert_eq!(listed[0]["body"], "ok");
        assert_eq!(listed[1]["number"], 100);
        assert_eq!(listed[1]["title"], "Bug");
        let snippet = listed[1]["body"].as_str().unwrap();
        assert_ne!(snippet, body);
        assert!(snippet.starts_with("full issue "));
        assert!(snippet.ends_with('…'));
        assert!(snippet.chars().count() <= ISSUE_BODY_CHARS + 1);
        assert_eq!(result["next_page"], 2);
    }

    #[test]
    fn issue_body_truncates_long_text_and_keeps_short_text() {
        let long = issue_record(&json!({
            "number": 7,
            "title": "Bug",
            "body": "x".repeat(2_000),
            "state": "open",
            "html_url": "https://github.com/owner/repository/issues/7"
        }));
        assert_eq!(long["number"], 7);
        assert_eq!(long["title"], "Bug");
        assert_eq!(long["state"], "open");
        assert_eq!(
            long["html_url"],
            "https://github.com/owner/repository/issues/7"
        );
        let body = long["body"].as_str().unwrap();
        assert!(body.ends_with('…'));
        assert_eq!(body.chars().count(), ISSUE_BODY_CHARS + 1);
        assert_eq!(
            issue_record(&json!({"number": 1, "title": "Tiny", "body": "ok"}))["body"],
            "ok"
        );
        assert_eq!(
            issue_record(&json!({"number": 2, "body": "a".repeat(ISSUE_BODY_CHARS)}))["body"],
            "a".repeat(ISSUE_BODY_CHARS)
        );
    }

    #[test]
    fn bound_build_keeps_failures_under_the_character_budget() {
        let mut checks: Vec<Value> = (0..80)
            .map(|id| {
                json!({
                    "id": id,
                    "name": format!("check-{id}"),
                    "head_sha": COMMIT,
                    "status": "completed",
                    "conclusion": "success",
                    "html_url": format!("https://github.com/owner/repository/runs/{id}")
                })
            })
            .collect();
        checks.push(json!({
            "id": 99,
            "name": "boom",
            "head_sha": COMMIT,
            "status": "completed",
            "conclusion": "failure",
            "html_url": "https://github.com/owner/repository/runs/99"
        }));
        let result = bound_build(json!({
            "state": "failure",
            "complete": true,
            "assessment": "Observed CI only; required branch checks and service health are not evaluated.",
            "workflows": [],
            "checks": checks,
            "statuses": []
        }));
        let listed = result["checks"].as_array().unwrap();
        assert_eq!(result["state"], "failure");
        assert!(listed.iter().any(|check| check["conclusion"] == "failure"));
        assert!(listed.len() < 81);
        assert_eq!(result["checks_omitted"], 81 - listed.len());
        assert!(json_chars(&result) <= FILE_PAGE_CHARS as usize);
        let small = bound_build(json!({
            "state": "success",
            "complete": true,
            "assessment": "Observed CI only; required branch checks and service health are not evaluated.",
            "workflows": [json!({"id": 1, "status": "completed", "conclusion": "success"})],
            "checks": [],
            "statuses": []
        }));
        assert_eq!(small["workflows"].as_array().unwrap().len(), 1);
        assert!(small.get("workflows_omitted").is_none());
        assert!(small.get("checks_omitted").is_none());
    }

    #[tokio::test]
    async fn create_pull_request_posts_title_head_and_base() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repository/pulls"))
            .and(header("Authorization", "Bearer test-secret"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "number": 12,
                "html_url": "https://github.com/owner/repository/pull/12",
                "title": "Fix",
                "state": "open"
            })))
            .mount(&server)
            .await;
        let result = github(&server)
            .write(
                Operation::CreatePull,
                &Arguments {
                    source_id: Uuid::new_v4(),
                    reference: None,
                    path: None,
                    page: None,
                    offset: None,
                    limit: None,
                    state: None,
                    title: Some("Fix".into()),
                    head: Some("feature".into()),
                    base: None,
                    body: Some("details".into()),
                    number: None,
                    job_id: None,
                    tag: None,
                    reason: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(result["pull_request"]["number"], 12);
        assert_eq!(
            result["pull_request"]["html_url"],
            "https://github.com/owner/repository/pull/12"
        );
    }

    #[tokio::test]
    async fn comment_on_issue_posts_markdown() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repository/issues/7/comments"))
            .and(header("Authorization", "Bearer test-secret"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "id": 99,
                "html_url": "https://github.com/owner/repository/issues/7#issuecomment-99",
                "body": "ship it",
                "created_at": "2026-09-05T00:00:00Z"
            })))
            .mount(&server)
            .await;
        let result = github(&server)
            .write(
                Operation::Comment,
                &Arguments {
                    source_id: Uuid::new_v4(),
                    reference: None,
                    path: None,
                    page: None,
                    offset: None,
                    limit: None,
                    state: None,
                    title: None,
                    head: None,
                    base: None,
                    body: Some("ship it".into()),
                    number: Some(7),
                    job_id: None,
                    tag: None,
                    reason: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(result["comment"]["id"], 99);
        assert_eq!(result["comment"]["body"], "ship it");
    }

    fn write_args(
        title: Option<&str>,
        head: Option<&str>,
        body: Option<&str>,
        number: Option<u64>,
    ) -> Arguments {
        Arguments {
            source_id: Uuid::new_v4(),
            reference: None,
            path: None,
            page: None,
            offset: None,
            limit: None,
            state: None,
            title: title.map(str::to_string),
            head: head.map(str::to_string),
            base: None,
            body: body.map(str::to_string),
            number,
            job_id: None,
            tag: None,
            reason: None,
        }
    }

    #[tokio::test]
    async fn create_pull_rejects_empty_title_and_invalid_head() {
        let server = MockServer::start().await;
        let github = github(&server);
        assert!(
            github
                .write(
                    Operation::CreatePull,
                    &write_args(None, Some("feature"), None, None)
                )
                .await
                .unwrap_err()
                .contains("title")
        );
        assert!(
            github
                .write(
                    Operation::CreatePull,
                    &write_args(Some("Fix"), Some("has space"), None, None)
                )
                .await
                .unwrap_err()
                .contains("head")
        );
        assert!(
            github
                .write(
                    Operation::CreatePull,
                    &write_args(Some("Fix"), Some(".."), None, None)
                )
                .await
                .unwrap_err()
                .contains("head")
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn comment_rejects_zero_and_empty_body() {
        let server = MockServer::start().await;
        let github = github(&server);
        assert!(
            github
                .write(
                    Operation::Comment,
                    &write_args(None, None, Some("hi"), Some(0))
                )
                .await
                .unwrap_err()
                .contains("positive")
        );
        assert!(
            github
                .write(
                    Operation::Comment,
                    &write_args(None, None, Some("  "), Some(7))
                )
                .await
                .unwrap_err()
                .contains("body")
        );
        assert!(
            github
                .write(Operation::Build, &write_args(None, None, None, None))
                .await
                .unwrap_err()
                .contains("read-only")
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }

    async fn mock_graphql(server: &MockServer, response: Value) {
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(header("Authorization", "Bearer test-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(server)
            .await;
    }

    fn summary(score: u8, sha: &str) -> String {
        format!("**Confidence Score:** {score}/5\n_Last reviewed commit: {sha}_")
    }

    fn review_payload(head: &str, resolved: bool, body: &str, more_threads: bool) -> Value {
        json!({"data": {"repository": {"pullRequest": {
            "number": 7,
            "isDraft": false,
            "headRefOid": head,
            "reviewThreads": {
                "pageInfo": {"hasNextPage": more_threads},
                "nodes": [{"isResolved": resolved}]
            },
            "comments": {
                "pageInfo": {"hasPreviousPage": false},
                "nodes": [{
                    "databaseId": 11,
                    "body": body,
                    "url": "https://github.com/owner/repository/pull/7#issuecomment-11",
                    "createdAt": "2026-09-05T00:00:00Z",
                    "author": {"login": "greptile-apps"}
                }]
            }
        }}}})
    }

    fn readiness_args(number: Option<u64>) -> Arguments {
        Arguments {
            source_id: Uuid::new_v4(),
            reference: None,
            path: None,
            page: None,
            offset: None,
            limit: None,
            state: None,
            title: None,
            head: None,
            base: None,
            body: None,
            number,
            job_id: None,
            tag: None,
            reason: None,
        }
    }

    async fn mock_pull(server: &MockServer, head: &str) {
        mock(
            server,
            "pulls/7",
            json!({"number": 7, "draft": false, "title": "Ship the billing export",
                   "html_url": "https://github.com/owner/repository/pull/7",
                   "updated_at": "2026-09-05T00:00:00Z", "head": {"sha": head}}),
        )
        .await;
    }

    async fn mock_green_ci(server: &MockServer) {
        mock(
            server,
            "actions/runs",
            json!({"workflow_runs": [{"workflow_id": 1, "head_sha": COMMIT, "status": "completed", "conclusion": "success"}], "total_count": 1}),
        )
        .await;
        mock(
            server,
            &format!("commits/{COMMIT}/check-runs"),
            json!({"check_runs": [{"head_sha": COMMIT, "status": "completed", "conclusion": "success"}]}),
        )
        .await;
        mock(server, &format!("commits/{COMMIT}/statuses"), json!([])).await;
    }

    #[tokio::test]
    async fn coherent_green_evidence_is_ready_and_cites_the_head_commit() {
        let server = MockServer::start().await;
        mock_pull(&server, COMMIT).await;
        mock_graphql(
            &server,
            review_payload(COMMIT, true, &summary(5, COMMIT), false),
        )
        .await;
        mock_green_ci(&server).await;
        let result = github(&server)
            .observe(Operation::PullRequests, &readiness_args(Some(7)))
            .await
            .unwrap();
        let pull = &result["pull_requests"][0];
        assert_eq!(pull["ready"], true);
        assert_eq!(pull["blockers"].as_array().unwrap().len(), 0);
        assert_eq!(pull["head"], COMMIT);
        assert_eq!(pull["checks"]["state"], "success");
        assert_eq!(pull["checks"]["counts"]["total"], 2);
        assert_eq!(pull["reviews"][0]["confidence"]["score"], 5);
        assert_eq!(pull["review_current"], true);
        assert_eq!(result["ready"], 1);
        assert_eq!(result["assessed"], 1);
        assert_eq!(result["next_page"], Value::Null);
        assert!(json_chars(&result) <= FILE_PAGE_CHARS as usize);
        let citation = crate::agent::citations::from_tool_at(
            "assess_pull_requests",
            &result.to_string(),
            "2026-09-05T00:00:00+00:00",
        )
        .remove(0);
        assert!(citation.passing());
        assert_eq!(citation.revision.as_deref(), Some(COMMIT));
        assert_eq!(citation.url, "https://github.com/owner/repository/pull/7");
    }

    #[tokio::test]
    async fn a_head_commit_the_observations_disagree_on_is_never_ready() {
        let server = MockServer::start().await;
        mock_pull(&server, COMMIT).await;
        mock_graphql(
            &server,
            review_payload(BLOB, true, &summary(5, BLOB), false),
        )
        .await;
        mock_green_ci(&server).await;
        let result = github(&server)
            .observe(Operation::PullRequests, &readiness_args(Some(7)))
            .await
            .unwrap();
        let pull = &result["pull_requests"][0];
        assert_eq!(pull["ready"], false);
        assert_eq!(pull["head"], Value::Null);
        assert_eq!(pull["blockers"][0]["code"], "head_commit_unverified");
        assert_eq!(result["ready"], 0);
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| !request.url.path().contains("check-runs")),
            "an unverified head commit must not be used to fetch checks"
        );
        let citation = crate::agent::citations::from_tool_at(
            "assess_pull_requests",
            &result.to_string(),
            "2026-09-05T00:00:00+00:00",
        )
        .remove(0);
        assert!(!citation.passing());
        assert_eq!(citation.outcome, crate::agent::CitationOutcome::Incomplete);
    }

    #[tokio::test]
    async fn threads_that_could_not_be_paginated_block_otherwise_green_evidence() {
        let server = MockServer::start().await;
        mock_pull(&server, COMMIT).await;
        mock_graphql(
            &server,
            review_payload(COMMIT, true, &summary(5, COMMIT), true),
        )
        .await;
        mock_green_ci(&server).await;
        let result = github(&server)
            .observe(Operation::PullRequests, &readiness_args(Some(7)))
            .await
            .unwrap();
        let pull = &result["pull_requests"][0];
        assert_eq!(pull["ready"], false);
        assert_eq!(pull["threads_complete"], false);
        assert_eq!(pull["checks"]["state"], "success");
        assert_eq!(pull["blockers"][0]["code"], "threads_incomplete");
        assert_eq!(
            pull["blockers"][0]["detail"],
            "Review threads could not be fully checked"
        );
    }

    #[tokio::test]
    async fn review_query_failures_and_mismatched_pull_requests_fail_closed() {
        let server = MockServer::start().await;
        mock_pull(&server, COMMIT).await;
        mock_graphql(
            &server,
            json!({"data": Value::Null, "errors": [{"message": "test-secret"}]}),
        )
        .await;
        let error = github(&server)
            .observe(Operation::PullRequests, &readiness_args(Some(7)))
            .await
            .unwrap_err();
        assert!(error.contains("rejected the review query"));
        assert!(!error.contains("test-secret"));
        server.reset().await;
        mock_pull(&server, COMMIT).await;
        let mut payload = review_payload(COMMIT, true, &summary(5, COMMIT), false);
        payload["data"]["repository"]["pullRequest"]["number"] = json!(8);
        mock_graphql(&server, payload).await;
        assert!(
            github(&server)
                .observe(Operation::PullRequests, &readiness_args(Some(7)))
                .await
                .unwrap_err()
                .contains("different or missing pull request")
        );
    }

    #[tokio::test]
    async fn pull_requests_without_a_number_or_draft_state_fail_closed() {
        let server = MockServer::start().await;
        mock(
            &server,
            "pulls/7",
            json!({"number": 7, "head": {"sha": COMMIT}}),
        )
        .await;
        assert!(
            github(&server)
                .observe(Operation::PullRequests, &readiness_args(Some(7)))
                .await
                .unwrap_err()
                .contains("draft state")
        );
        server.reset().await;
        mock(&server, "pulls/7", json!({"draft": false})).await;
        assert!(
            github(&server)
                .observe(Operation::PullRequests, &readiness_args(Some(7)))
                .await
                .unwrap_err()
                .contains("without a number")
        );
        assert!(
            github(&server)
                .observe(Operation::PullRequests, &readiness_args(Some(0)))
                .await
                .unwrap_err()
                .contains("positive")
        );
    }

    #[tokio::test]
    async fn a_listed_page_requests_the_most_recently_updated_pull_requests() {
        let server = MockServer::start().await;
        Mock::given(path("/repos/owner/repository/pulls"))
            .and(query_param("state", "open"))
            .and(query_param("sort", "updated"))
            .and(query_param("direction", "desc"))
            .and(query_param("per_page", "10"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
                "number": 7, "draft": true, "title": "Ship the billing export",
                "html_url": "https://github.com/owner/repository/pull/7",
                "updated_at": "2026-09-05T00:00:00Z", "head": {"sha": COMMIT}
            }])))
            .expect(1)
            .mount(&server)
            .await;
        mock_graphql(
            &server,
            review_payload(COMMIT, true, &summary(5, COMMIT), false),
        )
        .await;
        mock_green_ci(&server).await;
        let result = github(&server)
            .observe(Operation::PullRequests, &readiness_args(None))
            .await
            .unwrap();
        assert_eq!(result["assessed"], 1);
        assert_eq!(result["next_page"], Value::Null);
        assert_eq!(result["complete"], true);
        assert_eq!(result["pull_requests"][0]["ready"], false);
        assert_eq!(result["pull_requests"][0]["blockers"][0]["code"], "draft");
        assert_eq!(
            result["repository"], "https://github.com/owner/repository",
            "the citation-bearing shape keeps the repository the other operations return"
        );
    }

    #[tokio::test]
    async fn a_listed_page_rejects_an_unknown_state() {
        let server = MockServer::start().await;
        let mut arguments = readiness_args(None);
        arguments.state = Some("merged".into());
        assert!(
            github(&server)
                .observe(Operation::PullRequests, &arguments)
                .await
                .unwrap_err()
                .contains("open, closed or all")
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }

    #[test]
    fn tallied_counts_always_add_up_and_split_running_checks() {
        let evidence = tally(
            [
                "success",
                "failure",
                "queued",
                "in_progress",
                "neutral",
                "success",
            ]
            .into_iter(),
        );
        assert_eq!(evidence.total, 6);
        assert_eq!(evidence.passed, 2);
        assert_eq!(evidence.failed, 1);
        assert_eq!(evidence.running, 2);
        assert_eq!(evidence.queued, 1);
        assert_eq!(evidence.in_progress, 1);
        assert_eq!(evidence.unknown, 1);
        let summary = evidence.normalize();
        assert!(summary.complete, "a tally must survive its own normalizer");
        assert_eq!(summary.state, readiness::CheckState::Failure);
        let absent = tally(std::iter::empty()).normalize();
        assert_eq!(absent.state, readiness::CheckState::None);
        assert_eq!(absent.counts.total, 0);
        let green = tally(["success"].into_iter()).normalize();
        assert_eq!(green.state, readiness::CheckState::Success);
        let skipped = tally(["success", "skipped"].into_iter()).normalize();
        assert_eq!(
            skipped.state,
            readiness::CheckState::Unknown,
            "results outside the known set are not green"
        );
    }

    #[test]
    fn ci_tokens_come_from_the_record_that_carries_them() {
        assert_eq!(
            ci_token(&json!({"status": "completed", "conclusion": "failure"})),
            "failure"
        );
        assert_eq!(ci_token(&json!({"status": "queued"})), "queued");
        assert_eq!(ci_token(&json!({"status": "in_progress"})), "in_progress");
        assert_eq!(
            ci_token(&json!({"context": "lint", "state": "pending"})),
            "pending"
        );
        assert_eq!(ci_token(&json!({"context": "lint"})), "unknown");
        assert_eq!(ci_priority(&json!({"status": "queued"})), 1);
        assert_eq!(
            ci_priority(&json!({"status": "completed", "conclusion": "failure"})),
            0
        );
    }

    #[test]
    fn short_pages_end_pagination_and_full_pages_continue() {
        assert_eq!(next_page_of(9, 1, PULL_PAGE_SIZE).unwrap(), None);
        assert_eq!(
            next_page_of(PULL_PAGE_SIZE, 1, PULL_PAGE_SIZE).unwrap(),
            Some(2)
        );
        assert!(next_page_of(PULL_PAGE_SIZE, u32::MAX, PULL_PAGE_SIZE).is_err());
        assert_eq!(next_page(PAGE_SIZE - 1, 1).unwrap(), None);
        assert_eq!(next_page(PAGE_SIZE, 1).unwrap(), Some(2));
    }

    #[test]
    fn a_long_page_is_trimmed_with_its_citations_and_keeps_blocked_pull_requests() {
        let mut pulls: Vec<Value> = (0..40)
            .map(|number| {
                json!({
                    "number": number,
                    "ready": true,
                    "title": "Ready pull request with a title long enough to cost context ".repeat(4),
                    "url": format!("https://github.com/owner/repository/pull/{number}"),
                    "head": COMMIT,
                    "checks": {"state": "success", "complete": true},
                    "threads_complete": true,
                    "comments_complete": true,
                    "blockers": []
                })
            })
            .collect();
        pulls.push(json!({
            "number": 99,
            "ready": false,
            "title": "Blocked",
            "url": "https://github.com/owner/repository/pull/99",
            "head": BLOB,
            "checks": {"state": "failure", "complete": true},
            "threads_complete": true,
            "comments_complete": true,
            "blockers": [{"code": "checks_failed", "detail": "1 of 2 checks failed"}]
        }));
        let result = bound_readiness(
            json!({"assessed": pulls.len(), "ready": 40, "assessment": READINESS_ASSESSMENT}),
            &pulls,
            "2026-09-05T00:00:00+00:00",
        );
        let listed = result["pull_requests"].as_array().unwrap();
        assert!(
            listed.iter().any(|pull| pull["number"] == 99),
            "a trimmed page must still carry the pull request that is not ready"
        );
        assert!(listed.len() < 41);
        assert_eq!(result["pull_requests_omitted"], 41 - listed.len());
        assert_eq!(result["citations"].as_array().unwrap().len(), listed.len());
        assert_eq!(
            result["ready"], 40,
            "trimming the page must not change how many were assessed as ready"
        );
        assert_eq!(result["assessed"], 41);
        assert!(json_chars(&result) <= FILE_PAGE_CHARS as usize);
    }

    #[tokio::test]
    #[ignore = "requires migrated PostgreSQL via TEST_DATABASE_URL"]
    async fn source_access_requires_active_membership_and_matching_workspace() {
        use crate::db::{organizations, users, workspaces};
        let pool = sqlx::PgPool::connect(
            &std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL required"),
        )
        .await
        .unwrap();
        let identifier = Uuid::new_v4();
        let organization = organizations::create_organization(
            &pool,
            "Integration test",
            &format!("integration-{identifier}"),
            None,
        )
        .await
        .unwrap();
        let workspace = workspaces::create_workspace(
            &pool,
            organization.id,
            "Allowed",
            &format!("allowed-{identifier}"),
            None,
        )
        .await
        .unwrap();
        let foreign = workspaces::create_workspace(
            &pool,
            organization.id,
            "Foreign",
            &format!("foreign-{identifier}"),
            None,
        )
        .await
        .unwrap();
        let user = users::create_user(
            &pool,
            &format!("integration-{identifier}@example.test"),
            "hash",
            None,
            false,
        )
        .await
        .unwrap();
        let source = sources::create_source(
            &pool,
            workspace.id,
            "Invalid configured source",
            "github",
            json!({}),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let other = sources::create_source(
            &pool,
            foreign.id,
            "Foreign source",
            "github",
            json!({}),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let tool = Integration {
            operation: Operation::Build,
            scope: WorkspaceScope {
                state: crate::state::AppState::new(crate::state::test_config(), pool.clone(), None),
                workspace_id: workspace.id,
                chat_id: Some(Uuid::new_v4()),
                user_id: user.id,
            },
        };
        let denied = tool.run(json!({"source_id":source.id})).await.unwrap_err();
        assert!(denied.contains("cannot read"));
        workspace_members::add_member(
            &pool,
            workspace.id,
            user.id,
            workspace_members::WorkspaceRole::Viewer,
            None,
        )
        .await
        .unwrap();
        let allowed = tool.run(json!({"source_id":source.id})).await.unwrap_err();
        assert!(allowed.contains("configuration is invalid"));
        let foreign_result = tool.run(json!({"source_id":other.id})).await.unwrap_err();
        assert!(foreign_result.contains("not found"));
        workspace_members::remove_member(&pool, workspace.id, user.id)
            .await
            .unwrap();
        let revoked = tool.run(json!({"source_id":source.id})).await.unwrap_err();
        assert!(revoked.contains("cannot read"));
        organizations::delete_organization(&pool, organization.id)
            .await
            .unwrap();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user.id)
            .execute(&pool)
            .await
            .unwrap();
    }
    const JOB_ID: u64 = 4242;

    const RUN_ID: u64 = 99;

    fn check_args(job_id: Option<u64>, number: Option<u64>, limit: Option<u64>) -> Arguments {
        Arguments {
            source_id: Uuid::new_v4(),
            reference: None,
            path: None,
            page: None,
            offset: None,
            limit,
            state: None,
            title: None,
            head: None,
            base: None,
            body: None,
            number,
            job_id,
            tag: None,
            reason: None,
        }
    }

    fn job(head_sha: &str) -> Value {
        json!({
            "id": JOB_ID,
            "run_id": RUN_ID,
            "name": "build",
            "status": "completed",
            "conclusion": "failure",
            "head_sha": head_sha,
            "started_at": "2026-09-05T00:00:00Z",
            "completed_at": "2026-09-05T00:01:00Z",
            "html_url": format!("https://github.com/owner/repository/actions/runs/{RUN_ID}/job/{JOB_ID}")
        })
    }

    async fn mock_pull_for_job(server: &MockServer, number: u64, head_sha: &str) {
        mock(
            server,
            &format!("pulls/{number}"),
            json!({"number": number, "title": "Fix", "state": "open", "draft": false,
                   "html_url": format!("https://github.com/owner/repository/pull/{number}"),
                   "head": {"sha": head_sha, "ref": "feature"}}),
        )
        .await;
    }

    async fn mock_log(server: &MockServer, body: String) {
        Mock::given(method("GET"))
            .and(path(format!(
                "/repos/owner/repository/actions/jobs/{JOB_ID}/logs"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(server)
            .await;
    }

    fn noisy_log(lines: usize, failure_at: usize) -> String {
        (0..lines)
            .map(|index| {
                let body = if index == failure_at {
                    "##[error]Process completed with exit code 1.".to_string()
                } else {
                    format!("Compiling crate number {index}")
                };
                format!("2026-09-05T00:00:0{}.1234567Z {body}", index % 10)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn a_job_from_another_pull_request_is_refused_before_its_log_is_fetched() {
        let server = MockServer::start().await;
        mock_pull_for_job(&server, 7, COMMIT).await;
        mock(&server, &format!("actions/jobs/{JOB_ID}"), job(BLOB)).await;
        mock_log(&server, "secret log".into()).await;
        let error = github(&server)
            .observe(
                Operation::CheckLogs,
                &check_args(Some(JOB_ID), Some(7), None),
            )
            .await
            .unwrap_err();
        assert!(
            error.contains("does not belong"),
            "a job on a different commit must be refused: {error}"
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| !request.url.path().ends_with("/logs")),
            "the log must not be fetched before the job is bound to the pull request"
        );
    }

    #[tokio::test]
    async fn job_identity_must_match_the_repository_and_the_requested_job() {
        let server = MockServer::start().await;
        let foreign_repository = json!({
            "id": JOB_ID, "run_id": RUN_ID, "head_sha": COMMIT, "status": "completed",
            "conclusion": "failure", "name": "build",
            "html_url": format!("https://github.com/attacker/private/actions/runs/{RUN_ID}/job/{JOB_ID}")
        });
        let renumbered = json!({
            "id": JOB_ID + 1, "run_id": RUN_ID, "head_sha": COMMIT, "status": "completed",
            "conclusion": "failure", "name": "build",
            "html_url": format!("https://github.com/owner/repository/actions/runs/{RUN_ID}/job/{JOB_ID}")
        });
        for metadata in [foreign_repository, renumbered, job(BLOB)] {
            assert!(
                github(&server)
                    .verify_job(&metadata, JOB_ID, COMMIT)
                    .unwrap_err()
                    .contains("does not belong")
            );
        }
        assert_eq!(
            github(&server).verify_job(&job(COMMIT), JOB_ID, COMMIT),
            Ok(RUN_ID)
        );
        assert!(
            github(&server)
                .verify_job(&json!({"id": JOB_ID, "head_sha": COMMIT}), JOB_ID, COMMIT)
                .unwrap_err()
                .contains("workflow run identity")
        );
        assert!(!is_job_url(
            &format!("https://github.com/owner/repository/actions/runs/{RUN_ID}/job/{JOB_ID}?x=1"),
            "owner",
            "repository",
            RUN_ID,
            JOB_ID
        ));
        assert!(!is_job_url(
            &format!(
                "https://github.com.evil.test/owner/repository/actions/runs/{RUN_ID}/job/{JOB_ID}"
            ),
            "owner",
            "repository",
            RUN_ID,
            JOB_ID
        ));
    }

    #[tokio::test]
    async fn an_oversized_log_is_capped_and_reported_rather_than_silently_cut() {
        let server = MockServer::start().await;
        mock(&server, "commits/main", json!({"sha": COMMIT})).await;
        mock(&server, &format!("actions/jobs/{JOB_ID}"), job(COMMIT)).await;
        mock_log(&server, noisy_log(400, 10)).await;
        let mut github = github(&server);
        github.log_bytes = 1_000;
        let result = github
            .observe(Operation::CheckLogs, &check_args(Some(JOB_ID), None, None))
            .await
            .unwrap();
        let log = &result["log"];
        assert_eq!(log["download_capped"], true);
        assert_eq!(log["downloaded_bytes"], 1_000);
        assert_eq!(log["complete"], false);
        assert!(
            log["excerpt"].as_str().is_some_and(|text| !text.is_empty()),
            "a capped download must still return usable evidence"
        );
        let citation = crate::agent::citations::from_tool_at(
            "read_check_logs",
            &result.to_string(),
            "2026-09-05T00:00:00+00:00",
        )
        .remove(0);
        assert!(
            !citation.complete && !citation.passing(),
            "a capped log is incomplete evidence and must never read as a pass"
        );
    }

    #[tokio::test]
    async fn the_excerpt_keeps_the_failure_region_and_the_end_of_a_long_log() {
        let server = MockServer::start().await;
        mock_pull_for_job(&server, 7, COMMIT).await;
        mock(&server, &format!("actions/jobs/{JOB_ID}"), job(COMMIT)).await;
        mock_log(&server, noisy_log(4_000, 900)).await;
        let result = github(&server)
            .observe(
                Operation::CheckLogs,
                &check_args(Some(JOB_ID), Some(7), None),
            )
            .await
            .unwrap();
        let log = &result["log"];
        let excerpt = log["excerpt"].as_str().unwrap();
        assert_eq!(log["total_lines"], 4_000);
        assert_eq!(log["first_error_line"], 901);
        assert_eq!(log["complete"], false);
        assert!(
            excerpt.contains("##[error]Process completed with exit code 1."),
            "the failure line must survive trimming"
        );
        assert!(
            excerpt.contains("Compiling crate number 899")
                && excerpt.contains("Compiling crate number 901"),
            "the lines around the failure must survive trimming"
        );
        assert!(
            excerpt.contains("Compiling crate number 3999"),
            "the end of the log must survive trimming"
        );
        assert!(
            excerpt.contains("lines omitted"),
            "dropped regions must be marked, not silently removed"
        );
        assert!(
            excerpt.chars().count() <= LOG_EXCERPT_CHARS as usize,
            "the excerpt must respect its character budget"
        );
        assert!(
            !excerpt.contains("2026-09-05T00:00:0"),
            "per-line timestamps are noise the budget should not pay for"
        );
        assert_eq!(result["sha"], COMMIT);
        assert_eq!(result["ref"], "feature");
        assert_eq!(result["pull_request"]["number"], 7);
        assert_eq!(result["job"]["conclusion"], "failure");
        assert!(json_chars(&result) <= MAX_TOOL_MESSAGE_CHARS);
    }

    #[tokio::test]
    async fn a_noisy_log_is_trimmed_to_what_it_printed_and_survives_the_tool_result() {
        let server = MockServer::start().await;
        mock(&server, "commits/main", json!({"sha": COMMIT})).await;
        mock(&server, &format!("actions/jobs/{JOB_ID}"), job(COMMIT)).await;
        let mut lines: Vec<String> = (0..300)
            .map(|index| {
                format!(
                    "2026-09-05T00:00:00.1234567Z \u{1b}[32mok\u{1b}[0m step {index}\rretry {index}\rdone {index}"
                )
            })
            .collect();
        lines.push(format!(
            "2026-09-05T00:00:00.1234567Z \u{1b}[31m##[error]assertion failed\u{1b}[0m {}",
            "detail ".repeat(400)
        ));
        mock_log(&server, lines.join("\r\n")).await;
        let result = github(&server)
            .observe(Operation::CheckLogs, &check_args(Some(JOB_ID), None, None))
            .await
            .unwrap();
        let excerpt = result["log"]["excerpt"].as_str().unwrap();
        assert!(
            !excerpt.contains('\r'),
            "carriage-return redraws must collapse to the state that survived them"
        );
        assert!(
            excerpt.contains("done 299") && !excerpt.contains("retry 299"),
            "only the settled state of a redrawn line is worth the budget"
        );
        assert!(
            excerpt.contains("##[error]assertion failed"),
            "terminal escapes must not hide the failure region"
        );
        assert!(
            excerpt
                .lines()
                .all(|line| line.chars().count() <= LOG_LINE_CHARS + 1),
            "no single line may crowd out the rest of the excerpt"
        );
        assert!(excerpt.chars().count() <= LOG_EXCERPT_CHARS as usize);
        let message = ToolResult::success(result.to_string()).to_message();
        assert_eq!(
            message,
            result.to_string(),
            "the excerpt must fit the tool message budget instead of being cut at the boundary"
        );
    }

    #[test]
    fn log_lines_collapse_redraws_strip_stamps_and_bound_width() {
        assert_eq!(
            log_lines("2026-09-05T00:00:00.1234567Z hello\r\n\r\n"),
            vec!["hello".to_string()]
        );
        assert_eq!(
            log_lines("1%\r50%\r100% done"),
            vec!["100% done".to_string()]
        );
        assert_eq!(log_lines("plain\r"), vec!["plain".to_string()]);
        assert_eq!(
            strip_timestamp("not a stamp at all here"),
            "not a stamp at all here"
        );
        let wide = log_lines(&"x".repeat(LOG_LINE_CHARS * 3));
        assert_eq!(wide[0].chars().count(), LOG_LINE_CHARS + 1);
        assert!(log_lines("").is_empty());
        assert_eq!(find_error(&log_lines("a\nb")), None);
        assert_eq!(
            find_error(&log_lines("Command failed\nlater\n##[error]real")),
            Some(2),
            "an annotated error outranks an earlier generic marker"
        );
    }

    async fn mock_changed_files(server: &MockServer, filenames: &[&str]) {
        mock(
            server,
            "pulls/7/files",
            json!(
                filenames
                    .iter()
                    .map(|filename| json!({"filename": filename, "status": "modified"}))
                    .collect::<Vec<_>>()
            ),
        )
        .await;
    }

    async fn assess_green_pull(server: &MockServer) -> Value {
        mock_pull(server, COMMIT).await;
        mock_graphql(
            server,
            review_payload(COMMIT, true, &summary(5, COMMIT), false),
        )
        .await;
        mock_green_ci(server).await;
        github(server)
            .observe(Operation::PullRequests, &readiness_args(Some(7)))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn changed_files_are_fetched_and_reported_as_a_blast_radius() {
        let server = MockServer::start().await;
        mock_changed_files(
            &server,
            &["src/auth/token.rs", "README.md", "src/api/routes.rs"],
        )
        .await;
        let result = assess_green_pull(&server).await;
        let radius = &result["pull_requests"][0]["blast_radius"];
        assert_eq!(radius["tier"], "critical");
        assert_eq!(radius["touched"], 3);
        assert_eq!(radius["drivers"], json!(["src/auth/token.rs"]));
        assert_eq!(radius["presumed"], false);
        assert_eq!(
            radius["summary"],
            "Blast radius critical across 3 files (src/auth/token.rs)"
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path() == "/repos/owner/repository/pulls/7/files"),
            "the changed file list is fetched from the pull request files endpoint"
        );
    }

    #[tokio::test]
    async fn an_unfetchable_file_list_reads_as_assumed_rather_than_confident() {
        let server = MockServer::start().await;
        let result = assess_green_pull(&server).await;
        let radius = &result["pull_requests"][0]["blast_radius"];
        assert_eq!(
            radius["tier"], "core",
            "no observation must not read as a small change"
        );
        assert_eq!(radius["touched"], 0);
        assert_eq!(radius["presumed"], true);
        assert_eq!(
            radius["summary"],
            "Blast radius core (assumed: no files were observed)"
        );
    }

    #[tokio::test]
    async fn a_file_list_that_cannot_be_read_in_full_reads_as_assumed() {
        let server = MockServer::start().await;
        mock(
            &server,
            "pulls/7/files",
            json!([{"filename": "src/auth/token.rs"}, {"status": "modified"}]),
        )
        .await;
        let result = assess_green_pull(&server).await;
        let radius = &result["pull_requests"][0]["blast_radius"];
        assert_eq!(radius["presumed"], true);
        assert_eq!(radius["touched"], 0);
        assert_eq!(
            radius["tier"], "core",
            "a partial diff must not produce a confident tier"
        );
    }

    #[tokio::test]
    async fn a_blast_radius_can_never_flip_readiness() {
        for (files, tier) in [
            (vec!["src/auth/token.rs"], "critical"),
            (vec!["deploy/helm/values.yaml"], "infrastructure"),
            (vec!["README.md"], "cosmetic"),
            (vec!["tests/queue.rs"], "test"),
            (vec!["src/api/routes.rs"], "core"),
            (vec!["src/formatting/pretty.rs"], "peripheral"),
            (vec![], "core"),
        ] {
            for green in [true, false] {
                let server = MockServer::start().await;
                if !files.is_empty() {
                    mock_changed_files(&server, &files).await;
                }
                mock_pull(&server, COMMIT).await;
                mock_graphql(
                    &server,
                    review_payload(
                        COMMIT,
                        true,
                        &summary(if green { 5 } else { 3 }, COMMIT),
                        false,
                    ),
                )
                .await;
                mock_green_ci(&server).await;
                let result = github(&server)
                    .observe(Operation::PullRequests, &readiness_args(Some(7)))
                    .await
                    .unwrap();
                let pull = &result["pull_requests"][0];
                assert_eq!(
                    pull["ready"], green,
                    "blast radius {tier} changed readiness for green={green}"
                );
                assert_eq!(pull["blast_radius"]["tier"], tier);
                assert!(
                    !take_array(pull, "blockers")
                        .iter()
                        .any(|blocker| text(blocker, "code").contains("blast")),
                    "a blast radius must never appear as a blocker"
                );
            }
        }
    }

    #[tokio::test]
    async fn a_review_bot_outside_the_configured_set_is_not_review_evidence() {
        let server = MockServer::start().await;
        mock_changed_files(&server, &["README.md"]).await;
        mock_pull(&server, COMMIT).await;
        mock_graphql(
            &server,
            review_payload(COMMIT, true, &summary(5, COMMIT), false),
        )
        .await;
        mock_green_ci(&server).await;
        let mut github = github(&server);
        github.signals = ReviewSignals::select(&["coderabbit"]).unwrap();
        let result = github
            .observe(Operation::PullRequests, &readiness_args(Some(7)))
            .await
            .unwrap();
        let pull = &result["pull_requests"][0];
        assert_eq!(pull["ready"], false);
        assert_eq!(pull["blockers"][0]["code"], "review_missing");
        assert_eq!(
            pull["blockers"][0]["detail"],
            "coderabbitai summary missing"
        );
    }

    #[test]
    fn a_configured_review_signal_set_is_validated_when_the_source_is_read() {
        let configuration = |signals: Option<Vec<String>>| Configuration {
            owner: "owner".into(),
            repo: "repository".into(),
            branch: None,
            path: None,
            token: None,
            review_signals: signals,
        };
        assert_eq!(
            Github::new(configuration(None))
                .unwrap()
                .signals
                .reviewers(),
            ReviewSignals::recognized().reviewers()
        );
        assert_eq!(
            Github::new(configuration(Some(vec!["greptile".into()])))
                .unwrap()
                .signals
                .reviewers(),
            vec!["greptile-apps".to_string()]
        );
        let error = Github::new(configuration(Some(vec!["sonarcloud".into()])))
            .err()
            .expect("an unrecognised review signal is rejected");
        assert!(error.contains("sonarcloud"), "{error}");
    }

    const RELEASE_TAG: &str = "v1.4.0";

    fn workflow_run(id: u64, workflow_id: u64, conclusion: &str) -> Value {
        json!({
            "id": id,
            "workflow_id": workflow_id,
            "run_attempt": 1,
            "name": "Publish",
            "path": ".github/workflows/publish.yml",
            "event": "release",
            "status": "completed",
            "conclusion": conclusion,
            "head_branch": RELEASE_TAG,
            "head_sha": COMMIT,
            "created_at": "2026-09-05T00:01:00Z",
            "run_started_at": "2026-09-05T00:01:10Z",
            "updated_at": "2026-09-05T00:04:00Z",
            "html_url": format!("https://github.com/owner/repository/actions/runs/{id}"),
            "repository": {"full_name": "owner/repository"},
        })
    }

    async fn mock_release(server: &MockServer) {
        mock(
            server,
            "releases",
            json!([{
                "id": 90,
                "tag_name": RELEASE_TAG,
                "published_at": "2026-09-05T00:00:00Z",
                "html_url": "https://github.com/owner/repository/releases/tag/v1.4.0",
            }]),
        )
        .await;
    }

    async fn mock_release_runs(server: &MockServer, runs: Vec<Value>) {
        let total = runs.len();
        mock(
            server,
            "actions/runs",
            json!({"workflow_runs": runs, "total_count": total}),
        )
        .await;
    }

    fn release_args(tag: Option<&str>) -> Arguments {
        Arguments {
            tag: tag.map(str::to_string),
            ..readiness_args(None)
        }
    }

    #[tokio::test]
    async fn a_release_whose_runs_all_succeeded_is_green_and_cites_the_commit() {
        let server = MockServer::start().await;
        mock_release(&server).await;
        mock_release_runs(
            &server,
            vec![
                workflow_run(1, 10, "success"),
                workflow_run(2, 20, "success"),
            ],
        )
        .await;
        let result = github(&server)
            .observe(Operation::ReleasePipelines, &release_args(None))
            .await
            .unwrap();
        let release = &result["releases"][0];
        assert_eq!(release["state"], "succeeded");
        assert_eq!(release["succeeded"], true);
        assert_eq!(release["lookup"], "complete");
        assert_eq!(release["commit"], COMMIT);
        assert_eq!(release["runs"].as_array().unwrap().len(), 2);
        assert_eq!(release["blockers"].as_array().unwrap().len(), 0);
        assert_eq!(result["succeeded"], 1);
        assert_eq!(result["assessed"], 1);
        assert_eq!(result["next_page"], Value::Null);
        assert!(json_chars(&result) <= FILE_PAGE_CHARS as usize);

        let citation = crate::agent::citations::from_tool_at(
            "assess_release_pipelines",
            &result.to_string(),
            "2026-09-05T00:00:00+00:00",
        )
        .remove(0);
        assert!(
            citation.passing(),
            "a release pipeline this server observed is authoritative"
        );
        assert_eq!(citation.revision.as_deref(), Some(COMMIT));
        assert_eq!(
            citation.url,
            "https://github.com/owner/repository/releases/tag/v1.4.0"
        );
    }

    #[tokio::test]
    async fn a_failed_release_run_is_named_and_never_green() {
        let server = MockServer::start().await;
        mock_release(&server).await;
        mock_release_runs(
            &server,
            vec![
                workflow_run(1, 10, "success"),
                workflow_run(2, 20, "failure"),
            ],
        )
        .await;
        let result = github(&server)
            .observe(Operation::ReleasePipelines, &release_args(None))
            .await
            .unwrap();
        let release = &result["releases"][0];
        assert_eq!(release["state"], "failed");
        assert_eq!(release["succeeded"], false);
        assert_eq!(release["blockers"][0]["code"], "runs_failed");
        assert_eq!(
            release["blockers"][0]["detail"],
            "1 of 2 release workflow runs failed"
        );
        let citation = crate::agent::citations::from_tool_at(
            "assess_release_pipelines",
            &result.to_string(),
            "2026-09-05T00:00:00+00:00",
        )
        .remove(0);
        assert!(!citation.passing());
        assert_eq!(citation.outcome, crate::agent::CitationOutcome::Failure);
    }

    #[tokio::test]
    async fn a_release_with_no_observed_run_is_unknown_rather_than_green() {
        let server = MockServer::start().await;
        mock_release(&server).await;
        mock_release_runs(&server, Vec::new()).await;
        let result = github(&server)
            .observe(Operation::ReleasePipelines, &release_args(None))
            .await
            .unwrap();
        let release = &result["releases"][0];
        assert_eq!(release["state"], "unknown");
        assert_eq!(release["succeeded"], false);
        assert_eq!(release["complete"], false);
        assert_eq!(release["lookup"], "pending");
        assert_eq!(release["commit"], Value::Null);
        assert_eq!(result["succeeded"], 0);
        let citation = crate::agent::citations::from_tool_at(
            "assess_release_pipelines",
            &result.to_string(),
            "2026-09-05T00:00:00+00:00",
        )
        .remove(0);
        assert!(!citation.passing());
        assert_eq!(citation.outcome, crate::agent::CitationOutcome::Incomplete);
    }

    #[tokio::test]
    async fn a_run_record_that_does_not_add_up_makes_the_release_unknown() {
        let server = MockServer::start().await;
        mock_release(&server).await;
        let mut impostor = workflow_run(2, 20, "success");
        impostor["html_url"] = json!("https://github.com/owner/repository/actions/runs/999");
        mock_release_runs(&server, vec![workflow_run(1, 10, "success"), impostor]).await;
        let result = github(&server)
            .observe(Operation::ReleasePipelines, &release_args(None))
            .await
            .unwrap();
        let release = &result["releases"][0];
        assert_eq!(release["state"], "unknown");
        assert_eq!(release["lookup"], "unavailable");
        assert_eq!(release["blockers"][0]["code"], "lookup_unavailable");
        assert_eq!(
            release["runs"].as_array().unwrap().len(),
            1,
            "the record that held is still reported, it is just not a complete lookup"
        );
    }

    #[tokio::test]
    async fn a_release_can_be_assessed_by_tag_alone() {
        let server = MockServer::start().await;
        mock(
            &server,
            &format!("releases/tags/{RELEASE_TAG}"),
            json!({
                "id": 90,
                "tag_name": RELEASE_TAG,
                "published_at": "2026-09-05T00:00:00Z",
                "html_url": "https://github.com/owner/repository/releases/tag/v1.4.0",
            }),
        )
        .await;
        mock_release_runs(&server, vec![workflow_run(1, 10, "success")]).await;
        let result = github(&server)
            .observe(
                Operation::ReleasePipelines,
                &release_args(Some(RELEASE_TAG)),
            )
            .await
            .unwrap();
        assert_eq!(result["releases"][0]["succeeded"], true);
        assert_eq!(result["next_page"], Value::Null);
        assert_eq!(result["complete"], true);
        assert_eq!(
            github(&server)
                .observe(Operation::ReleasePipelines, &release_args(Some("../etc")))
                .await
                .unwrap_err(),
            "tag is invalid."
        );
    }

    #[tokio::test]
    async fn a_release_the_sweep_missed_falls_back_to_its_own_lookup() {
        let server = MockServer::start().await;
        mock_release(&server).await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository/actions/runs"))
            .and(query_param("branch", RELEASE_TAG))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"workflow_runs": [workflow_run(1, 10, "success")], "total_count": 1}),
            ))
            .mount(&server)
            .await;
        mock_release_runs(&server, Vec::new()).await;
        let result = github(&server)
            .observe(Operation::ReleasePipelines, &release_args(None))
            .await
            .unwrap();
        let release = &result["releases"][0];
        assert_eq!(
            release["succeeded"], true,
            "an empty repository sweep is not evidence that a release has no pipeline"
        );
        assert_eq!(release["lookup"], "complete");
    }

    #[tokio::test]
    async fn a_release_without_a_usable_identity_fails_closed() {
        let server = MockServer::start().await;
        mock(
            &server,
            "releases",
            json!([{"id": 90, "tag_name": "", "published_at": "2026-09-05T00:00:00Z"}]),
        )
        .await;
        assert_eq!(
            github(&server)
                .observe(Operation::ReleasePipelines, &release_args(None))
                .await
                .unwrap_err(),
            "GitHub returned a release without a usable identity."
        );
    }

    #[tokio::test]
    async fn one_sweep_is_routed_to_the_release_each_run_belongs_to() {
        let server = MockServer::start().await;
        mock(
            &server,
            "releases",
            json!([
                {
                    "id": 90,
                    "tag_name": RELEASE_TAG,
                    "published_at": "2026-09-05T00:00:00Z",
                    "html_url": "https://github.com/owner/repository/releases/tag/v1.4.0",
                },
                {
                    "id": 89,
                    "tag_name": "v1.3.0",
                    "published_at": "2026-09-01T00:00:00Z",
                    "html_url": "https://github.com/owner/repository/releases/tag/v1.3.0",
                },
            ]),
        )
        .await;
        let mut older = workflow_run(2, 20, "failure");
        older["head_branch"] = json!("v1.3.0");
        mock_release_runs(&server, vec![workflow_run(1, 10, "success"), older]).await;
        let result = github(&server)
            .observe(Operation::ReleasePipelines, &release_args(None))
            .await
            .unwrap();
        let releases = result["releases"].as_array().unwrap();
        assert_eq!(releases.len(), 2);
        let current = releases
            .iter()
            .find(|row| row["tag"] == RELEASE_TAG)
            .unwrap();
        let previous = releases.iter().find(|row| row["tag"] == "v1.3.0").unwrap();
        assert_eq!(
            current["succeeded"], true,
            "another release's failure is not this release's evidence"
        );
        assert_eq!(current["lookup"], "complete");
        assert_eq!(current["runs"].as_array().unwrap().len(), 1);
        assert_eq!(previous["state"], "failed");
        assert_eq!(result["succeeded"], 1);
    }
}
