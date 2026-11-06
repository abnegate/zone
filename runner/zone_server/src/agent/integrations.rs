//! Live GitHub observations and writes using an authorized workspace source.

use abnegate_secret::SecretValue;
use abnegate_vcs::BranchName;
use abnegate_vcs::CheckRun;
use abnegate_vcs::Collected;
use abnegate_vcs::CommitSha as Revision;
use abnegate_vcs::CommitStatus;
use abnegate_vcs::DiffSide;
use abnegate_vcs::Issue;
use abnegate_vcs::IssueComment;
use abnegate_vcs::IssueReference;
use abnegate_vcs::JobLogError;
use abnegate_vcs::Label;
use abnegate_vcs::ListedPullRequest;
use abnegate_vcs::ObjectId;
use abnegate_vcs::ParseError;
use abnegate_vcs::PullRequestDetail;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::PullRequestReference;
use abnegate_vcs::PullRequestService;
use abnegate_vcs::PullRequestState;
use abnegate_vcs::Release;
use abnegate_vcs::Repository;
use abnegate_vcs::ReviewThreadCount;
use abnegate_vcs::ReviewThreadRecord;
use abnegate_vcs::StateFilter;
use abnegate_vcs::ThreadComment;
use abnegate_vcs::TreeEntry;
use abnegate_vcs::TreeEntryKind;
use abnegate_vcs::WorkflowJob;
use abnegate_vcs::WorkflowRun;
use abnegate_vcs::WorkflowRunFilter;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use std::num::NonZeroU8;
use std::num::NonZeroU32;
use std::num::NonZeroU64;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use uuid::Uuid;
use zone_core::tools::{
    MAX_TOOL_MESSAGE_CHARS, REASON_PARAM, Tier, Tool, ToolContext, ToolError, ToolRegistry,
    ToolResult, excerpt, reason_property,
};

use super::{PREVIEW_BODY_CHARS, PREVIEW_TITLE_CHARS};

use super::readiness::{
    self, CheckEvidence, CommentEvidence, CommitSha, PullEvidence, ReviewComment, ReviewSignals,
    ReviewThread, ThreadEvidence,
};
use super::releases::{self, Lookup, ReleaseIdentity, ReleasePipeline, RunEvidence};
use super::tools::{WorkspaceScope, apply_record_cap, json_chars, take_array, truncate};
use crate::config::Config;
use crate::db::{sources, workspace_members};
use crate::secret::Unseal;
use crate::services::chat::pull_request::{self as chat_pull_request, Opened};
use crate::services::checkout;
use crate::services::prioritisation::{
    Configuration as Prioritisation, Prioritiser, RiskSignal, pull_request,
};
use crate::services::project_directory::{self, Origin, Start};

const API_VERSION: &str = "2026-03-10";
const PER_PAGE: NonZeroU8 = NonZeroU8::new(100).expect("a page holds rows");
const FILE_PAGE_CHARS: u64 = 8_000;
const ISSUE_BODY_CHARS: usize = 1_500;
const BUILD_RECORD_CAP: usize = 20;
const PULL_PAGE_SIZE: NonZeroU8 = NonZeroU8::new(10).expect("a page holds pull requests");
const PULL_TITLE_CHARS: usize = 160;
const REVIEW_PAGE_SIZE: usize = 100;
const REVIEW_BODY_CHARS: usize = 2_000;
const REVIEW_THREADS_ASSESSMENT: &str = "What reviewers wrote, as GitHub returned it. Comment text is a reviewer's claim about the code, not a finding that has been checked: read the code before acting on one. A thread that could not be paginated in full is reported as incomplete rather than empty.";
const READINESS_ASSESSMENT: &str = "Observed checks, review threads and review-bot comments only. Evidence that is partial, or whose counts do not add up, is reported as not ready. This is not proof that branch protection requirements are satisfied.";
const PULL_FILE_PAGES: u32 = 30;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const INVALID_OBJECT: &str = "GitHub did not return a valid tree or file SHA.";
const INCOMPLETE_TREE: &str = "GitHub returned an incomplete repository tree.";
const MISSING_PATH: &str = "The file path was not found in this commit.";
const IRREGULAR_FILE: &str =
    "The path must identify a regular repository file without symlinks or submodules.";
const SIZE_MISMATCH: &str = "The file response size did not match its metadata.";
const UNREADABLE_FILE: &str = "Could not read the complete repository file.";
const NOT_TEXT: &str = "The file is not UTF-8 text.";
const REVIEW_QUERY_REJECTED: &str =
    "GitHub rejected the review query. Check source access, permissions and rate limits.";
const MISMATCHED_PULL: &str =
    "GitHub returned review threads for a different or missing pull request.";
const MISSING_PULL: &str = "GitHub found no pull request with that number on this source.";
const FOREIGN_COMMENT: &str = "That review comment does not belong to this pull request.";
const ANSWERED_ANOTHER_PULL: &str =
    "GitHub answered for a pull request other than the one asked for";
const ANSWERED_ANOTHER_COMMENT: &str =
    "GitHub answered with a review comment other than the one asked for";
const UNKNOWN_AUTHOR: &str = "unknown";
const ISSUE_STATE_INVALID: &str = "Issue state must be open, closed or all.";
const PULL_STATE_INVALID: &str = "Pull request state must be open, closed or all.";
const NUMBER_REQUIRED: &str = "number is required.";
const BODY_REQUIRED: &str = "body is required.";
const HEAD_INVALID: &str = "head is invalid.";
const BASE_INVALID: &str = "base is invalid.";
const RELEASE_PAGE_SIZE: NonZeroU8 = NonZeroU8::new(5).expect("a page holds releases");
const RELEASE_EVENT: &str = "release";
const RELEASE_ASSESSMENT: &str = "Observed release-triggered workflow runs only. A lookup that could not be completed, a run record whose identity does not add up, and a release with no observed run are all reported as unknown rather than succeeded. This is not proof that release artifacts were published or that the deployed service is healthy.";
const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;
const LOG_EXCERPT_CHARS: u64 = 5_500;
const LOG_EXCERPT_FLOOR_CHARS: u64 = 500;
const LOG_CONTEXT_LINES: usize = 40;
const LOG_TAIL_LINES: usize = 120;
const LOG_LINE_CHARS: usize = 500;
const ELISION_RESERVE_CHARS: usize = 120;
const RESULT_ENVELOPE_CHARS: usize = 384;
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
const LOG_NOTE: &str = "Excerpt of what the job printed, not proof of why it failed. Per-line timestamps, progress redraws and over-long lines are trimmed, and unshown regions are marked as omitted.";
pub(crate) const SUCCESS_ASSESSMENT: &str = "success";
const FAILURE_ASSESSMENT: &str = "failure";
pub(crate) const PENDING_ASSESSMENT: &str = "pending";
const UNKNOWN_ASSESSMENT: &str = "unknown";

/// Conclusions a completed run reports that are neither a pass nor a failure.
/// The run finished and said so, which is the opposite of the silence
/// `unknown` stands for, so a wait on them ends — named, and never as a pass.
const TERMINAL_ASSESSMENTS: [&str; 2] = ["neutral", "skipped"];

pub const SETTLED_ASSESSMENTS: [&str; 4] = [
    SUCCESS_ASSESSMENT,
    FAILURE_ASSESSMENT,
    TERMINAL_ASSESSMENTS[0],
    TERMINAL_ASSESSMENTS[1],
];
const RATE_LIMIT_EXHAUSTED: &str = "GitHub's API rate limit is exhausted for this source's credential. Retrying now will not help; wait for the limit to reset.";
const REQUEST_FAILED: &str = "GitHub request failed or timed out.";
const INVALID_RESPONSE: &str = "GitHub returned an invalid response.";
const UNEXPECTED_RESPONSE: &str = "GitHub returned an unexpected response. Check source access, permissions, rate limits and the requested resource.";
const INVALID_REFERENCE: &str = "ref is invalid.";
const FILE_TOO_LARGE: &str = "The file is too large to read.";
const INVALID_ORIGIN: &str = "The configured GitHub API origin is invalid.";
const INVALID_REPOSITORY: &str = "The source owner or repository name is invalid.";
const CLIENT_UNAVAILABLE: &str = "Could not initialize the GitHub client.";
const MISSING_DEFAULT_BRANCH: &str = "GitHub did not return a default branch.";
const MISSING_COMMIT: &str = "GitHub did not return an immutable commit SHA.";
const NUMBER_NOT_POSITIVE: &str = "number must be positive.";
const PAGE_NOT_POSITIVE: &str = "page must be positive.";
const LOG_UNDIRECTED: &str = "GitHub redirected the job log without a destination.";
const LOG_UNEXPECTED_HOST: &str = "GitHub redirected the job log to an unexpected host.";
const LOG_UNREACHABLE: &str = "The job log download failed or timed out.";
const LOG_INTERRUPTED: &str = "The job log download failed part way through.";
const BUILD_ASSESSMENT: &str =
    "Observed CI only; required branch checks and service health are not evaluated.";
const COMPLETED_STATUS: &str = "completed";
const UNIDENTIFIED_CI: &str = "GitHub returned CI records without their identities.";
const FOREIGN_CHECKS: &str = "GitHub returned checks for a different or missing commit SHA.";
const WORKFLOW_SEARCH_LIMIT: &str =
    "GitHub's 1,000-result workflow search limit prevents a complete build assessment.";
const INVALID_TAG: &str = "tag is invalid.";
const UNUSABLE_RELEASE: &str = "GitHub returned a release without a usable identity.";
const FOREIGN_DEPLOYMENT: &str =
    "GitHub returned a deployment for a different or missing commit SHA.";
const DIFFERENT_PULL: &str = "GitHub returned a different pull request.";
const MISSING_PULL_HEAD: &str = "GitHub did not return an immutable pull request head SHA.";
const JOB_WITHOUT_COMMIT: &str = "GitHub returned a job without a commit SHA.";
const FOREIGN_JOB: &str =
    "That job does not belong to the pull request or commit being asked about.";
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);
const UNAUTHORIZED: u16 = 401;
const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;
const UNPROCESSABLE_ENTITY: u16 = 422;
const TOO_MANY_REQUESTS: u16 = 429;

/// Compiling the path vocabulary is the expensive part, so the prioritiser
/// behind a blast radius is built once and shared by every assessment.
static PRIORITISER: LazyLock<Prioritiser> =
    LazyLock::new(|| Prioritiser::new(Prioritisation::default()));

#[derive(Clone, Copy)]
enum Operation {
    Build,
    Deployments,
    Issues,
    Issue,
    File,
    PullRequests,
    ReviewThreads,
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
        Operation::Issue,
        Operation::File,
        Operation::PullRequests,
        Operation::ReviewThreads,
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
    directory: Option<String>,
    number: Option<u64>,
    #[serde(default)]
    resolved: Option<bool>,
    #[serde(default)]
    comment: Option<u64>,
    #[serde(default)]
    after: Option<u64>,
    job_id: Option<u64>,
    tag: Option<String>,
    /// Why the model made this write. Declared so `deny_unknown_fields` accepts
    /// the property the write schemas advertise; a parse failure here reaches
    /// the model as an unrepairable message, so its absence never fails a call.
    reason: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct Configuration {
    pub(crate) owner: String,
    pub(crate) repo: String,
    pub(crate) branch: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) token: Option<String>,
    /// Which review bots count as review evidence for this source. Absent
    /// means every bot this build recognises.
    pub(crate) review_signals: Option<Vec<String>>,
}

#[async_trait]
impl Tool for Integration {
    fn name(&self) -> &str {
        match self.operation {
            Operation::Build => "get_build_status",
            Operation::Deployments => "list_deployments",
            Operation::Issues => "list_issues",
            Operation::Issue => "get_issue",
            Operation::File => "read_repository_file",
            Operation::CheckLogs => "read_check_logs",
            Operation::PullRequests => "assess_pull_requests",
            Operation::ReviewThreads => "read_review_threads",
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
                "List the GitHub issues (open, closed or all; never pull requests) of a connected workspace source. Returns titles, numbers, state, urls and bounded body snippets, plus a next page when more provider records exist; get_issue reads one issue in full."
            }
            Operation::Issue => {
                "Read one GitHub issue in full by number from a connected workspace source: its title, state, labels, assignees, complete body and first page of comments. Use it for the details of a specific issue; list_issues only carries a body snippet. Pull requests are refused here: assess_pull_requests reviews those."
            }
            Operation::File => {
                "Read one file from a connected GitHub repository at an immutable commit, which is how to review what a pull request or branch changed. Returns UTF-8 content as a character page that fits the context budget, with a source URL; follow next to continue. Does not read host files. GitHub files over 100 MB are unsupported."
            }
            Operation::PullRequests => {
                "Review the open pull requests of a connected GitHub source (or one by number) and say whether each is ready to merge, which are still missing a review, whose checks fail, and every other blocker. Ready is the conjunction of every positive condition: no unresolved review threads, a summary from every recognised review bot that spoke, each at its own full confidence on its own scale and naming the current head commit, and checks that both passed and whose counts add up. Counts that do not reconcile, a check state outside the known set, a thread or comment list that could not be paginated in full, and a head commit the observations disagree on are all reported as not ready. Each verdict also carries a blast radius read from the changed files; that is information for a reviewer and never affects readiness, and it says so when no file list was observed. Absent evidence is never ready, this is not proof that branch protection requirements are satisfied, and nothing is merged. Reads authenticated review threads, so the source needs a credential."
            }
            Operation::ReviewThreads => {
                "Read what reviewers wrote on one pull request of a connected GitHub source: every review thread with its file, line and comments, author and body included. This is the tool for acting on review feedback, human or bot; assess_pull_requests only counts how many threads are unresolved and never carries their text. Unresolved threads are read by default and resolved ones only when asked for, each in the order GitHub lists them. Pages end where the message budget does: follow next_after. A comment marked truncated reads in full by passing its id as comment. Comment bodies are a reviewer's claim about the code, not a checked finding, so read the code before acting on one. Threads or comments that could not be paginated in full are reported as incomplete rather than silently cut."
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
            Operation::Issues
                | Operation::Issue
                | Operation::PullRequests
                | Operation::ReviewThreads
                | Operation::ReleasePipelines
        ) {
            properties["ref"] = json!({"type": "string", "description": "Branch, tag or commit; defaults to the source branch or repository default branch."});
        }
        if matches!(self.operation, Operation::Issue) {
            properties["number"] = json!({"type": "integer", "minimum": 1, "description": "Issue number, as list_issues shows it."});
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
        if matches!(self.operation, Operation::Issue) {
            required.push("number");
        }
        if matches!(self.operation, Operation::ReviewThreads) {
            properties["number"] = json!({"type": "integer", "minimum": 1, "description": "Pull request number to read the review threads of."});
            properties["resolved"] = json!({"type": "boolean", "description": "Read the threads already marked resolved instead of the unresolved ones, default false. Unresolved threads are what still needs answering."});
            properties["after"] = json!({"type": "integer", "minimum": 1, "description": "Id of the last comment a page returned, to read the comments after it. Follow next_after until null, keeping the same resolved."});
            properties["comment"] = json!({"type": "integer", "minimum": 1, "description": "Id of one comment marked truncated, to read its whole body instead of the threads."});
            properties["offset"] = json!({"type": "integer", "minimum": 0, "description": "Only with comment: a character offset into that body, default 0. Follow next until null."});
            required.push("number");
        }
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
            properties["directory"] = json!({"type": "string", "description": "Absolute path of the checkout you committed head in. Zone pushes head from it with the source's credential before opening the pull request; do not git push yourself. Defaults to the chat's project directory. Omit for a fork's owner:branch head that is already pushed."});
            required.extend(["title", "head"]);
        }
        if matches!(self.operation, Operation::Comment) {
            properties["number"] = json!({"type": "integer", "minimum": 1, "description": "Issue or pull request number."});
            properties["body"] = json!({"type": "string", "description": "Comment markdown."});
            required.extend(["number", "body"]);
        }
        if self.tier().mutating() {
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

    /// Both writes land on a repository other people watch, under the
    /// workspace's own credential.
    fn tier(&self) -> Tier {
        match self.operation {
            Operation::CreatePull | Operation::Comment => Tier::Outward,
            Operation::Build
            | Operation::Deployments
            | Operation::Issues
            | Operation::Issue
            | Operation::File
            | Operation::CheckLogs
            | Operation::PullRequests
            | Operation::ReviewThreads
            | Operation::ReleasePipelines => Tier::Read,
        }
    }

    fn preview(&self, params: &Value) -> Option<String> {
        match self.operation {
            Operation::CreatePull => {
                let base = params["base"].as_str().unwrap_or("the default branch");
                let body = params["body"]
                    .as_str()
                    .map_or(0, |body| body.chars().count());
                Some(format!(
                    "Open a pull request titled \"{}\", merging {} into {base}, with a \
                     {body}-character description.",
                    excerpt(params["title"].as_str()?, PREVIEW_TITLE_CHARS),
                    params["head"].as_str()?
                ))
            }
            Operation::Comment => Some(format!(
                "Comment on #{}: \"{}\"",
                params["number"].as_u64()?,
                excerpt(params["body"].as_str()?, PREVIEW_BODY_CHARS)
            )),
            _ => None,
        }
    }
}

impl Integration {
    async fn run(&self, params: Value) -> Result<Value, String> {
        let arguments: Arguments = serde_json::from_value(params)
            .map_err(|_| "Invalid integration arguments.".to_string())?;
        if let Some(number) = arguments.page {
            page(number)?;
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
                self.scope
                    .state
                    .master_key()
                    .open_strict(&encrypted)
                    .map(|token| token.expose().to_string())
                    .map_err(|_| "The source credentials could not be decrypted.".to_string())?,
            );
        }
        let github = Github::new(configuration, self.scope.state.config())?;
        let mut result = if matches!(self.operation, Operation::CreatePull) {
            self.open_pull(&github, &arguments).await?
        } else if write {
            github.write(self.operation, &arguments).await?
        } else {
            github.observe(self.operation, &arguments).await?
        };
        result["source_id"] = json!(arguments.source_id);
        result["observed_at"] = json!(Utc::now().to_rfc3339());
        if matches!(self.operation, Operation::CreatePull) {
            self.record(&arguments, &result).await;
        }
        Ok(result)
    }

    /// Opens the pull request, or when GitHub refuses it because one is
    /// already open from the same head into the same base, answers with that
    /// one, so a retry after a timed-out create gets the pull request the
    /// first attempt made instead of an error.
    async fn open_pull(&self, github: &Github, arguments: &Arguments) -> Result<Value, String> {
        let repository = format!(
            "https://github.com/{}/{}",
            github.configuration.owner, github.configuration.repo
        );
        let (_, head, base) = github.pull_fields(arguments)?;
        if head == base {
            return Err(format!(
                "head and base are both {base}. Commit on a branch of its own and open the pull \
                 request from that."
            ));
        }
        self.push_head(github, arguments, head, &repository).await?;
        let (head, base) = match github.create_pull(arguments).await? {
            Creation::Created(created) => {
                return Ok(json!({
                    "repository": repository,
                    "pull_request": created,
                }));
            }
            Creation::Refused { head, base } => (head, base),
        };
        let existing = github
            .already_open(&head, &base)
            .await
            .ok_or_else(|| status_error(UNPROCESSABLE_ENTITY))?;
        Ok(json!({
            "repository": repository,
            "pull_request": existing,
            "already_open": true,
        }))
    }

    /// Pushes `head` from the checkout the chat committed it in, with the
    /// source's token, so a chat whose agent holds no GitHub credential can
    /// still open a pull request. A fork's `owner:branch` head, and a chat with
    /// no checkout to push from, are left for GitHub to find already pushed.
    async fn push_head(
        &self,
        github: &Github,
        arguments: &Arguments,
        head: &str,
        repository: &str,
    ) -> Result<(), String> {
        if head.contains(':') {
            return Ok(());
        }
        let Some(directory) = self.checkout(arguments).await? else {
            return Ok(());
        };
        let token = github
            .configuration
            .token
            .as_deref()
            .ok_or("The source has no credential to push with.")?;
        let url = abnegate_vcs::RepositoryUrl::parse(repository)
            .map_err(|_| "The source does not name a GitHub repository.".to_string())?;
        let origin =
            Origin::parse(repository).ok_or("The source does not name a GitHub repository.")?;
        chat_pull_request::push(
            &checkout::git(),
            &directory,
            &url,
            &origin,
            head,
            &SecretValue::new(token),
        )
        .await
        .map(drop)
        .map_err(|error| format!("Could not push {head}: {error}"))
    }

    /// The checkout to push from: the one the model names, which must be one
    /// of this workspace's, or else the directory the chat's project starts
    /// it in, which the turn itself ran in.
    async fn checkout(&self, arguments: &Arguments) -> Result<Option<std::path::PathBuf>, String> {
        if let Some(directory) = arguments
            .directory
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let directory = std::path::PathBuf::from(directory);
            if !project_directory::owns(&self.scope.state, self.scope.workspace_id, &directory)
                .await
            {
                return Err(format!(
                    "{} is not in one of this workspace's folders.",
                    directory.display()
                ));
            }
            return Ok(Some(directory));
        }
        let Some(chat) = self.scope.chat_id else {
            return Ok(None);
        };
        Ok(
            match project_directory::peek(&self.scope.state, chat, self.scope.workspace_id).await {
                Start::Directory(directory) => Some(directory),
                _ => None,
            },
        )
    }

    /// Notes the pull request this chat just opened as the chat's own. A
    /// failure to note it never fails the write that already happened.
    async fn record(&self, arguments: &Arguments, result: &Value) {
        let Some(chat_id) = self.scope.chat_id else {
            return;
        };
        let created = &result["pull_request"];
        let Some(url) = created["html_url"].as_str() else {
            return;
        };
        let opened = Opened {
            chat_id,
            workspace_id: self.scope.workspace_id,
            url: url.to_string(),
            title: created["title"].as_str().map(str::to_string),
            source_id: Some(arguments.source_id),
        };
        if let Err(error) = chat_pull_request::record(&self.scope.state, &opened).await {
            tracing::warn!(chat = %chat_id, %error, "could not record the pull request a chat opened");
        }
    }
}

/// What GitHub made of a request to open a pull request.
enum Creation {
    Created(Value),
    Refused { head: String, base: String },
}

pub struct Github {
    service: PullRequestService,
    repository: Repository,
    token: SecretValue,
    log_bytes: u64,
    signals: ReviewSignals,
    configuration: Configuration,
}

impl Github {
    /// This source's client against the GitHub origin the server is
    /// configured for.
    pub(crate) fn new(configuration: Configuration, config: &Config) -> Result<Self, String> {
        let service = config.pull_requests().map_err(|error| match error {
            PullRequestError::InvalidOrigin => INVALID_ORIGIN.to_string(),
            _ => CLIENT_UNAVAILABLE.to_string(),
        })?;
        Self::serving(configuration, service)
    }

    fn serving(configuration: Configuration, service: PullRequestService) -> Result<Self, String> {
        if !segment(&configuration.owner) || !segment(&configuration.repo) {
            return Err(INVALID_REPOSITORY.to_string());
        }
        let signals = match &configuration.review_signals {
            Some(names) => ReviewSignals::select(names)?,
            None => ReviewSignals::recognized(),
        };
        let token = credential(configuration.token.clone());
        let repository = Repository::new(&configuration.owner, &configuration.repo)
            .map_err(|_| INVALID_REPOSITORY.to_string())?;
        Ok(Self {
            service: service
                .with_anonymous_reads()
                .with_retry_after(MAX_RETRY_AFTER)
                .with_api_version(API_VERSION),
            repository,
            token,
            log_bytes: MAX_LOG_BYTES,
            signals,
            configuration,
        })
    }

    fn reference(&self, number: u64) -> Result<PullRequestReference, String> {
        let number = NonZeroU64::new(number).ok_or(NUMBER_NOT_POSITIVE)?;
        Ok(PullRequestReference::new(self.repository.clone(), number))
    }

    fn issue_reference(&self, number: u64) -> Result<IssueReference, String> {
        let number = NonZeroU64::new(number).ok_or(NUMBER_NOT_POSITIVE)?;
        Ok(IssueReference::new(self.repository.clone(), number))
    }

    /// The `owner/repo` name GitHub reports records under.
    fn full_name(&self) -> String {
        format!("{}/{}", self.configuration.owner, self.configuration.repo)
    }

    async fn write(&self, operation: Operation, arguments: &Arguments) -> Result<Value, String> {
        let repository = format!(
            "https://github.com/{}/{}",
            self.configuration.owner, self.configuration.repo
        );
        match operation {
            Operation::CreatePull => match self.create_pull(arguments).await? {
                Creation::Created(created) => Ok(json!({
                    "repository": repository,
                    "pull_request": created,
                })),
                Creation::Refused { .. } => Err(status_error(UNPROCESSABLE_ENTITY)),
            },
            Operation::Comment => {
                let reference = self.issue_reference(arguments.number.ok_or(NUMBER_REQUIRED)?)?;
                let body = arguments
                    .body
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or(BODY_REQUIRED)?;
                let created = self
                    .service
                    .post_comment(&reference, &self.token, body)
                    .await
                    .map_err(described)?;
                Ok(json!({
                    "repository": repository,
                    "comment": {
                        "id": created.id,
                        "html_url": created.url,
                        "body": created.body,
                        "created_at": created.created_at,
                    },
                }))
            }
            _ => Err("This operation is read-only.".to_string()),
        }
    }

    /// The title, head and base a pull request is opened with, refused before
    /// anything is pushed or posted when any is missing or malformed.
    fn pull_fields<'a>(
        &self,
        arguments: &'a Arguments,
    ) -> Result<(&'a str, &'a str, String), String> {
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
            return Err(HEAD_INVALID.to_string());
        }
        let base = arguments
            .base
            .clone()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| self.configuration.branch.clone())
            .ok_or("base is required (or set the source branch).")?;
        if !git_ref(&base) {
            return Err(BASE_INVALID.to_string());
        }
        Ok((title, head, base))
    }

    /// Opens the pull request, or reports that GitHub refused it because one
    /// is already open from the same head.
    async fn create_pull(&self, arguments: &Arguments) -> Result<Creation, String> {
        let (title, head, base) = self.pull_fields(arguments)?;
        let body = arguments.body.as_deref().unwrap_or_default();
        let (fork, branch) = forked(head);
        let branch = BranchName::parse(branch).map_err(|_| HEAD_INVALID.to_string())?;
        let target = BranchName::parse(&base).map_err(|_| BASE_INVALID.to_string())?;
        let created = match fork {
            Some(fork) => {
                self.service
                    .create_pull_request_from(
                        &self.repository,
                        &self.token,
                        fork,
                        &branch,
                        &target,
                        title,
                        body,
                        false,
                    )
                    .await
            }
            None => {
                self.service
                    .create_pull_request(
                        &self.repository,
                        &self.token,
                        &branch,
                        &target,
                        title,
                        body,
                        false,
                    )
                    .await
            }
        };
        match created {
            Ok(created) => Ok(Creation::Created(json!({
                "number": created.number.get(),
                "html_url": created.url,
                "title": title,
                "state": created.state,
            }))),
            Err(PullRequestError::PullRequestAlreadyExists(_)) => Ok(Creation::Refused {
                head: head.to_string(),
                base,
            }),
            Err(error) => Err(described(error)),
        }
    }

    /// The open pull request from `head` into `base`, which GitHub refusing
    /// a duplicate says exists. A closed or merged one from the same head is
    /// not an answer to a request to open one.
    async fn already_open(&self, head: &str, base: &str) -> Option<Value> {
        let (fork, branch) = forked(head);
        let branch = BranchName::parse(branch).ok()?;
        let base = BranchName::parse(base).ok()?;
        let found = self
            .service
            .find_pull_request(&self.repository, &self.token, &branch, &base, fork)
            .await
            .ok()??;
        (found.state == PullRequestState::Open).then(|| {
            json!({
                "number": found.reference.number().get(),
                "html_url": found.url,
                "title": found.title,
                "state": found.state,
            })
        })
    }

    async fn resolve(&self, reference: Option<&str>) -> Result<(String, String), String> {
        let reference = match reference.or(self.configuration.branch.as_deref()) {
            Some(value) if !value.trim().is_empty() => value.to_string(),
            Some(_) => return Err("ref must not be empty.".to_string()),
            None => self
                .service
                .get_default_branch(&self.repository, &self.token)
                .await
                .map_err(|error| match error {
                    PullRequestError::Parse(_) => MISSING_DEFAULT_BRANCH.to_string(),
                    error => described(error),
                })?
                .to_string(),
        };
        let sha = self
            .service
            .resolve_commit(&self.repository, &self.token, &reference)
            .await
            .map_err(|error| match error {
                PullRequestError::Parse(ParseError::CommitSha(_)) => MISSING_COMMIT.to_string(),
                PullRequestError::Parse(ParseError::BranchName(_))
                | PullRequestError::InvalidRepositoryUrl => INVALID_REFERENCE.to_string(),
                error => described(error),
            })?;
        Ok((reference, sha.to_string()))
    }

    /// The one call a wait on GitHub checks polls: resolve the reference once,
    /// then read the checks on the immutable commit it resolved to.
    ///
    /// Only `SETTLED_ASSESSMENTS` end a wait. `pending` has not finished, and
    /// `unknown` is a commit GitHub has registered no run for at all, which
    /// right after a push is indistinguishable from a repository with no CI —
    /// so how long to tolerate it is the caller's decision, not this call's.
    /// A run that completed neutral or skipped is neither: it reported and it
    /// is over, so it settles under its own name rather than waiting out a
    /// grace period meant for silence.
    pub async fn settled(
        &self,
        reference: Option<&str>,
    ) -> Result<(String, String, &'static str), String> {
        let (reference, sha) = self.resolve(reference).await?;
        let (workflows, checks, statuses, complete) = self.ci_rows(&sha).await?;
        let state = assessed(&conclusions(&workflows, &checks, &statuses), complete);
        Ok((reference, sha, state))
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
        if matches!(operation, Operation::ReviewThreads) {
            let mut result = self.review_threads(arguments).await?;
            result["repository"] = json!(repository);
            return Ok(result);
        }
        if matches!(operation, Operation::ReleasePipelines) {
            let mut result = self.release_pipelines(arguments).await?;
            result["repository"] = json!(repository);
            return Ok(result);
        }
        if matches!(operation, Operation::Issues) {
            let mut result = self.issues(arguments).await?;
            result["repository"] = json!(repository);
            return Ok(result);
        }
        if matches!(operation, Operation::Issue) {
            let number = arguments
                .number
                .filter(|number| *number > 0)
                .ok_or(NUMBER_REQUIRED)?;
            let mut result = self.issue(number).await?;
            result["repository"] = json!(repository);
            return Ok(result);
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
            | Operation::Issue
            | Operation::PullRequests
            | Operation::ReviewThreads
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

    async fn issues(&self, arguments: &Arguments) -> Result<Value, String> {
        let state = state_filter(arguments.state.as_deref()).ok_or(ISSUE_STATE_INVALID)?;
        let page = page(arguments.page.unwrap_or(1))?;
        let listed = self
            .service
            .list_issues(&self.repository, &self.token, state, page, PER_PAGE)
            .await
            .map_err(described)?;
        let issues: Vec<Value> = listed.items.iter().map(issue_record).collect();
        Ok(json!({"issues": issues, "next_page": listed.next.map(NonZeroU32::get)}))
    }

    /// One issue with its whole body and first page of comments. The list
    /// endpoint bounds bodies so a page of issues fits the context; a single
    /// issue is what a person asks for when they want the full text.
    async fn issue(&self, number: u64) -> Result<Value, String> {
        let reference = self.issue_reference(number)?;
        let issue = self
            .service
            .fetch_issue(&reference, &self.token)
            .await
            .map_err(described)?;
        if issue.pull_request {
            return Err(format!(
                "#{number} is a pull request, not an issue; assess_pull_requests reviews it."
            ));
        }
        let comments = self
            .service
            .list_comments(&reference, &self.token, NonZeroU32::MIN, PER_PAGE)
            .await
            .map_err(described)?;
        let mut record = issue_record(&issue);
        record["body"] = json!(issue.body);
        record["comments"] = json!(issue.comments);
        record["author"] = json!(issue.author);
        Ok(json!({
            "issue": record,
            "comments": comments.items.iter().map(|comment| json!({
                "id": comment.id,
                "body": comment.body,
                "html_url": comment.url,
                "created_at": comment.created_at,
                "author": comment.author,
            })).collect::<Vec<_>>(),
            "comments_complete": comments.next.is_none(),
        }))
    }

    async fn ci_rows(
        &self,
        sha: &str,
    ) -> Result<(Vec<WorkflowRun>, Vec<CheckRun>, Vec<CommitStatus>, bool), String> {
        let commit = revision(sha)?;
        let filter = WorkflowRunFilter::new().with_head_sha(commit.clone());
        let (workflows, checks, statuses) = tokio::join!(
            self.service
                .collect_workflow_runs(&self.repository, &self.token, &filter),
            self.service
                .collect_check_runs(&self.repository, &self.token, &commit),
            self.service
                .collect_statuses(&self.repository, &self.token, &commit),
        );
        let workflows = workflows.map_err(ci_described)?;
        if !workflows.complete {
            return Err(WORKFLOW_SEARCH_LIMIT.to_string());
        }
        let checks = checks.map_err(ci_described)?;
        let statuses = statuses.map_err(ci_described)?;
        if statuses
            .items
            .iter()
            .any(|status| status.context.is_empty())
        {
            return Err(UNIDENTIFIED_CI.to_string());
        }
        let mut identities = std::collections::HashSet::new();
        let workflows: Vec<WorkflowRun> = workflows
            .items
            .into_iter()
            .filter(|run| {
                identities.insert((run.workflow_id, run.event.clone(), run.head_branch.clone()))
            })
            .collect();
        if workflows.iter().any(|run| run.head_sha != commit)
            || checks
                .items
                .iter()
                .any(|check| check.head_sha.as_ref() != Some(&commit))
        {
            return Err(FOREIGN_CHECKS.to_string());
        }
        let complete = checks.complete && statuses.complete;
        Ok((workflows, checks.items, statuses.items, complete))
    }

    async fn build(&self, sha: &str) -> Result<Value, String> {
        let (workflows, checks, statuses, complete) = self.ci_rows(sha).await?;
        let state = assessed(&conclusions(&workflows, &checks, &statuses), complete);
        Ok(bound_build(json!({
            "state": state,
            "complete": complete,
            "assessment": BUILD_ASSESSMENT,
            "workflows": workflows.iter().map(workflow_record).collect::<Vec<_>>(),
            "checks": checks.iter().map(check_record).collect::<Vec<_>>(),
            "statuses": statuses.iter().map(status_record).collect::<Vec<_>>(),
        })))
    }

    async fn check_evidence(&self, sha: &str) -> Result<CheckEvidence, String> {
        let (workflows, checks, statuses, complete) = self.ci_rows(sha).await?;
        Ok(CheckEvidence {
            complete,
            ..tally(conclusions(&workflows, &checks, &statuses).into_iter())
        })
    }

    async fn readiness(&self, arguments: &Arguments) -> Result<Value, String> {
        let (rows, next) = match arguments.number {
            Some(number) => {
                let reference = self.reference(number)?;
                let detail = self.pull(&reference).await?;
                (vec![(listed(reference, &detail), Some(detail))], None)
            }
            None => {
                let state = state_filter(arguments.state.as_deref()).ok_or(PULL_STATE_INVALID)?;
                let page = page(arguments.page.unwrap_or(1))?;
                let listed = self
                    .service
                    .list_pull_requests(&self.repository, &self.token, state, page, PULL_PAGE_SIZE)
                    .await
                    .map_err(described)?;
                (
                    listed.items.into_iter().map(|row| (row, None)).collect(),
                    listed.next.map(NonZeroU32::get),
                )
            }
        };
        let mut assessed = Vec::with_capacity(rows.len());
        for (row, detail) in rows {
            let (assessment, risk) = self.assess(&row, detail).await?;
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

    /// One pull request's readiness. `detail` is the pull request as read by
    /// number, when it already was; a listed row is read again by number, and
    /// a head the two reads disagree on is never verified.
    async fn assess(
        &self,
        row: &ListedPullRequest,
        detail: Option<PullRequestDetail>,
    ) -> Result<(readiness::Assessment, RiskSignal), String> {
        let detail = match detail {
            Some(detail) => detail,
            None => self.pull(&row.reference).await?,
        };
        let review = self.review(&detail).await?;
        let head = row
            .head_sha
            .as_ref()
            .and_then(|listed| CommitSha::parse(listed.as_str()))
            .filter(|listed| review.head.as_ref() == Some(listed));
        let checks = match &head {
            Some(head) => self.check_evidence(head.as_str()).await?,
            None => CheckEvidence::default(),
        };
        let risk = pull_request::risk(&PRIORITISER, &self.changed_paths(&row.reference).await);
        Ok((
            readiness::assess(
                PullEvidence {
                    number: row.reference.number().get(),
                    title: truncate(&row.title, PULL_TITLE_CHARS),
                    url: row.url.clone(),
                    updated_at: row.updated_at.clone().unwrap_or_default(),
                    draft: row.draft || review.draft,
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
    async fn changed_paths(&self, reference: &PullRequestReference) -> Vec<String> {
        let mut paths = Vec::new();
        let mut page = NonZeroU32::MIN;
        loop {
            let Ok(listed) = self
                .service
                .list_files(reference, &self.token, page, PER_PAGE)
                .await
            else {
                return Vec::new();
            };
            for file in listed.items {
                if file.filename.trim().is_empty() {
                    return Vec::new();
                }
                paths.push(file.filename);
            }
            match listed.next {
                None => return paths,
                Some(next) if next.get() <= PULL_FILE_PAGES => page = next,
                Some(_) => return Vec::new(),
            }
        }
    }

    /// Every review thread on the pull request, with what each reviewer wrote.
    async fn review_threads(&self, arguments: &Arguments) -> Result<Value, String> {
        if arguments.reference.is_some() {
            return Err("ref does not apply: review threads belong to the pull request and are read as they stand now, not at a commit.".into());
        }
        let number = arguments
            .number
            .filter(|number| *number > 0)
            .ok_or("number is required: name the pull request to read.")?;
        if number > i32::MAX as u64 {
            return Err(format!(
                "{number} is larger than any pull request number GitHub assigns."
            ));
        }
        if let Some(comment) = arguments.comment {
            if arguments.after.is_some() {
                return Err(
                    "after pages the threads; with comment, page its body with offset.".into(),
                );
            }
            return self
                .whole_comment(number, comment, arguments.offset.unwrap_or(0))
                .await;
        }
        if arguments.offset.is_some() {
            return Err("offset only applies with comment; page the threads with after.".into());
        }
        let wanted = arguments.resolved.unwrap_or(false);
        let Collected {
            items: threads,
            complete,
            ..
        } = self.all_review_threads(number).await?;
        let unresolved = threads.iter().filter(|thread| !thread.resolved).count();
        let kept: Vec<(usize, &ReviewThreadRecord)> = threads
            .iter()
            .enumerate()
            .filter(|(_, thread)| thread.resolved == wanted)
            .collect();
        let start = match arguments.after {
            None => (0, 0),
            Some(id) => resume_after(&threads, &kept, id)?,
        };
        let kept: Vec<&ReviewThreadRecord> = kept.into_iter().map(|(_, thread)| thread).collect();
        let mut page = json!({
            "assessment": REVIEW_THREADS_ASSESSMENT,
            "number": number,
            "complete": complete,
            "reading": if wanted { "resolved" } else { "unresolved" },
            "counts": {"unresolved": unresolved, "resolved": threads.len() - unresolved},
            "after": arguments.after,
            "next_after": u64::MAX,
            "threads": [],
        });
        let (rows, next) = review_thread_page(&kept, start, json_chars(&page))?;
        page["threads"] = json!(rows);
        page["next_after"] = json!(next);
        Ok(page)
    }

    async fn all_review_threads(
        &self,
        number: u64,
    ) -> Result<Collected<ReviewThreadRecord>, String> {
        self.service
            .collect_review_threads(&self.reference(number)?, &self.token)
            .await
            .map_err(thread_described)
    }

    /// One review comment's whole body, for a comment `read_review_threads`
    /// had to cut. Read over REST by id, and refused unless it belongs to the
    /// pull request being read.
    async fn whole_comment(&self, number: u64, id: u64, offset: u64) -> Result<Value, String> {
        let comment = self
            .service
            .fetch_review_comment(&self.reference(number)?, &self.token, id)
            .await
            .map_err(comment_described)?;
        let body = comment.body.as_str();
        let total = body.chars().count() as u64;
        if offset > total {
            return Err("offset is past the end of the comment.".into());
        }
        let mut page = json!({
            "assessment": REVIEW_THREADS_ASSESSMENT,
            "number": number,
            "comment": id,
            "author": author(&comment.author),
            "path": comment.path,
            "url": comment.url,
            "body": "",
            "offset": offset,
            "next": total,
            "total": total,
            "complete": false,
        });
        let room = MAX_TOOL_MESSAGE_CHARS
            .saturating_sub(RESULT_ENVELOPE_CHARS)
            .saturating_sub(json_chars(&page));
        let rest = &body[byte_at(body, offset as usize)..];
        let taken = escaped_fit(rest, room).min(FILE_PAGE_CHARS as usize);
        let end = offset + taken as u64;
        let next = (end < total).then_some(end);
        page["body"] = json!(&rest[..byte_at(rest, taken)]);
        page["next"] = json!(next);
        page["complete"] = json!(next.is_none());
        Ok(page)
    }

    async fn pull(&self, reference: &PullRequestReference) -> Result<PullRequestDetail, String> {
        let detail = self
            .service
            .fetch_pull(reference, &self.token)
            .await
            .map_err(described)?;
        if detail.number != reference.number() {
            return Err(MISMATCHED_PULL.to_string());
        }
        Ok(detail)
    }

    /// The tree entry `path` names at `commit`, walked one tree at a time so
    /// every entry passed on the way is checked: a symbolic link or a
    /// submodule anywhere along it refuses the read.
    async fn entry(&self, commit: &Revision, path: &str) -> Result<TreeEntry, String> {
        let mut tree = ObjectId::from(commit);
        let mut components = path.split('/').peekable();
        while let Some(component) = components.next() {
            let listed = self
                .service
                .fetch_tree(&self.repository, &self.token, &tree, false)
                .await
                .map_err(|error| match error {
                    PullRequestError::Parse(_) => INVALID_OBJECT.to_string(),
                    error => described(error),
                })?;
            if listed.truncated {
                return Err(INCOMPLETE_TREE.to_string());
            }
            let entry = listed
                .entries
                .into_iter()
                .find(|entry| entry.path == component)
                .ok_or(MISSING_PATH)?;
            let last = components.peek().is_none();
            if (last && !entry.regular_file()) || (!last && entry.kind != TreeEntryKind::Tree) {
                return Err(IRREGULAR_FILE.to_string());
            }
            if last {
                return Ok(entry);
            }
            tree = entry.sha;
        }
        Err(MISSING_PATH.to_string())
    }

    async fn review(&self, detail: &PullRequestDetail) -> Result<Review, String> {
        let reference = self.reference(detail.number.get())?;
        let counts = self
            .service
            .fetch_review_thread_counts(&reference, &self.token)
            .await
            .map_err(thread_described)?;
        Ok(Review {
            draft: detail.draft,
            head: CommitSha::parse(detail.head_sha.as_str()),
            threads: thread_evidence(counts),
            comments: self
                .review_comments(&IssueReference::from(&reference))
                .await?,
        })
    }

    /// The newest of a pull request's conversation comments, where review
    /// bots post their summaries, read no further than one page past the
    /// most a readiness verdict weighs, so a conversation longer than that
    /// reads as incomplete rather than as all of it.
    async fn review_comments(&self, reference: &IssueReference) -> Result<CommentEvidence, String> {
        let first = self
            .service
            .list_comments(reference, &self.token, NonZeroU32::MIN, PER_PAGE)
            .await
            .map_err(described)?;
        let mut comments = first.items;
        let complete = match first.next {
            None => true,
            Some(next) => {
                let rest = self
                    .service
                    .list_comments(reference, &self.token, next, PER_PAGE)
                    .await
                    .map_err(described)?;
                let complete = rest.items.is_empty();
                comments.extend(rest.items);
                complete
            }
        };
        let excess = comments.len().saturating_sub(REVIEW_PAGE_SIZE);
        Ok(CommentEvidence {
            complete,
            comments: comments
                .into_iter()
                .skip(excess)
                .map(issue_comment)
                .collect(),
        })
    }

    async fn release_pipelines(&self, arguments: &Arguments) -> Result<Value, String> {
        let (listed, next) = match arguments.tag.as_deref() {
            Some(tag) => {
                if !segment(tag) {
                    return Err(INVALID_TAG.to_string());
                }
                let release = self
                    .service
                    .fetch_release(&self.repository, &self.token, tag)
                    .await
                    .map_err(|error| match error {
                        PullRequestError::Parse(ParseError::Tag(_))
                        | PullRequestError::InvalidRepositoryUrl => INVALID_TAG.to_string(),
                        error => described(error),
                    })?;
                (vec![release], None)
            }
            None => {
                let listed = self
                    .service
                    .list_releases(
                        &self.repository,
                        &self.token,
                        page(arguments.page.unwrap_or(1))?,
                        RELEASE_PAGE_SIZE,
                    )
                    .await
                    .map_err(described)?;
                (listed.items, listed.next)
            }
        };
        let releases: Vec<ReleaseIdentity> = listed
            .iter()
            .map(|release| self.release_identity(release))
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
                "next_page": next.map(NonZeroU32::get),
                "complete": next.is_none(),
                "assessment": RELEASE_ASSESSMENT,
            }),
            &assessed,
            &checked_at,
        ))
    }

    fn release_identity(&self, release: &Release) -> Result<ReleaseIdentity, String> {
        let identity = ReleaseIdentity {
            repository: self.full_name(),
            id: release.id,
            tag: release.tag.clone(),
            published_at: release.published_at.clone().unwrap_or_default(),
            url: release.url.clone(),
        };
        identity
            .valid()
            .then_some(identity)
            .ok_or_else(|| UNUSABLE_RELEASE.to_string())
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
        self.pipeline_runs(WorkflowRunFilter::new().with_created(format!(">={earliest}")))
            .await
            .ok()
    }

    /// The runs GitHub itself attributes to one release's tag.
    async fn exact_pipeline(&self, release: &ReleaseIdentity, checked_at: &str) -> ReleasePipeline {
        let runs = match BranchName::parse(&release.tag) {
            Ok(branch) => {
                self.pipeline_runs(WorkflowRunFilter::new().with_branch(branch))
                    .await
            }
            Err(_) => Err(INVALID_TAG.to_string()),
        };
        match runs {
            Ok(records) => ReleasePipeline::observe(release.clone(), &records, checked_at),
            Err(_) => ReleasePipeline::unavailable(release.clone(), checked_at),
        }
    }

    async fn pipeline_runs(&self, filter: WorkflowRunFilter) -> Result<Vec<RunEvidence>, String> {
        let runs = self
            .service
            .collect_workflow_runs(
                &self.repository,
                &self.token,
                &filter.with_event(RELEASE_EVENT),
            )
            .await
            .map_err(described)?;
        if !runs.complete {
            return Err(WORKFLOW_SEARCH_LIMIT.to_string());
        }
        Ok(runs.items.iter().map(run_evidence).collect())
    }

    async fn deployments(&self, sha: &str, number: u32) -> Result<Value, String> {
        let commit = revision(sha)?;
        let listed = self
            .service
            .list_deployments(
                &self.repository,
                &self.token,
                &commit,
                page(number)?,
                PER_PAGE,
            )
            .await
            .map_err(|error| match error {
                PullRequestError::Parse(_) => FOREIGN_DEPLOYMENT.to_string(),
                error => described(error),
            })?;
        if listed
            .items
            .iter()
            .any(|deployment| deployment.sha != commit)
        {
            return Err(FOREIGN_DEPLOYMENT.to_string());
        }
        let deployments: Vec<Value> = listed
            .items
            .iter()
            .map(|deployment| {
                json!({
                    "id": deployment.id,
                    "sha": deployment.sha.as_str(),
                    "ref": deployment.reference,
                    "environment": deployment.environment,
                    "created_at": deployment.created_at,
                    "updated_at": deployment.updated_at,
                    "status": deployment.status.as_ref().map_or_else(|| json!({}), |status| json!({
                        "state": status.state,
                        "description": status.description,
                        "environment_url": status.environment_url,
                        "log_url": status.log_url,
                        "created_at": status.created_at,
                    })),
                    "url": deployment.api_url,
                })
            })
            .collect();
        Ok(json!({"deployments": deployments, "next_page": listed.next.map(NonZeroU32::get)}))
    }

    async fn file(&self, sha: &str, path: &str, offset: u64, limit: u64) -> Result<Value, String> {
        validate_path(path, self.configuration.path.as_deref())?;
        let entry = self.entry(&revision(sha)?, path).await?;
        let size = entry.size.ok_or(SIZE_MISMATCH)?;
        if size > MAX_FILE_BYTES {
            return Err(FILE_TOO_LARGE.to_string());
        }
        let blob = self
            .service
            .fetch_blob(
                &self.repository,
                &self.token,
                &entry.sha,
                MAX_FILE_BYTES as usize,
            )
            .await
            .map_err(|error| match error {
                PullRequestError::Http(_) => UNREADABLE_FILE.to_string(),
                PullRequestError::Parse(_) => INVALID_OBJECT.to_string(),
                error => described(error),
            })?;
        let bytes = blob.content;
        if bytes.len() as u64 != size {
            return Err(SIZE_MISMATCH.to_string());
        }
        let content = std::str::from_utf8(&bytes).map_err(|_| NOT_TEXT.to_string())?;
        let (page, total, next) = text_page(content, offset, limit)?;
        Ok(
            json!({"path": path, "content": page, "bytes": bytes.len(), "blob_sha": entry.sha.as_str(),
            "offset": offset, "next": next, "total": total, "complete": next.is_none(),
            "url": format!("https://github.com/{}/{}/blob/{}/{}", self.configuration.owner, self.configuration.repo, sha, path.split('/').map(|part| urlencoding::encode(part).into_owned()).collect::<Vec<_>>().join("/"))}),
        )
    }
    async fn check_logs(&self, arguments: &Arguments) -> Result<Value, String> {
        let job_id = arguments.job_id.ok_or("job_id is required.")?;
        let job_id = NonZeroU64::new(job_id).ok_or("job_id must be positive.")?;
        let (reference, sha, pull) = self.log_target(arguments).await?;
        let job = self
            .service
            .fetch_workflow_job(&self.repository, &self.token, job_id)
            .await
            .map_err(|error| match error {
                PullRequestError::Parse(_) => JOB_WITHOUT_COMMIT.to_string(),
                error => described(error),
            })?;
        let run_id = self.verify_job(&job, job_id.get(), &sha)?;
        let downloaded = self
            .service
            .fetch_job_log(
                &self.repository,
                &self.token,
                job_id,
                self.log_bytes as usize,
            )
            .await
            .map_err(described)?;
        let lines = log_lines(&String::from_utf8_lossy(&downloaded.content));
        let log = JobLog {
            job,
            run_id,
            sha,
            reference,
            pull,
            downloaded: downloaded.content.len(),
            capped: downloaded.truncated,
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
        let reference = self.reference(number)?;
        let pull = self
            .service
            .fetch_pull(&reference, &self.token)
            .await
            .map_err(|error| match error {
                PullRequestError::Parse(_) => MISSING_PULL_HEAD.to_string(),
                error => described(error),
            })?;
        if pull.number != reference.number() {
            return Err(DIFFERENT_PULL.to_string());
        }
        Ok((
            pull.head.to_string(),
            pull.head_sha.to_string(),
            Some(json!({
                "number": pull.number,
                "title": pull.title,
                "state": pull.state,
                "draft": pull.draft,
                "html_url": pull.url,
            })),
        ))
    }

    fn verify_job(&self, job: &WorkflowJob, job_id: u64, sha: &str) -> Result<u64, String> {
        let run_id = job.run_id.get();
        if job.id.get() != job_id
            || !job.head_sha.as_str().eq_ignore_ascii_case(sha)
            || !is_job_url(
                job.url.as_deref().unwrap_or_default(),
                &self.configuration.owner,
                &self.configuration.repo,
                run_id,
                job_id,
            )
        {
            return Err(FOREIGN_JOB.to_string());
        }
        Ok(run_id)
    }
}

struct JobLog {
    job: WorkflowJob,
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
            "job": {
                "id": self.job.id,
                "run_id": self.job.run_id,
                "name": self.job.name,
                "status": self.job.status,
                "conclusion": self.job.conclusion,
                "html_url": self.job.url,
                "started_at": self.job.started_at,
                "completed_at": self.job.completed_at,
            },
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
        let conclusion = if self.job.status == COMPLETED_STATUS {
            self.job.conclusion.as_deref().unwrap_or(UNKNOWN_ASSESSMENT)
        } else {
            PENDING_ASSESSMENT
        };
        json!({
            "kind": "github_build",
            "title": format!("{} job log excerpt", self.job.name),
            "url": self.job.url.as_deref().unwrap_or_default(),
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
    value.eq_ignore_ascii_case(&format!(
        "https://github.com/{owner}/{repo}/actions/runs/{run_id}/job/{job_id}"
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

/// The threads from `start` (a thread, and how many of its comments to skip)
/// onwards, cut where the next comment would take the page past the tool
/// message budget. At least one comment is always returned so a caller
/// following `next_after` always moves forward.
fn review_thread_page(
    threads: &[&ReviewThreadRecord],
    (start, mut first): (usize, usize),
    mut used: usize,
) -> Result<(Vec<Value>, Option<u64>), String> {
    let budget = MAX_TOOL_MESSAGE_CHARS.saturating_sub(RESULT_ENVELOPE_CHARS);
    let mut rows = Vec::new();
    let mut last = None;
    for thread in &threads[start..] {
        let comments = &thread.comments;
        let skip = std::mem::take(&mut first);
        if skip >= comments.len() {
            continue;
        }
        let (line, start_line) = located(thread);
        let mut row = json!({
            "path": thread.path,
            "line": line,
            "start_line": start_line,
            "side": side(thread.side),
            "resolved": thread.resolved,
            "outdated": thread.outdated,
            "first_comment": skip,
            "comments_complete": thread.comments_complete,
            "comments": [],
        });
        let mut cost = json_chars(&row) + 1;
        let mut wrote = Vec::new();
        for comment in &comments[skip..] {
            let id = comment
                .database_id
                .ok_or("GitHub returned a review comment without an id.")?;
            let entry = review_comment_row(id, comment);
            cost += json_chars(&entry) + 1;
            if used + cost > budget && last.is_some() {
                if !wrote.is_empty() {
                    row["comments"] = json!(wrote);
                    rows.push(row);
                }
                return Ok((rows, last));
            }
            wrote.push(entry);
            last = Some(id);
        }
        used += cost;
        row["comments"] = json!(wrote);
        rows.push(row);
    }
    Ok((rows, None))
}

/// Where the page after comment `id` starts in `kept`, which holds threads
/// in GitHub's order with their index there. The comment is found among
/// every thread, so a thread resolved or reopened since the last page still
/// marks the place: reading resumes at the next kept thread after it.
fn resume_after(
    threads: &[ReviewThreadRecord],
    kept: &[(usize, &ReviewThreadRecord)],
    id: u64,
) -> Result<(usize, usize), String> {
    let (thread, position) = threads
        .iter()
        .enumerate()
        .find_map(|(index, thread)| {
            thread
                .comments
                .iter()
                .position(|comment| comment.database_id == Some(id))
                .map(|position| (index, position))
        })
        .ok_or_else(|| {
            format!("Comment {id} is no longer among these threads: it may have been deleted. Read again without after.")
        })?;
    if let Some(at) = kept.iter().position(|(index, _)| *index == thread) {
        return Ok((at, position + 1));
    }
    Ok((kept.partition_point(|(index, _)| *index < thread), 0))
}

/// An outdated thread no longer maps onto the current diff, so GitHub nulls
/// its current lines; the lines it was written against still locate it.
fn located(thread: &ReviewThreadRecord) -> (Option<u32>, Option<u32>) {
    match thread.line {
        None => (thread.original_line, thread.original_start_line),
        line => (line, thread.start_line),
    }
}

fn side(side: DiffSide) -> Value {
    match side {
        DiffSide::Unknown => Value::Null,
        side => json!(side.as_str()),
    }
}

/// How many leading characters of `text` fit in `budget` once serialized as a
/// JSON string, so a body of newlines or quotes cannot double past the cap.
fn escaped_fit(text: &str, budget: usize) -> usize {
    let mut cost = 0;
    for (count, character) in text.chars().enumerate() {
        cost += match character {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
            control if control < ' ' => 6,
            _ => 1,
        };
        if cost > budget {
            return count;
        }
    }
    text.chars().count()
}

fn review_comment_row(id: u64, comment: &ThreadComment) -> Value {
    let kept = escaped_fit(&comment.body, REVIEW_BODY_CHARS);
    json!({
        "id": id,
        "author": author(&comment.author),
        "body": truncate(&comment.body, kept),
        "truncated": kept < comment.body.chars().count(),
        "url": comment.url,
        "created_at": comment.created_at,
    })
}

fn author(login: &str) -> &str {
    if login.is_empty() {
        UNKNOWN_AUTHOR
    } else {
        login
    }
}

fn byte_at(text: &str, characters: usize) -> usize {
    text.char_indices()
        .nth(characters)
        .map_or(text.len(), |(index, _)| index)
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
    let branch = match value.split_once(':') {
        Some((owner, branch)) if segment(owner) => branch,
        Some(_) => return false,
        None => value,
    };
    BranchName::parse(branch).is_ok()
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

fn forked(head: &str) -> (Option<&str>, &str) {
    match head.split_once(':') {
        Some((owner, branch)) => (Some(owner), branch),
        None => (None, head),
    }
}

fn state_filter(state: Option<&str>) -> Option<StateFilter> {
    match state.unwrap_or("open") {
        "open" => Some(StateFilter::Open),
        "closed" => Some(StateFilter::Closed),
        "all" => Some(StateFilter::All),
        _ => None,
    }
}

fn listed(reference: PullRequestReference, detail: &PullRequestDetail) -> ListedPullRequest {
    let row = ListedPullRequest::new(
        reference,
        detail.node_id.clone(),
        detail.url.clone(),
        detail.head.clone(),
        detail.base.clone(),
    )
    .with_title(detail.title.clone())
    .with_state(detail.state)
    .with_draft(detail.draft)
    .with_head_sha(detail.head_sha.clone());
    match &detail.updated_at {
        Some(updated_at) => row.with_updated_at(updated_at.clone()),
        None => row,
    }
}

fn thread_evidence(counts: ReviewThreadCount) -> ThreadEvidence {
    let open = std::iter::repeat_n(ReviewThread { resolved: false }, counts.open as usize);
    let resolved = std::iter::repeat_n(ReviewThread { resolved: true }, counts.resolved as usize);
    ThreadEvidence {
        complete: counts.complete,
        threads: open.chain(resolved).collect(),
    }
}

fn issue_comment(comment: IssueComment) -> ReviewComment {
    ReviewComment {
        id: comment.id,
        author: comment.author,
        body: comment.body,
        url: comment.url,
        created_at: comment.created_at,
    }
}

fn labels(labels: &[Label]) -> Vec<Value> {
    labels
        .iter()
        .map(|label| {
            json!({
                "name": label.name,
                "color": label.color,
                "description": label.description,
            })
        })
        .collect()
}

fn credential(token: Option<String>) -> SecretValue {
    SecretValue::new(token.unwrap_or_default())
}

fn page(number: u32) -> Result<NonZeroU32, String> {
    NonZeroU32::new(number).ok_or_else(|| PAGE_NOT_POSITIVE.to_string())
}

fn revision(sha: &str) -> Result<Revision, String> {
    Revision::parse(sha).map_err(|_| MISSING_COMMIT.to_string())
}

/// What a crate error says in Zone's words. GitHub's own words in
/// `GitHubApi` and `Refused` can quote the request, so they are never
/// repeated.
fn described(error: PullRequestError) -> String {
    match error {
        PullRequestError::QuotaExhausted { .. } => RATE_LIMIT_EXHAUSTED.to_string(),
        PullRequestError::AuthenticationFailed | PullRequestError::AuthenticationRequired => {
            status_error(UNAUTHORIZED)
        }
        PullRequestError::Forbidden => status_error(FORBIDDEN),
        PullRequestError::NotFound
        | PullRequestError::RepositoryNotFound
        | PullRequestError::PullRequestNotFound => status_error(NOT_FOUND),
        PullRequestError::RateLimited => status_error(TOO_MANY_REQUESTS),
        PullRequestError::PullRequestAlreadyExists(_) => status_error(UNPROCESSABLE_ENTITY),
        PullRequestError::Refused { status, .. } => status_error(status),
        PullRequestError::Http(_) => REQUEST_FAILED.to_string(),
        PullRequestError::JobLog(failure) => match failure {
            JobLogError::Undirected => LOG_UNDIRECTED.to_string(),
            JobLogError::UnexpectedHost => LOG_UNEXPECTED_HOST.to_string(),
            JobLogError::Unreachable => LOG_UNREACHABLE.to_string(),
            JobLogError::Refused(status) => {
                format!("The job log download returned HTTP {status}.")
            }
            JobLogError::Interrupted => LOG_INTERRUPTED.to_string(),
            _ => UNEXPECTED_RESPONSE.to_string(),
        },
        PullRequestError::TooLarge => FILE_TOO_LARGE.to_string(),
        PullRequestError::InvalidOrigin => INVALID_ORIGIN.to_string(),
        PullRequestError::InvalidRepositoryName | PullRequestError::InvalidRepositoryUrl => {
            INVALID_REPOSITORY.to_string()
        }
        PullRequestError::Parse(_) => INVALID_RESPONSE.to_string(),
        _ => UNEXPECTED_RESPONSE.to_string(),
    }
}

fn status_error(status: u16) -> String {
    format!(
        "GitHub returned HTTP {status}. Check source access, permissions, rate limits and the requested resource."
    )
}

fn thread_described(error: PullRequestError) -> String {
    match error {
        PullRequestError::PullRequestNotFound => MISSING_PULL.to_string(),
        PullRequestError::GitHubApi(message) if message == ANSWERED_ANOTHER_PULL => {
            MISMATCHED_PULL.to_string()
        }
        PullRequestError::GitHubApi(_)
        | PullRequestError::Forbidden
        | PullRequestError::RateLimited
        | PullRequestError::NotFound
        | PullRequestError::RepositoryNotFound => REVIEW_QUERY_REJECTED.to_string(),
        error => described(error),
    }
}

fn comment_described(error: PullRequestError) -> String {
    match error {
        PullRequestError::NotFound => FOREIGN_COMMENT.to_string(),
        PullRequestError::GitHubApi(message) if message == ANSWERED_ANOTHER_COMMENT => {
            FOREIGN_COMMENT.to_string()
        }
        error => described(error),
    }
}

fn ci_described(error: PullRequestError) -> String {
    match error {
        PullRequestError::Parse(_) => FOREIGN_CHECKS.to_string(),
        error => described(error),
    }
}

fn conclusions<'a>(
    workflows: &'a [WorkflowRun],
    checks: &'a [CheckRun],
    statuses: &'a [CommitStatus],
) -> Vec<&'a str> {
    workflows
        .iter()
        .map(run_token)
        .chain(checks.iter().map(check_token))
        .chain(statuses.iter().map(|status| status.state.as_str()))
        .collect()
}

/// A read that did not reach every check can still prove a failure, never a
/// pass or a settled non-pass.
fn assessed(conclusions: &[&str], complete: bool) -> &'static str {
    match assessment(conclusions) {
        FAILURE_ASSESSMENT => FAILURE_ASSESSMENT,
        _ if !complete => PENDING_ASSESSMENT,
        state => state,
    }
}

fn workflow_record(run: &WorkflowRun) -> Value {
    json!({
        "id": run.id,
        "name": run.name,
        "head_sha": run.head_sha.as_str(),
        "status": run.status,
        "conclusion": run.conclusion,
        "html_url": run.url,
        "updated_at": run.updated_at,
    })
}

fn check_record(check: &CheckRun) -> Value {
    json!({
        "id": check.id,
        "name": check.name,
        "head_sha": check.head_sha.as_ref().map(Revision::as_str),
        "status": check.status,
        "conclusion": check.conclusion,
        "html_url": check.url,
        "details_url": check.details_url,
        "completed_at": check.completed_at,
    })
}

fn status_record(status: &CommitStatus) -> Value {
    json!({
        "context": status.context,
        "state": status.state,
        "description": status.description,
        "target_url": status.target_url,
        "created_at": status.created_at,
    })
}

fn run_evidence(run: &WorkflowRun) -> RunEvidence {
    RunEvidence {
        id: run.id.get(),
        workflow_id: run.workflow_id.get(),
        attempt: run.attempt.unwrap_or_default(),
        name: run.name.clone().unwrap_or_default(),
        path: run.path.clone(),
        event: run.event.clone(),
        status: run.status.clone().unwrap_or_default(),
        conclusion: run.conclusion.clone(),
        head_branch: run.head_branch.clone().unwrap_or_default(),
        head_sha: run.head_sha.to_string(),
        created_at: run.created_at.clone(),
        started_at: run.started_at.clone(),
        updated_at: run.updated_at.clone(),
        url: run.url.clone(),
        repository: run.repository.clone(),
    }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// What a commit's checks add up to, in one word a wait can act on.
///
/// `unknown` is silence — nothing registered, or a token this does not know —
/// and a wait tolerates it for a grace period before giving up on it. A
/// completed run that concluded neutral or skipped is not silence: it reported,
/// and it is over. Folding those into `unknown` had a wait sit out the whole
/// grace period and then say no checks were configured or reporting, about
/// checks that had reported. They settle under their own name instead, which
/// reads as the non-pass they are.
fn assessment(conclusions: &[&str]) -> &'static str {
    if conclusions.is_empty() {
        return UNKNOWN_ASSESSMENT;
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
        return FAILURE_ASSESSMENT;
    }
    if conclusions.iter().any(|value| {
        matches!(
            *value,
            "pending" | "queued" | "in_progress" | "waiting" | "requested"
        )
    }) {
        return PENDING_ASSESSMENT;
    }
    if !conclusions
        .iter()
        .all(|value| *value == SUCCESS_ASSESSMENT || TERMINAL_ASSESSMENTS.contains(value))
    {
        return UNKNOWN_ASSESSMENT;
    }
    conclusions
        .iter()
        .find_map(|value| {
            TERMINAL_ASSESSMENTS
                .iter()
                .copied()
                .find(|terminal| terminal == value)
        })
        .unwrap_or(SUCCESS_ASSESSMENT)
}

fn issue_record(issue: &Issue) -> Value {
    json!({
        "number": issue.reference.number().get(),
        "title": issue.title,
        "body": issue.body.as_deref().map(|body| truncate(body, ISSUE_BODY_CHARS)),
        "state": issue.state,
        "html_url": issue.url,
        "created_at": issue.created_at,
        "updated_at": issue.updated_at,
        "closed_at": issue.closed_at,
        "labels": labels(&issue.labels),
        "assignees": issue.assignees,
    })
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

fn ci_token<'a>(status: Option<&'a str>, conclusion: Option<&'a str>) -> &'a str {
    match status {
        Some(COMPLETED_STATUS) => conclusion.unwrap_or(UNKNOWN_ASSESSMENT),
        Some(status) => status,
        None => UNKNOWN_ASSESSMENT,
    }
}

fn run_token(run: &WorkflowRun) -> &str {
    ci_token(run.status.as_deref(), run.conclusion.as_deref())
}

fn check_token(check: &CheckRun) -> &str {
    ci_token(Some(&check.status), check.conclusion.as_deref())
}

fn ci_priority(row: &Value) -> u8 {
    let token = row["state"]
        .as_str()
        .unwrap_or_else(|| ci_token(row["status"].as_str(), row["conclusion"].as_str()));
    match assessment(&[token]) {
        FAILURE_ASSESSMENT => 0,
        PENDING_ASSESSMENT => 1,
        SUCCESS_ASSESSMENT => 3,
        _ => 2,
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
    use abnegate_vcs::RateLimit;
    use chrono::DateTime;
    use std::time::Instant;
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use zone_core::tools::REASON_DESCRIPTION;

    const PAGE_SIZE: usize = PER_PAGE.get() as usize;
    const RETRY_AFTER_HEADER: &str = "Retry-After";
    const RATE_LIMIT_REMAINING_HEADER: &str = "X-RateLimit-Remaining";

    #[test]
    fn heads_may_be_slashed_branch_names() {
        assert!(git_ref("feat/features-settings-tab"));
        assert!(git_ref("owner:feat/branch"));
        assert!(!git_ref("feat/../main"));
        assert!(!git_ref("bad owner:branch"));
        assert!(!git_ref(""));
    }

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
                offline: false,
                origin: crate::db::tasks::Origin::Member,
            },
            operation,
        }
    }

    fn github(server: &MockServer) -> Github {
        github_named(server, "owner", "repository")
    }

    fn configured(owner: &str, repo: &str) -> Configuration {
        Configuration {
            owner: owner.into(),
            repo: repo.into(),
            branch: Some("main".into()),
            path: None,
            token: Some("test-secret".into()),
            review_signals: None,
        }
    }

    fn github_named(server: &MockServer, owner: &str, repo: &str) -> Github {
        github_serving(server, configured(owner, repo))
    }

    fn github_serving(server: &MockServer, configuration: Configuration) -> Github {
        Github::serving(
            configuration,
            PullRequestService::standing_in_for("github.com", &server.uri()).unwrap(),
        )
        .unwrap()
    }

    fn github_with_token(server: &MockServer, token: Option<&str>) -> Github {
        github_serving(
            server,
            Configuration {
                token: token.map(str::to_string),
                ..configured("owner", "repository")
            },
        )
    }

    const BEARER: &str = "Bearer test-secret";

    async fn mock(server: &MockServer, endpoint: &str, response: Value) {
        mock_answering(server, endpoint, response, Some(BEARER)).await;
    }

    async fn mock_answering(
        server: &MockServer,
        endpoint: &str,
        response: Value,
        authorization: Option<&str>,
    ) {
        let request =
            Mock::given(method("GET")).and(path(format!("/repos/owner/repository/{endpoint}")));
        let request = match authorization {
            Some(value) => request.and(header("Authorization", value)),
            None => request,
        };
        request
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .mount(server)
            .await;
    }

    async fn assert_sent_without_authorization(server: &MockServer) {
        let requests = server.received_requests().await.unwrap();
        assert!(!requests.is_empty(), "the source must have been read");
        for request in requests {
            assert!(
                !request.headers.contains_key("authorization"),
                "{} {} carried an Authorization header",
                request.method,
                request.url.path()
            );
        }
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
            Operation::Issue,
            Operation::File,
            Operation::PullRequests,
            Operation::ReviewThreads,
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

    /// The pass asked for "the full details of issue number 5" and the model
    /// had only list_issues, whose bodies are snippets; it answered from the
    /// snippet. One issue in full is its own read, and a pull request number
    /// is turned away towards the tool that reviews those.
    #[tokio::test]
    async fn get_issue_reads_one_issue_in_full_with_its_comments() {
        let server = MockServer::start().await;
        let body = "the whole body ".repeat(400);
        mock(
            &server,
            "issues/5",
            json!({
                "number": 5,
                "title": "Document how to run the widget checks locally",
                "body": body,
                "state": "open",
                "html_url": "https://github.com/owner/repository/issues/5",
                "user": {"login": "abnegate"},
                "labels": [{"name": "docs", "color": "0075ca", "description": "Documentation"}],
                "assignees": [{"login": "octocat"}],
                "comments": 1,
                "created_at": "2026-09-01T00:00:00Z",
                "updated_at": "2026-09-02T00:00:00Z",
                "closed_at": null
            }),
        )
        .await;
        mock(
            &server,
            "issues/5/comments",
            json!([{
                "id": 11,
                "body": "README should explain check.sh",
                "html_url": "https://github.com/owner/repository/issues/5#issuecomment-11",
                "user": {"login": "reviewer"},
                "created_at": "2026-09-02T00:00:00Z"
            }]),
        )
        .await;
        mock(
            &server,
            "issues/8",
            json!({
                "number": 8,
                "title": "Ship the billing export",
                "state": "open",
                "html_url": "https://github.com/owner/repository/pull/8",
                "labels": [],
                "comments": 0,
                "created_at": "2026-09-01T00:00:00Z",
                "updated_at": "2026-09-02T00:00:00Z",
                "pull_request": {}
            }),
        )
        .await;

        let tool = tool(Operation::Issue);
        assert_eq!(tool.name(), "get_issue");
        assert!(purpose_of(tool.description()).contains("one GitHub issue in full"));
        let schema = tool.parameters_schema();
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!("number"))
        );
        assert!(schema["properties"].get("ref").is_none());

        let arguments = |number: u64| Arguments {
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
            directory: None,
            number: Some(number),
            resolved: None,
            comment: None,
            after: None,
            job_id: None,
            tag: None,
            reason: None,
        };
        let result = github(&server)
            .observe(Operation::Issue, &arguments(5))
            .await
            .unwrap();
        assert_eq!(result["issue"]["number"], 5);
        assert_eq!(result["issue"]["body"], body, "the body is not a snippet");
        assert_eq!(result["issue"]["author"], "abnegate");
        assert_eq!(
            result["issue"]["labels"],
            json!([{"name": "docs", "color": "0075ca", "description": "Documentation"}])
        );
        assert_eq!(result["issue"]["assignees"], json!(["octocat"]));
        assert_eq!(result["issue"]["comments"], 1);
        assert_eq!(result["issue"]["state"], "open");
        assert_eq!(result["comments"][0]["author"], "reviewer");
        assert_eq!(
            result["comments"][0]["body"],
            "README should explain check.sh"
        );
        assert_eq!(result["comments_complete"], true);
        assert_eq!(result["repository"], "https://github.com/owner/repository");

        let refused = github(&server)
            .observe(Operation::Issue, &arguments(8))
            .await
            .unwrap_err();
        assert!(refused.contains("pull request"), "{refused}");
        assert!(refused.contains("assess_pull_requests"), "{refused}");
    }

    fn purpose_of(description: &str) -> String {
        crate::agent::toolbox::purpose(description)
    }

    #[test]
    fn absent_unknown_and_pending_results_are_not_green() {
        for conclusions in [vec![], vec!["unknown"], vec!["success", "unexpected"]] {
            assert_eq!(assessment(&conclusions), UNKNOWN_ASSESSMENT);
        }
        assert_eq!(assessment(&["success"]), SUCCESS_ASSESSMENT);
        assert_eq!(assessment(&["success", "pending"]), PENDING_ASSESSMENT);
        assert_eq!(assessment(&["pending", "failure"]), FAILURE_ASSESSMENT);
        assert_eq!(assessment(&["cancelled"]), FAILURE_ASSESSMENT);
    }

    /// A completed run that concluded neutral or skipped has reported, and
    /// reporting is the one thing `unknown` says did not happen. Reading it as
    /// unknown had a wait sit out the whole grace period and then answer that
    /// no checks are configured or reporting on the commit — about a commit
    /// whose checks had reported and finished. Each settles under its own name,
    /// and none of those names is a pass.
    #[test]
    fn a_completed_run_that_neither_passed_nor_failed_settles_under_its_own_name() {
        for conclusion in TERMINAL_ASSESSMENTS {
            assert_eq!(assessment(&[conclusion]), conclusion);
            assert_eq!(
                assessment(&["success", conclusion]),
                conclusion,
                "the conclusion that is not a pass is the one the commit is reported as"
            );
            assert_ne!(assessment(&[conclusion]), SUCCESS_ASSESSMENT);
            assert_ne!(
                assessment(&[conclusion]),
                UNKNOWN_ASSESSMENT,
                "{conclusion} reported, which is the opposite of what unknown stands for"
            );
            assert!(
                SETTLED_ASSESSMENTS.contains(&assessment(&[conclusion])),
                "{conclusion} is over, so a wait on it has nothing left to wait for"
            );
        }
        assert_eq!(
            assessment(&["queued", "skipped"]),
            PENDING_ASSESSMENT,
            "a run still queued beside a skipped one has not finished"
        );
        assert_eq!(
            assessment(&["failure", "neutral"]),
            FAILURE_ASSESSMENT,
            "a failure outranks every other conclusion on the commit"
        );
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
        Mock::given(path("/repos/owner/repository/actions/runs"))
            .and(query_param("head_sha", COMMIT))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"workflow_runs": [workflow_run(11, 1, "success")], "total_count":1}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        mock(&server, &format!("commits/{COMMIT}/check-runs"), json!({"check_runs": [{"head_sha": COMMIT,"status":"completed","conclusion":"success"}]})).await;
        mock_statuses(&server, json!([])).await;
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
                    directory: None,
                    number: None,
                    resolved: None,
                    comment: None,
                    after: None,
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
        let checks: Vec<Value> = (1..=100)
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
            json!({"check_runs":[{"id":101,"head_sha":COMMIT,"status":"completed","conclusion":"failure"}]}),
        ))
        .mount(&server)
        .await;
        mock_statuses(&server, json!([])).await;
        let result = github(&server).build(COMMIT).await.unwrap();
        let checks = result["checks"].as_array().unwrap();
        assert_eq!(result["complete"], true);
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
        mock_statuses(&server, json!([])).await;
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
        mock_statuses(&server, json!([])).await;
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

        let recovered = MockServer::start().await;
        mock(
            &recovered,
            "actions/runs",
            json!({"workflow_runs":[],"total_count":0}),
        )
        .await;
        mock(
            &recovered,
            &format!("commits/{COMMIT}/check-runs"),
            json!({"check_runs":[]}),
        )
        .await;
        mock(
            &recovered,
            &format!("commits/{COMMIT}/statuses"),
            json!([
                {"context":"test","state":"success"},
                {"context":"test","state":"failure"}
            ]),
        )
        .await;
        mock_statuses(&recovered, json!([{"context":"test","state":"success"}])).await;
        let build = github(&recovered).build(COMMIT).await.unwrap();
        assert_eq!(
            build["state"], "success",
            "the combined status keeps only each context's latest state"
        );
        assert_eq!(build["statuses"].as_array().unwrap().len(), 1);
        assert!(
            recovered
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| !request.url.path().ends_with("/statuses")),
            "every status a context ever reported is never read"
        );
    }

    #[tokio::test]
    async fn provider_errors_never_echo_secret_or_follow_redirects() {
        let server = MockServer::start().await;
        let external = MockServer::start().await;
        let unbranched = |server: &MockServer| {
            let mut github = github(server);
            github.configuration.branch = None;
            github
        };
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", external.uri())
                    .set_body_string("test-secret"),
            )
            .mount(&server)
            .await;
        let error = unbranched(&server).resolve(None).await.unwrap_err();
        assert_eq!(error, status_error(302));
        assert!(error.contains("302"));
        assert!(!error.contains("test-secret"));
        assert!(external.received_requests().await.unwrap().is_empty());
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string("test-secret"))
            .mount(&server)
            .await;
        let error = unbranched(&server).resolve(None).await.unwrap_err();
        assert_eq!(error, status_error(FORBIDDEN));
        assert!(!error.contains("test-secret"));
    }

    #[tokio::test]
    async fn an_unclassified_status_names_itself_and_never_github_s_words() {
        for status in [400, 409, 410, UNPROCESSABLE_ENTITY, 500, 503] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/repos/owner/repository/commits/main"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(json!({"message": "test-secret"})),
                )
                .expect(1)
                .mount(&server)
                .await;
            let error = github(&server).resolve(None).await.unwrap_err();
            assert_eq!(error, status_error(status), "HTTP {status}");
            assert!(!error.contains("test-secret"), "HTTP {status}: {error}");
        }
    }

    #[tokio::test]
    async fn every_request_names_the_api_version() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository"))
            .and(header("X-GitHub-Api-Version", API_VERSION))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"default_branch": "main"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository/commits/main"))
            .and(header("X-GitHub-Api-Version", API_VERSION))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"sha": COMMIT})))
            .mount(&server)
            .await;
        let mut github = github(&server);
        github.configuration.branch = None;
        let resolved = github.resolve(None).await.unwrap();
        assert_eq!(resolved, ("main".to_string(), COMMIT.to_string()));
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests.iter().all(|request| request
                .headers
                .get("x-github-api-version")
                .is_some_and(|version| version == API_VERSION)),
            "every request names the API version"
        );
    }

    #[test]
    fn crate_errors_keep_zones_words() {
        let cases = [
            (
                PullRequestError::quota_exhausted(
                    RateLimit::default()
                        .with_limit(5000)
                        .with_remaining(0)
                        .with_resource("core"),
                ),
                RATE_LIMIT_EXHAUSTED.to_string(),
            ),
            (
                PullRequestError::quota_exhausted(RateLimit::default()),
                RATE_LIMIT_EXHAUSTED.to_string(),
            ),
            (
                PullRequestError::AuthenticationFailed,
                status_error(UNAUTHORIZED),
            ),
            (
                PullRequestError::AuthenticationRequired,
                status_error(UNAUTHORIZED),
            ),
            (PullRequestError::Forbidden, status_error(FORBIDDEN)),
            (PullRequestError::NotFound, status_error(NOT_FOUND)),
            (
                PullRequestError::RepositoryNotFound,
                status_error(NOT_FOUND),
            ),
            (
                PullRequestError::PullRequestNotFound,
                status_error(NOT_FOUND),
            ),
            (
                PullRequestError::RateLimited,
                status_error(TOO_MANY_REQUESTS),
            ),
            (
                PullRequestError::PullRequestAlreadyExists(BranchName::parse("feature").unwrap()),
                status_error(UNPROCESSABLE_ENTITY),
            ),
            (
                PullRequestError::JobLog(JobLogError::Undirected),
                LOG_UNDIRECTED.to_string(),
            ),
            (
                PullRequestError::JobLog(JobLogError::UnexpectedHost),
                LOG_UNEXPECTED_HOST.to_string(),
            ),
            (
                PullRequestError::JobLog(JobLogError::Unreachable),
                LOG_UNREACHABLE.to_string(),
            ),
            (
                PullRequestError::JobLog(JobLogError::Refused(503)),
                "The job log download returned HTTP 503.".to_string(),
            ),
            (
                PullRequestError::JobLog(JobLogError::Interrupted),
                LOG_INTERRUPTED.to_string(),
            ),
            (PullRequestError::TooLarge, FILE_TOO_LARGE.to_string()),
            (PullRequestError::InvalidOrigin, INVALID_ORIGIN.to_string()),
            (
                PullRequestError::InvalidRepositoryName,
                INVALID_REPOSITORY.to_string(),
            ),
            (
                PullRequestError::InvalidRepositoryUrl,
                INVALID_REPOSITORY.to_string(),
            ),
            (
                PullRequestError::Parse(ParseError::BranchName("test-secret".into())),
                INVALID_RESPONSE.to_string(),
            ),
            (
                PullRequestError::Parse(ParseError::CommitSha("test-secret".into())),
                INVALID_RESPONSE.to_string(),
            ),
            (
                PullRequestError::Parse(ParseError::ObjectId("test-secret".into())),
                INVALID_RESPONSE.to_string(),
            ),
            (
                PullRequestError::Parse(ParseError::RepositoryPath("test-secret".into())),
                INVALID_RESPONSE.to_string(),
            ),
            (
                PullRequestError::Parse(ParseError::RepositoryUrl),
                INVALID_RESPONSE.to_string(),
            ),
            (
                PullRequestError::Parse(ParseError::Tag("test-secret".into())),
                INVALID_RESPONSE.to_string(),
            ),
            (
                PullRequestError::GitHubApi(
                    "GitHub API returned 500 Internal Server Error: test-secret".into(),
                ),
                UNEXPECTED_RESPONSE.to_string(),
            ),
            (
                PullRequestError::GitHubApi("GitHub API returned 302 Found".into()),
                UNEXPECTED_RESPONSE.to_string(),
            ),
            (PullRequestError::refused(302, ""), status_error(302)),
            (
                PullRequestError::refused(400, "Problems parsing JSON"),
                status_error(400),
            ),
            (
                PullRequestError::refused(409, "Git Repository is empty."),
                status_error(409),
            ),
            (
                PullRequestError::refused(
                    UNPROCESSABLE_ENTITY,
                    "Validation Failed; No commits between main and test-secret",
                ),
                status_error(UNPROCESSABLE_ENTITY),
            ),
            (
                PullRequestError::refused(500, "test-secret"),
                status_error(500),
            ),
            (PullRequestError::refused(503, ""), status_error(503)),
            (
                PullRequestError::RepositoryExists("test-secret".into()),
                UNEXPECTED_RESPONSE.to_string(),
            ),
            (
                PullRequestError::NotMergeable("test-secret".into()),
                UNEXPECTED_RESPONSE.to_string(),
            ),
            (
                PullRequestError::Protected("test-secret".into()),
                UNEXPECTED_RESPONSE.to_string(),
            ),
            (PullRequestError::HeadMoved, UNEXPECTED_RESPONSE.to_string()),
            (
                PullRequestError::AutoMergeNotAllowed,
                UNEXPECTED_RESPONSE.to_string(),
            ),
            (
                PullRequestError::MergeMethodNotAllowed("test-secret".into()),
                UNEXPECTED_RESPONSE.to_string(),
            ),
            (
                PullRequestError::AlreadyMergeable,
                UNEXPECTED_RESPONSE.to_string(),
            ),
        ];
        for (error, expected) in cases {
            let shown = format!("{error:?}");
            let said = described(error);
            assert_eq!(said, expected, "{shown}");
            assert!(!said.contains("test-secret"), "{shown} echoed its payload");
        }
    }

    #[tokio::test]
    async fn an_unreachable_github_reads_as_failed_or_timed_out() {
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let github = Github::serving(
            configured("owner", "repository"),
            PullRequestService::standing_in_for("github.com", &format!("http://{address}"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(github.resolve(None).await.unwrap_err(), REQUEST_FAILED);
    }

    #[tokio::test]
    async fn default_branch_and_commit_answers_keep_their_site_words() {
        let server = MockServer::start().await;
        let mut github = github(&server);
        github.configuration.branch = None;
        for (repository, commit, expected) in [
            (
                json!({"default_branch": "bad..name"}),
                json!({"sha": COMMIT}),
                MISSING_DEFAULT_BRANCH,
            ),
            (json!({}), json!({"sha": COMMIT}), UNEXPECTED_RESPONSE),
            (
                json!({"default_branch": "main"}),
                json!({"sha": "abc123"}),
                MISSING_COMMIT,
            ),
            (
                json!({"default_branch": "main"}),
                json!({}),
                UNEXPECTED_RESPONSE,
            ),
        ] {
            server.reset().await;
            Mock::given(method("GET"))
                .and(path("/repos/owner/repository"))
                .respond_with(ResponseTemplate::new(200).set_body_json(repository.clone()))
                .mount(&server)
                .await;
            mock(&server, "commits/main", commit.clone()).await;
            assert_eq!(
                github.resolve(None).await.unwrap_err(),
                expected,
                "{repository} then {commit}"
            );
        }
    }

    #[tokio::test]
    async fn a_revision_expression_is_refused_before_anything_is_sent() {
        let server = MockServer::start().await;
        mock(&server, "commits/main", json!({"sha": COMMIT})).await;
        for expression in ["main~1", "main^", "HEAD@{1}"] {
            assert_eq!(
                github(&server).resolve(Some(expression)).await.unwrap_err(),
                INVALID_REFERENCE,
                "{expression}"
            );
        }
        assert!(server.received_requests().await.unwrap().is_empty());
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
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"sha":COMMIT,"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"100644","sha":BLOB,"size":content.len()}]})).await;
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
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"sha":COMMIT,"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"120000","sha":BLOB,"size":9}]})).await;
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
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"sha":COMMIT,"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"100644","sha":BLOB,"size":content.len()}]})).await;
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
    async fn a_file_larger_than_the_cap_is_refused_before_its_blob_is_read() {
        let server = MockServer::start().await;
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"sha":COMMIT,"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"100644","sha":BLOB,"size":MAX_FILE_BYTES + 1}]})).await;
        let refused = github(&server)
            .file(COMMIT, "README.md", 0, FILE_PAGE_CHARS)
            .await
            .unwrap_err();
        assert_eq!(refused, FILE_TOO_LARGE);
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "only the tree is read"
        );
    }

    #[tokio::test]
    async fn a_tree_entry_without_a_size_is_refused_before_its_blob_is_read() {
        let server = MockServer::start().await;
        mock(&server, &format!("git/trees/{COMMIT}"), json!({"sha":COMMIT,"truncated":false,"tree":[{"path":"README.md","type":"blob","mode":"100644","sha":BLOB}]})).await;
        let refused = github(&server)
            .file(COMMIT, "README.md", 0, FILE_PAGE_CHARS)
            .await
            .unwrap_err();
        assert_eq!(refused, SIZE_MISMATCH);
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "only the tree is read"
        );
    }

    #[tokio::test]
    async fn deployments_include_latest_provider_status() {
        let server = MockServer::start().await;
        mock(
            &server,
            "deployments",
            json!([{"id":7,"sha":COMMIT,"ref":"main","environment":"production",
                    "created_at":"2026-09-05T00:00:00Z","updated_at":"2026-09-05T00:00:00Z"}]),
        )
        .await;
        Mock::given(path("/repos/owner/repository/deployments/7/statuses"))
            .and(query_param("per_page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!([{"state":"pending","description":"","created_at":"2026-09-05T00:00:00Z"}]),
            ))
            .mount(&server)
            .await;
        let result = github(&server).deployments(COMMIT, 1).await.unwrap();
        assert_eq!(result["deployments"][0]["status"]["state"], "pending");
        assert_eq!(result["deployments"][0]["ref"], "main");
        assert_eq!(
            result["deployments"][0]["url"],
            format!("{}/repos/owner/repository/deployments/7", server.uri())
        );
        assert_eq!(result["next_page"], Value::Null);
    }

    #[tokio::test]
    async fn issues_keep_full_body_exclude_pulls_and_report_provider_page() {
        let server = MockServer::start().await;
        let mut issues = vec![json!({"number":1,"pull_request":{}}); 98];
        let body = "full issue ".repeat(2000);
        let issue = |number: u64, title: &str, body: &str| {
            json!({
                "number": number,
                "title": title,
                "body": body,
                "state": "open",
                "html_url": format!("https://github.com/owner/repository/issues/{number}"),
                "labels": [],
                "comments": 0,
                "created_at": "2026-09-01T00:00:00Z",
                "updated_at": "2026-09-02T00:00:00Z"
            })
        };
        issues.push(issue(99, "Tiny", "ok"));
        issues.push(issue(100, "Bug", &body));
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
                    directory: None,
                    number: None,
                    resolved: None,
                    comment: None,
                    after: None,
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
        let issue = |number: u64| {
            Issue::new(
                IssueReference::new(
                    Repository::new("owner", "repository").unwrap(),
                    NonZeroU64::new(number).unwrap(),
                ),
                "Bug",
                format!("https://github.com/owner/repository/issues/{number}"),
                "2026-09-01T00:00:00Z",
                "2026-09-02T00:00:00Z",
            )
        };
        let long = issue_record(&issue(7).with_body("x".repeat(2_000)));
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
        assert_eq!(issue_record(&issue(1).with_body("ok"))["body"], "ok");
        assert_eq!(
            issue_record(&issue(2).with_body("a".repeat(ISSUE_BODY_CHARS)))["body"],
            "a".repeat(ISSUE_BODY_CHARS)
        );
        assert_eq!(issue_record(&issue(3))["body"], Value::Null);
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
            .and(body_partial_json(json!({
                "title": "Fix",
                "head": "feature",
                "base": "main",
                "body": "details"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(created_pull(12, "feature")))
            .expect(1)
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
                    directory: None,
                    number: None,
                    resolved: None,
                    comment: None,
                    after: None,
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
        assert_eq!(result["pull_request"]["title"], "Fix");
        assert_eq!(result["pull_request"]["state"], "open");
    }

    fn created_pull(number: u64, head: &str) -> Value {
        json!({
            "id": number,
            "number": number,
            "html_url": format!("https://github.com/owner/repository/pull/{number}"),
            "title": "Fix",
            "state": "open",
            "head": {"ref": head, "sha": COMMIT},
            "base": {"ref": "main", "sha": COMMIT}
        })
    }

    fn found_pull(number: u64, owner: &str, state: &str) -> Value {
        json!({
            "node_id": format!("PR_{number}"),
            "number": number,
            "html_url": format!("https://github.com/owner/repository/pull/{number}"),
            "state": state,
            "title": format!("Fix, take {number}"),
            "draft": false,
            "head": {"ref": "feature", "label": format!("{owner}:feature"), "sha": COMMIT},
            "base": {"ref": "main", "label": "owner:main", "sha": COMMIT}
        })
    }

    async fn refuse_duplicate(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/repos/owner/repository/pulls"))
            .respond_with(ResponseTemplate::new(422).set_body_json(json!({
                "message": "Validation Failed",
                "errors": [{"message": "A pull request already exists for owner:feature."}]
            })))
            .expect(1)
            .mount(server)
            .await;
    }

    async fn mock_found(server: &MockServer, owner: &str, rows: Value) {
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository/pulls"))
            .and(query_param("head", format!("{owner}:feature")))
            .and(query_param("base", "main"))
            .and(query_param("state", "all"))
            .respond_with(ResponseTemplate::new(200).set_body_json(rows))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn a_duplicate_create_answers_only_with_an_open_pull_request() {
        let server = MockServer::start().await;
        refuse_duplicate(&server).await;
        mock_found(&server, "owner", json!([found_pull(12, "owner", "closed")])).await;
        let refused = tool(Operation::CreatePull)
            .open_pull(&github(&server), &pull_args("feature", Some("main"), None))
            .await
            .expect_err("a closed pull request is not the one GitHub says is open");
        assert_eq!(refused, status_error(UNPROCESSABLE_ENTITY));

        server.reset().await;
        refuse_duplicate(&server).await;
        mock_found(
            &server,
            "owner",
            json!([
                found_pull(12, "owner", "closed"),
                found_pull(13, "owner", "open")
            ]),
        )
        .await;
        let answer = tool(Operation::CreatePull)
            .open_pull(&github(&server), &pull_args("feature", Some("main"), None))
            .await
            .expect("the open pull request answers a duplicate create");
        assert_eq!(answer["already_open"], true, "{answer}");
        assert_eq!(answer["pull_request"]["number"], 13, "{answer}");
        assert_eq!(answer["pull_request"]["title"], "Fix, take 13", "{answer}");
        assert_eq!(answer["pull_request"]["state"], "open", "{answer}");
        assert_eq!(
            answer["pull_request"]["html_url"],
            "https://github.com/owner/repository/pull/13"
        );

        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repository/pulls"))
            .respond_with(ResponseTemplate::new(422).set_body_json(json!({
                "message": "Validation Failed",
                "errors": [{"message": "No commits between main and feature"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let refused = tool(Operation::CreatePull)
            .open_pull(&github(&server), &pull_args("feature", Some("main"), None))
            .await
            .expect_err("a refusal that is not a duplicate is an error");
        assert_eq!(refused, status_error(UNPROCESSABLE_ENTITY));
        assert!(!refused.contains("No commits"), "{refused}");
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.method.as_str() == "POST"),
            "only a duplicate is looked up"
        );
    }

    #[tokio::test]
    async fn a_fork_head_is_opened_from_its_owner() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repository/pulls"))
            .and(body_partial_json(
                json!({"head": "contributor:feature", "base": "main"}),
            ))
            .respond_with(ResponseTemplate::new(201).set_body_json(created_pull(14, "feature")))
            .expect(1)
            .mount(&server)
            .await;
        let created = github(&server)
            .write(
                Operation::CreatePull,
                &pull_args("contributor:feature", Some("main"), None),
            )
            .await
            .unwrap();
        assert_eq!(created["pull_request"]["number"], 14, "{created}");

        server.reset().await;
        refuse_duplicate(&server).await;
        mock_found(
            &server,
            "contributor",
            json!([found_pull(14, "contributor", "open")]),
        )
        .await;
        let answer = tool(Operation::CreatePull)
            .open_pull(
                &github(&server),
                &pull_args("contributor:feature", Some("main"), None),
            )
            .await
            .expect("the fork's open pull request answers a duplicate create");
        assert_eq!(answer["already_open"], true, "{answer}");
        assert_eq!(answer["pull_request"]["number"], 14, "{answer}");
    }

    #[tokio::test]
    async fn comment_on_issue_posts_markdown() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repository/issues/7/comments"))
            .and(header("Authorization", "Bearer test-secret"))
            .and(body_partial_json(json!({"body": "ship it"})))
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
                    directory: None,
                    number: Some(7),
                    resolved: None,
                    comment: None,
                    after: None,
                    job_id: None,
                    tag: None,
                    reason: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(result["comment"]["id"], 99);
        assert_eq!(result["comment"]["body"], "ship it");
        assert_eq!(
            result["comment"]["html_url"],
            "https://github.com/owner/repository/issues/7#issuecomment-99"
        );
        assert_eq!(result["comment"]["created_at"], "2026-09-05T00:00:00Z");
    }

    fn pull_args(head: &str, base: Option<&str>, directory: Option<&str>) -> Arguments {
        Arguments {
            base: base.map(str::to_string),
            directory: directory.map(str::to_string),
            ..write_args(Some("Fix"), Some(head), None, None)
        }
    }

    #[tokio::test]
    async fn a_head_that_is_the_base_is_refused_before_anything_is_pushed_or_posted() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201))
            .expect(0)
            .mount(&server)
            .await;
        let refused = tool(Operation::CreatePull)
            .open_pull(&github(&server), &pull_args("main", None, Some("/nowhere")))
            .await
            .expect_err("a pull request from main into main is refused");
        assert!(refused.contains("head and base are both main"), "{refused}");
    }

    #[tokio::test]
    async fn invalid_fields_are_refused_before_the_head_is_pushed() {
        let server = MockServer::start().await;
        let arguments = Arguments {
            title: None,
            ..pull_args("feature", None, Some("/nowhere"))
        };
        let refused = tool(Operation::CreatePull)
            .open_pull(&github(&server), &arguments)
            .await
            .expect_err("a pull request with no title is refused");
        assert!(refused.contains("title is required"), "{refused}");
    }

    #[tokio::test]
    async fn a_named_directory_outside_the_workspace_is_never_pushed_from() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201))
            .expect(0)
            .mount(&server)
            .await;
        let outside = tempfile::tempdir().unwrap();
        let refused = tool(Operation::CreatePull)
            .open_pull(
                &github(&server),
                &pull_args("feature", None, outside.path().to_str()),
            )
            .await
            .expect_err("another place's checkout is refused");
        assert!(
            refused.contains("is not in one of this workspace's folders"),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn a_directory_in_the_workspaces_own_clones_is_its_own() {
        let tool = tool(Operation::CreatePull);
        let clone = tool
            .scope
            .state
            .config()
            .clones
            .join(tool.scope.workspace_id.as_hyphenated().to_string())
            .join("repository");
        let arguments = pull_args("feature", None, clone.to_str());
        assert_eq!(
            tool.checkout(&arguments).await.expect("its own clone"),
            Some(clone)
        );
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
            directory: None,
            number,
            resolved: None,
            comment: None,
            after: None,
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

    fn thread_counts(resolved: bool, more_threads: bool) -> Value {
        json!({"data": {"repository": {"pullRequest": {
            "number": 7,
            "reviewThreads": {
                "pageInfo": {"hasNextPage": more_threads, "endCursor": null},
                "nodes": [{"id": "PRRT_1", "isResolved": resolved}]
            }
        }}}})
    }

    fn conversation_comment(id: u64, author: &str, body: &str) -> Value {
        json!({
            "id": id,
            "body": body,
            "html_url": format!("https://github.com/owner/repository/pull/7#issuecomment-{id}"),
            "created_at": "2026-09-05T00:00:00Z",
            "user": {"login": author}
        })
    }

    /// Pull request 7's review: one thread, resolved or not, more of them
    /// GitHub would not page to when `more_threads`, and the conversation
    /// comment a review bot summarised the change in.
    async fn mock_review(server: &MockServer, resolved: bool, body: &str, more_threads: bool) {
        mock_graphql(server, thread_counts(resolved, more_threads)).await;
        mock(
            server,
            "issues/7/comments",
            json!([conversation_comment(11, "greptile-apps", body)]),
        )
        .await;
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
            directory: None,
            number,
            resolved: None,
            comment: None,
            after: None,
            job_id: None,
            tag: None,
            reason: None,
        }
    }

    fn thread_payload(more_threads: bool, more_comments: bool) -> Value {
        json!({"data": {"repository": {"pullRequest": {
            "number": 7,
            "reviewThreads": {
                "pageInfo": {"hasNextPage": more_threads},
                "nodes": [
                    {
                        "id": "PRRT_2",
                        "isResolved": true,
                        "isOutdated": true,
                        "path": "src/old.rs",
                        "line": 1,
                        "startLine": null,
                        "diffSide": "RIGHT",
                        "comments": {
                            "pageInfo": {"hasNextPage": false},
                            "nodes": [review_comment_node(2, "Already dealt with.")]
                        }
                    },
                    {
                        "id": "PRRT_1",
                        "isResolved": false,
                        "isOutdated": false,
                        "path": "src/lib.rs",
                        "line": 42,
                        "startLine": null,
                        "diffSide": "RIGHT",
                        "comments": {
                            "pageInfo": {"hasNextPage": more_comments},
                            "nodes": [review_comment_node(1, "This unwrap panics on an empty list.")]
                        }
                    }
                ]
            }
        }}}})
    }

    fn review_comment_node(id: u64, body: &str) -> Value {
        json!({
            "databaseId": id,
            "body": body,
            "url": format!("https://github.com/owner/repository/pull/7#discussion_r{id}"),
            "createdAt": "2026-11-05T00:00:00Z",
            "author": {"login": "coderabbitai"}
        })
    }

    fn long_thread_payload(comments: u64, chars: usize) -> Value {
        let nodes: Vec<Value> = (1..=comments)
            .map(|id| review_comment_node(id, &"x".repeat(chars)))
            .collect();
        json!({"data": {"repository": {"pullRequest": {
            "number": 7,
            "reviewThreads": {
                "pageInfo": {"hasNextPage": false},
                "nodes": [{
                    "id": "PRRT_long",
                    "isResolved": false,
                    "isOutdated": false,
                    "path": "src/lib.rs",
                    "line": 42,
                    "startLine": null,
                    "diffSide": "RIGHT",
                    "comments": {"pageInfo": {"hasNextPage": false}, "nodes": nodes}
                }]
            }
        }}}})
    }

    #[tokio::test]
    async fn review_threads_carry_what_a_reviewer_wrote() {
        let server = MockServer::start().await;
        mock_graphql(&server, thread_payload(false, false)).await;

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("the threads are readable");

        assert_eq!(read["counts"]["unresolved"], 1);
        assert_eq!(read["complete"], true);
        let threads = read["threads"].as_array().expect("threads are a list");
        assert_eq!(
            threads.len(),
            1,
            "a settled thread is not what still needs answering"
        );
        assert_eq!(read["counts"]["resolved"], 1);
        assert_eq!(threads[0]["path"], "src/lib.rs");
        assert_eq!(threads[0]["line"], 42);
        assert_eq!(threads[0]["comments"][0]["author"], "coderabbitai");
        assert_eq!(
            threads[0]["comments"][0]["body"], "This unwrap panics on an empty list.",
            "the body is the whole point of this tool"
        );
    }

    #[tokio::test]
    async fn resolved_threads_are_read_when_asked_for() {
        let server = MockServer::start().await;
        mock_graphql(&server, thread_payload(false, false)).await;
        let mut arguments = readiness_args(Some(7));
        arguments.resolved = Some(true);

        let read = github(&server)
            .review_threads(&arguments)
            .await
            .expect("the threads are readable");

        let threads = read["threads"].as_array().expect("a list");
        assert_eq!(threads.len(), 1, "only the resolved thread was asked for");
        assert_eq!(threads[0]["resolved"], true);
        assert_eq!(threads[0]["path"], "src/old.rs");
        assert_eq!(read["reading"], "resolved");
        assert_eq!(
            read["counts"]["unresolved"], 1,
            "what still needs answering is counted though not read"
        );
        assert_eq!(read["counts"]["resolved"], 1);
    }

    #[tokio::test]
    async fn an_outdated_thread_keeps_the_line_it_was_written_against() {
        let server = MockServer::start().await;
        let mut payload = thread_payload(false, false);
        let thread = &mut payload["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][1];
        thread["isOutdated"] = json!(true);
        thread["line"] = Value::Null;
        thread["originalLine"] = json!(40);
        mock_graphql(&server, payload).await;

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("the threads are readable");

        assert_eq!(read["threads"][0]["outdated"], true);
        assert_eq!(
            read["threads"][0]["line"], 40,
            "a push that moved the code must not lose where the comment was"
        );
    }

    #[tokio::test]
    async fn review_threads_do_not_advertise_a_ref() {
        let schema = tool(Operation::ReviewThreads).parameters_schema();
        assert!(
            schema["properties"].get("ref").is_none(),
            "threads are read as they stand now, never at a commit: {schema}"
        );
    }

    #[tokio::test]
    async fn review_threads_refuse_a_ref() {
        let server = MockServer::start().await;
        mock_graphql(&server, thread_payload(false, false)).await;
        let mut arguments = readiness_args(Some(7));
        arguments.reference = Some("an-old-commit".into());

        let refused = github(&server)
            .review_threads(&arguments)
            .await
            .expect_err("current threads must not pass for the threads at a ref");

        assert!(refused.contains("ref does not apply"), "{refused}");
        assert!(
            server.received_requests().await.unwrap().is_empty(),
            "a refused ref is refused before GitHub is asked"
        );
    }

    /// The longest owner and repository names GitHub allows, so a page that
    /// fits with these fits with any.
    fn longest_github(server: &MockServer) -> Github {
        github_named(server, &"o".repeat(39), &"r".repeat(100))
    }

    /// The size of `read` once `observe` and `run` have added what they add.
    fn delivered(mut read: Value, github: &Github) -> usize {
        read["repository"] = json!(format!(
            "https://github.com/{}/{}",
            github.configuration.owner, github.configuration.repo
        ));
        read["source_id"] = json!(Uuid::new_v4());
        read["observed_at"] = json!(Utc::now().to_rfc3339());
        json_chars(&read)
    }

    async fn read_every_comment(github: &Github) -> (Vec<u64>, usize) {
        let mut seen = Vec::new();
        let mut after = None;
        let mut pages = 0;
        loop {
            let mut arguments = readiness_args(Some(7));
            arguments.after = after;
            let read = github
                .review_threads(&arguments)
                .await
                .expect("each page is readable");
            for thread in read["threads"].as_array().expect("threads are a list") {
                for comment in thread["comments"].as_array().expect("comments are a list") {
                    seen.push(comment["id"].as_u64().expect("every comment has an id"));
                }
            }
            after = read["next_after"].as_u64();
            let size = delivered(read, github);
            assert!(
                size <= MAX_TOOL_MESSAGE_CHARS,
                "a page must survive the transcript cap whole, got {size}"
            );
            pages += 1;
            assert!(pages <= 100, "following next_after must finish");
            if after.is_none() {
                return (seen, pages);
            }
        }
    }

    #[tokio::test]
    async fn review_threads_page_within_the_message_budget() {
        let server = MockServer::start().await;
        mock_graphql(&server, long_thread_payload(20, REVIEW_BODY_CHARS)).await;

        let (seen, pages) = read_every_comment(&longest_github(&server)).await;

        assert!(pages > 1, "twenty long comments cannot fit one message");
        assert_eq!(
            seen,
            (1..=20).collect::<Vec<_>>(),
            "every comment arrives exactly once, in order"
        );
    }

    #[tokio::test]
    async fn pages_cut_across_threads_without_losing_a_comment() {
        let server = MockServer::start().await;
        let threads: Vec<Value> = (0..6u64)
            .map(|thread| {
                let comments: Vec<Value> = (1..=3u64)
                    .map(|index| {
                        let id = thread * 3 + index;
                        review_comment_node(id, &"z\n\"".repeat(150 * id as usize))
                    })
                    .collect();
                json!({
                    "id": format!("PRRT_{thread}"),
                    "isResolved": false,
                    "isOutdated": false,
                    "path": format!("src/{thread}.rs"),
                    "line": thread + 1,
                    "startLine": null,
                    "diffSide": "RIGHT",
                    "comments": {"pageInfo": {"hasNextPage": false}, "nodes": comments}
                })
            })
            .collect();
        mock_graphql(
            &server,
            json!({"data": {"repository": {"pullRequest": {
                "number": 7,
                "reviewThreads": {"pageInfo": {"hasNextPage": false}, "nodes": threads}
            }}}}),
        )
        .await;

        let (seen, pages) = read_every_comment(&longest_github(&server)).await;

        assert!(
            pages > 2,
            "eighteen escape-heavy comments span several pages"
        );
        assert_eq!(
            seen,
            (1..=18).collect::<Vec<_>>(),
            "every comment arrives exactly once, in order"
        );
    }

    #[test]
    fn the_envelope_reserve_covers_the_longest_repository_names() {
        let mut envelope = json!({"repository": format!(
            "https://github.com/{}/{}",
            "o".repeat(39),
            "r".repeat(100)
        )});
        envelope["source_id"] = json!(Uuid::new_v4());
        envelope["observed_at"] = json!(Utc::now().to_rfc3339());

        assert!(
            json_chars(&envelope) < RESULT_ENVELOPE_CHARS,
            "what observe and run add after a page is sized must fit the reserve, got {}",
            json_chars(&envelope)
        );
    }

    #[test]
    fn escaped_fit_counts_what_json_adds() {
        assert_eq!(escaped_fit("plain", 10), 5);
        assert_eq!(
            escaped_fit("\n\n\n", 4),
            2,
            "a newline costs two once escaped"
        );
        assert_eq!(escaped_fit("\u{1}", 5), 0, "a bare control costs six");
        assert_eq!(
            json_chars(&json!("\"\\\n\r\t\u{8}\u{c}\u{1}é")),
            2 + 2 * 7 + 6 + 1,
            "the costs mirror serde_json's own escaping"
        );
        assert_eq!(escaped_fit("\"\\\n\r\t\u{8}\u{c}\u{1}é", 21), 9);
        assert_eq!(escaped_fit("\"\\\n\r\t\u{8}\u{c}\u{1}é", 20), 8);
    }

    #[tokio::test]
    async fn an_escape_heavy_comment_still_fits_one_message() {
        let server = MockServer::start().await;
        let mut payload = long_thread_payload(1, 0);
        payload["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][0]["comments"]["nodes"]
            [0]["body"] = json!("\u{1}".repeat(REVIEW_BODY_CHARS));
        mock_graphql(&server, payload).await;

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("the threads are readable");
        assert_eq!(read["threads"][0]["comments"][0]["truncated"], true);

        let size = delivered(read, &github(&server));
        assert!(
            size <= MAX_TOOL_MESSAGE_CHARS,
            "a body that escapes to six times its length must still fit, got {size}"
        );
    }

    #[tokio::test]
    async fn an_escape_heavy_comment_reads_whole_across_pages() {
        let server = MockServer::start().await;
        let body = "line\n\"quoted\"\n".repeat(2_000);
        mock(
            &server,
            "pulls/comments/1",
            json!({
                "id": 1,
                "body": body,
                "html_url": "https://github.com/owner/repository/pull/7#discussion_r1",
                "created_at": "2026-11-05T00:00:00Z",
                "pull_request_url": "https://api.github.com/repos/owner/repository/pulls/7",
                "user": {"login": "coderabbitai"}
            }),
        )
        .await;
        let github = github(&server);
        let mut read = String::new();
        let mut offset = Some(0);
        let mut pages = 0;

        while let Some(next) = offset {
            let mut arguments = readiness_args(Some(7));
            arguments.comment = Some(1);
            arguments.offset = Some(next);
            let page = github
                .review_threads(&arguments)
                .await
                .expect("each page is readable");
            read.push_str(page["body"].as_str().expect("a body page"));
            offset = page["next"].as_u64();
            let size = delivered(page, &github);
            assert!(
                size <= MAX_TOOL_MESSAGE_CHARS,
                "an escaped page must survive the transcript cap whole, got {size}"
            );
            pages += 1;
            assert!(pages <= 50, "following next must finish");
        }

        assert_eq!(read, body, "the pages join back into the whole comment");
    }

    #[tokio::test]
    async fn a_cursor_whose_comment_is_gone_is_refused() {
        let server = MockServer::start().await;
        mock_graphql(&server, thread_payload(false, false)).await;
        let mut arguments = readiness_args(Some(7));
        arguments.after = Some(99);

        let refused = github(&server)
            .review_threads(&arguments)
            .await
            .expect_err("a cursor that no longer lands anywhere must not restart silently");

        assert!(
            refused.contains("no longer among these threads"),
            "{refused}"
        );
    }

    /// Threads with one comment each, numbered by their comment id, resolved
    /// where `resolved` says so.
    fn single_comment_threads(resolved: &[bool]) -> Value {
        let mut payload = long_thread_payload(1, 10);
        let nodes = &mut payload["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"];
        let template = nodes[0].clone();
        *nodes = json!(
            resolved
                .iter()
                .zip(1u64..)
                .map(|(resolved, id)| {
                    let mut thread = template.clone();
                    thread["id"] = json!(format!("PRRT_{id}"));
                    thread["isResolved"] = json!(resolved);
                    thread["comments"]["nodes"] = json!([review_comment_node(id, "comment")]);
                    thread
                })
                .collect::<Vec<_>>()
        );
        payload
    }

    async fn ids_after(payload: Value, resolved: bool, after: u64) -> Vec<u64> {
        let server = MockServer::start().await;
        mock_graphql(&server, payload).await;
        let mut arguments = readiness_args(Some(7));
        arguments.resolved = Some(resolved);
        arguments.after = Some(after);
        let read = github(&server)
            .review_threads(&arguments)
            .await
            .expect("the comment that marked the place still exists");
        read["threads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|thread| thread["comments"][0]["id"].as_u64().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn a_thread_resolved_between_pages_does_not_skip_or_repeat_its_neighbours() {
        let ids = ids_after(single_comment_threads(&[false, true, false]), false, 2).await;

        assert_eq!(
            ids,
            [3],
            "thread 2 was read unresolved and resolved since; reading goes on after it, not from the top"
        );
    }

    #[tokio::test]
    async fn a_thread_reopened_between_pages_does_not_skip_or_repeat_its_neighbours() {
        let ids = ids_after(single_comment_threads(&[true, false, true]), true, 2).await;

        assert_eq!(
            ids,
            [3],
            "thread 2 was read resolved and reopened since; reading goes on after it, not from the top"
        );
    }

    #[tokio::test]
    async fn a_cursor_that_cycles_ends_the_listing_without_repeating_threads() {
        let server = MockServer::start().await;
        let page = |id: u64, cursor: &str| {
            let mut payload = single_comment_threads(&[false]);
            let threads = &mut payload["data"]["repository"]["pullRequest"]["reviewThreads"];
            threads["nodes"][0]["id"] = json!(format!("PRRT_{id}"));
            threads["nodes"][0]["comments"]["nodes"][0]["databaseId"] = json!(id);
            threads["pageInfo"] = json!({"hasNextPage": true, "endCursor": cursor});
            payload
        };
        let cycles = THREAD_PAGE_LIMIT as u64 / 2;
        for (after, response, asked) in [
            (Value::Null, page(1, "A"), 1),
            (json!("A"), page(2, "B"), cycles),
            (json!("B"), page(1, "A"), cycles - 1),
        ] {
            Mock::given(method("POST"))
                .and(path("/graphql"))
                .and(body_partial_json(json!({"variables": {"after": after}})))
                .respond_with(ResponseTemplate::new(200).set_body_json(response))
                .expect(asked)
                .mount(&server)
                .await;
        }

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("a cycling cursor still reads what it found");

        assert_eq!(
            read["complete"], false,
            "a cycle is not the end of the list"
        );
        let ids: Vec<&Value> = read["threads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|thread| &thread["comments"][0]["id"])
            .collect();
        assert_eq!(
            ids,
            [&json!(1), &json!(2)],
            "the repeated page is not read twice"
        );
        assert_eq!(read["next_after"], Value::Null);
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            THREAD_PAGE_LIMIT,
            "a cycling cursor is followed no further than the page limit"
        );
    }

    /// The most pages of review threads the crate reads before it reports the
    /// walk as incomplete.
    const THREAD_PAGE_LIMIT: usize = 10;

    /// A page holding the one thread `id` names, ending at a cursor of its
    /// own, with another page following when `more`.
    fn thread_page(id: u64, more: bool) -> Value {
        let mut payload = single_comment_threads(&[false]);
        let threads = &mut payload["data"]["repository"]["pullRequest"]["reviewThreads"];
        threads["nodes"][0]["id"] = json!(format!("PRRT_{id}"));
        threads["nodes"][0]["comments"]["nodes"][0]["databaseId"] = json!(id);
        threads["pageInfo"] = json!({"hasNextPage": more, "endCursor": format!("cursor-{id}")});
        payload
    }

    async fn mock_thread_page(server: &MockServer, after: Value, response: Value) {
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(json!({"variables": {"after": after}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(server)
            .await;
    }

    fn comment_ids(read: &Value) -> Vec<u64> {
        read["threads"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|thread| thread["comments"].as_array().unwrap())
            .map(|comment| comment["id"].as_u64().unwrap())
            .collect()
    }

    #[tokio::test]
    async fn a_thread_listed_on_two_pages_is_read_once() {
        let server = MockServer::start().await;
        mock_thread_page(&server, Value::Null, thread_page(1, true)).await;
        let mut second = thread_page(2, false);
        let nodes = &mut second["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"];
        let repeated =
            thread_page(1, true)["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][0]
                .clone();
        nodes.as_array_mut().unwrap().insert(0, repeated);
        mock_thread_page(&server, json!("cursor-1"), second).await;

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("both pages are readable");

        assert_eq!(read["complete"], true);
        assert_eq!(
            comment_ids(&read),
            [1, 2],
            "a thread that moved onto the next page while it was read is one thread"
        );
        assert_eq!(read["counts"]["unresolved"], 2);
    }

    #[tokio::test]
    async fn threads_past_the_page_limit_read_as_incomplete() {
        let server = MockServer::start().await;
        let pages = THREAD_PAGE_LIMIT as u64;
        for id in 1..=pages {
            let after = match id {
                1 => Value::Null,
                id => json!(format!("cursor-{}", id - 1)),
            };
            mock_thread_page(&server, after, thread_page(id, true)).await;
        }
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(
                json!({"variables": {"after": format!("cursor-{pages}")}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(thread_page(pages + 1, false)))
            .expect(0)
            .mount(&server)
            .await;

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("the pages read so far are readable");

        assert_eq!(
            read["complete"], false,
            "a thread past the page limit may be the one still open"
        );
        assert_eq!(comment_ids(&read), (1..=pages).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn a_thread_reads_the_side_github_names_and_no_other() {
        let server = MockServer::start().await;
        let mut payload = thread_payload(false, false);
        let threads = &mut payload["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"];
        threads[0]["isResolved"] = json!(false);
        threads[0]["diffSide"] = json!("LEFT");
        threads[1].as_object_mut().unwrap().remove("diffSide");
        mock_graphql(&server, payload).await;

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("the threads are readable");

        assert_eq!(read["threads"][0]["side"], "LEFT");
        assert_eq!(
            read["threads"][1]["side"],
            Value::Null,
            "a side GitHub did not name is not guessed"
        );
    }

    #[tokio::test]
    async fn a_malformed_thread_response_is_an_error_not_a_panic() {
        for data in [
            json!([]),
            json!({"repository": "x"}),
            json!({"repository": {"pullRequest": {"number": 7, "reviewThreads": []}}}),
        ] {
            let server = MockServer::start().await;
            mock_graphql(&server, json!({ "data": data })).await;

            let refused = github(&server)
                .review_threads(&readiness_args(Some(7)))
                .await;

            assert!(refused.is_err(), "{data} must be refused, got {refused:?}");
        }
    }

    #[tokio::test]
    async fn offset_without_comment_is_refused() {
        let server = MockServer::start().await;
        mock_graphql(&server, thread_payload(false, false)).await;
        let mut arguments = readiness_args(Some(7));
        arguments.offset = Some(3);

        let refused = github(&server)
            .review_threads(&arguments)
            .await
            .expect_err("a character offset means nothing across threads");

        assert!(refused.contains("page the threads with after"), "{refused}");
    }

    #[tokio::test]
    async fn every_page_of_threads_is_read() {
        let server = MockServer::start().await;
        let mut first = thread_payload(true, false);
        first["data"]["repository"]["pullRequest"]["reviewThreads"]["pageInfo"]["endCursor"] =
            json!("cursor-1");
        first["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][1]["isResolved"] =
            json!(true);
        let mut second = long_thread_payload(1, 10);
        second["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][0]["comments"]["nodes"]
            [0]["databaseId"] = json!(51);
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(json!({"variables": {
                "owner": "owner",
                "name": "repository",
                "number": 7,
                "first": PAGE_SIZE,
                "after": null
            }})))
            .respond_with(ResponseTemplate::new(200).set_body_json(first))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(
                json!({"variables": {"after": "cursor-1"}}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(second))
            .expect(1)
            .mount(&server)
            .await;

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("every page is readable");

        assert_eq!(read["complete"], true);
        assert_eq!(
            read["counts"]["unresolved"], 1,
            "the open thread on the second page counts though the first page was all resolved"
        );
        assert_eq!(read["counts"]["resolved"], 2);
        assert_eq!(read["threads"][0]["comments"][0]["id"], 51);
        let cursors: Vec<Value> = server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|request| {
                serde_json::from_slice::<Value>(&request.body).unwrap()["variables"]["after"]
                    .clone()
            })
            .collect();
        assert_eq!(
            cursors,
            [Value::Null, json!("cursor-1")],
            "each page is asked for once, in order, after the cursor the page before ended at"
        );
    }

    #[tokio::test]
    async fn a_pull_request_that_does_not_exist_says_so() {
        let server = MockServer::start().await;
        mock_graphql(
            &server,
            json!({"data": {"repository": {"pullRequest": null}}, "errors": [{
                "type": "NOT_FOUND",
                "path": ["repository", "pullRequest"],
                "message": "Could not resolve to a PullRequest with the number of 9999."
            }]}),
        )
        .await;

        let refused = github(&server)
            .review_threads(&readiness_args(Some(9_999)))
            .await
            .expect_err("a missing pull request is not an access problem");

        assert!(
            refused.contains("no pull request with that number"),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn a_repository_github_will_not_show_is_an_access_problem() {
        let server = MockServer::start().await;
        mock_graphql(
            &server,
            json!({"data": {"repository": null}, "errors": [{
                "type": "NOT_FOUND",
                "path": ["repository"],
                "message": "Could not resolve to a Repository with the name 'owner/repository'."
            }]}),
        )
        .await;

        let refused = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect_err("a repository the token cannot see is not a wrong number");

        assert!(refused.contains("Check source access"), "{refused}");
    }

    #[tokio::test]
    async fn a_number_github_cannot_hold_is_refused_before_asking() {
        let server = MockServer::start().await;

        let refused = github(&server)
            .review_threads(&readiness_args(Some(i32::MAX as u64 + 1)))
            .await
            .expect_err("GraphQL numbers are 32-bit");

        assert!(
            refused.contains("larger than any pull request number"),
            "{refused}"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_outdated_range_keeps_both_ends_from_one_revision() {
        let server = MockServer::start().await;
        let mut payload = thread_payload(false, false);
        let thread = &mut payload["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][1];
        thread["startLine"] = Value::Null;
        thread["originalLine"] = json!(30);
        thread["originalStartLine"] = json!(25);
        mock_graphql(&server, payload).await;

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("the threads are readable");

        assert_eq!(read["threads"][0]["line"], 42);
        assert_eq!(
            read["threads"][0]["start_line"],
            Value::Null,
            "a current line must not be paired with a start from an older revision"
        );
    }

    #[tokio::test]
    async fn a_cut_comment_reads_in_full_by_id() {
        let server = MockServer::start().await;
        let body = "y".repeat(REVIEW_BODY_CHARS * 2);
        mock_graphql(&server, long_thread_payload(1, body.len())).await;
        mock(
            &server,
            "pulls/comments/1",
            json!({
                "id": 1,
                "body": body,
                "path": "src/lib.rs",
                "html_url": "https://github.com/owner/repository/pull/7#discussion_r1",
                "created_at": "2026-11-05T00:00:00Z",
                "pull_request_url": "https://api.github.com/repos/owner/repository/pulls/7",
                "user": {"login": "coderabbitai"}
            }),
        )
        .await;
        let github = github(&server);

        let threads = github
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("the threads are readable");
        let cut = &threads["threads"][0]["comments"][0];
        assert_eq!(cut["truncated"], true);
        assert_eq!(cut["id"], 1);

        let mut arguments = readiness_args(Some(7));
        arguments.comment = Some(1);
        let whole = github
            .review_threads(&arguments)
            .await
            .expect("the cut comment is readable by id");
        assert_eq!(whole["body"], body.as_str(), "nothing is cut this time");
        assert_eq!(whole["complete"], true);
        assert_eq!(whole["author"], "coderabbitai");
    }

    #[tokio::test]
    async fn a_comment_from_another_pull_request_is_refused() {
        let server = MockServer::start().await;
        mock(
            &server,
            "pulls/comments/1",
            json!({
                "id": 1,
                "body": "elsewhere",
                "html_url": "https://github.com/owner/repository/pull/70#discussion_r1",
                "created_at": "2026-11-05T00:00:00Z",
                "pull_request_url": "https://api.github.com/repos/owner/repository/pulls/70",
                "user": {"login": "someone"}
            }),
        )
        .await;
        let mut arguments = readiness_args(Some(7));
        arguments.comment = Some(1);

        let refused = github(&server)
            .review_threads(&arguments)
            .await
            .expect_err("a comment on pull request 70 is not feedback on 7");

        assert!(refused.contains("does not belong"), "{refused}");
    }

    #[tokio::test]
    async fn a_comment_reads_though_github_cases_the_repository_differently() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/Owner/Repository/pulls/comments/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 1,
                "body": "found it",
                "html_url": "https://github.com/owner/repository/pull/7#discussion_r1",
                "created_at": "2026-11-05T00:00:00Z",
                "pull_request_url": "https://api.github.com/repos/owner/repository/pulls/7",
                "user": {"login": "coderabbitai"}
            })))
            .mount(&server)
            .await;
        let mut arguments = readiness_args(Some(7));
        arguments.comment = Some(1);

        let read = github_named(&server, "Owner", "Repository")
            .review_threads(&arguments)
            .await
            .expect("a source saved as Owner/Repository owns owner/repository");

        assert_eq!(read["body"], "found it");
    }

    /// Read comment 1 of pull request `number` on `owner/repo`, which GitHub
    /// answers for with `answer`.
    async fn comment_read(
        owner: &str,
        repo: &str,
        number: u64,
        answer: Value,
    ) -> Result<Value, String> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/repos/{owner}/{repo}/pulls/comments/1")))
            .respond_with(ResponseTemplate::new(200).set_body_json(answer))
            .mount(&server)
            .await;
        let mut arguments = readiness_args(Some(number));
        arguments.comment = Some(1);
        github_named(&server, owner, repo)
            .review_threads(&arguments)
            .await
    }

    fn comment_on(pull_request_url: &str) -> Value {
        json!({
            "id": 1,
            "body": "found it",
            "html_url": "https://github.com/owner/repository/pull/182#discussion_r1",
            "created_at": "2026-11-05T00:00:00Z",
            "pull_request_url": pull_request_url,
            "user": {"login": "coderabbitai"}
        })
    }

    #[tokio::test]
    async fn a_pull_api_url_matches_its_own_number_only() {
        let url = "https://api.github.com/repos/owner/repository/pulls/182";
        assert!(
            comment_read("Owner", "Repository", 182, comment_on(url))
                .await
                .is_ok()
        );
        for (owner, number, pull_request_url) in [
            ("owner", 18, url),
            ("owner", 82, url),
            ("wner", 182, url),
            (
                "owner",
                182,
                "https://api.github.com/repos/owner/repository/issues/182",
            ),
        ] {
            let refused = comment_read(owner, "repository", number, comment_on(pull_request_url))
                .await
                .expect_err("a comment on another pull request is refused");
            assert_eq!(
                refused, FOREIGN_COMMENT,
                "{owner}#{number} at {pull_request_url}"
            );
        }
    }

    #[tokio::test]
    async fn a_comment_github_cannot_find_or_answers_for_under_another_id_is_refused() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository/pulls/comments/1"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({"message": "Not Found"})))
            .mount(&server)
            .await;
        let mut arguments = readiness_args(Some(182));
        arguments.comment = Some(1);
        assert_eq!(
            github(&server)
                .review_threads(&arguments)
                .await
                .unwrap_err(),
            FOREIGN_COMMENT,
            "a comment GitHub cannot find is not a comment on this pull request"
        );

        let mut other = comment_on("https://api.github.com/repos/owner/repository/pulls/182");
        other["id"] = json!(2);
        assert_eq!(
            comment_read("owner", "repository", 182, other)
                .await
                .unwrap_err(),
            FOREIGN_COMMENT,
            "an answer about another comment is not this one"
        );
    }

    #[tokio::test]
    async fn a_page_that_did_not_finish_says_so() {
        let server = MockServer::start().await;
        mock_graphql(&server, thread_payload(true, true)).await;

        let read = github(&server)
            .review_threads(&readiness_args(Some(7)))
            .await
            .expect("the threads are readable");

        assert_eq!(read["complete"], false);
        assert_eq!(read["threads"][0]["comments_complete"], false);
    }

    #[tokio::test]
    async fn review_threads_for_another_pull_request_are_refused() {
        let server = MockServer::start().await;
        mock_graphql(&server, thread_payload(false, false)).await;

        let refused = github(&server)
            .review_threads(&readiness_args(Some(8)))
            .await
            .expect_err("threads numbered for a different pull request are not evidence");

        assert!(refused.contains("different or missing"), "{refused}");
    }

    fn pull_detail(head: &str) -> Value {
        json!({"node_id": "PR_7", "number": 7, "draft": false, "state": "open",
               "title": "Ship the billing export",
               "html_url": "https://github.com/owner/repository/pull/7",
               "updated_at": "2026-09-05T00:00:00Z",
               "head": {"ref": "feature", "sha": head},
               "base": {"ref": "main", "sha": COMMIT}})
    }

    async fn mock_pull(server: &MockServer, head: &str) {
        mock(server, "pulls/7", pull_detail(head)).await;
    }

    fn listed_pull(draft: bool) -> Value {
        json!({"node_id": "PR_7", "number": 7, "draft": draft, "state": "open",
               "title": "Ship the billing export",
               "html_url": "https://github.com/owner/repository/pull/7",
               "updated_at": "2026-09-05T00:00:00Z",
               "head": {"ref": "feature", "label": "owner:feature", "sha": COMMIT},
               "base": {"ref": "main", "label": "owner:main"}})
    }

    async fn mock_green_ci(server: &MockServer) {
        mock_green_ci_answering(server, Some(BEARER)).await;
    }

    async fn mock_green_ci_answering(server: &MockServer, authorization: Option<&str>) {
        mock_answering(
            server,
            "actions/runs",
            json!({"workflow_runs": [{"id": 11, "workflow_id": 1, "name": "CI",
                   "path": ".github/workflows/ci.yml", "event": "pull_request",
                   "head_sha": COMMIT, "status": "completed", "conclusion": "success",
                   "created_at": "2026-09-05T00:00:00Z", "updated_at": "2026-09-05T00:05:00Z",
                   "html_url": "https://github.com/owner/repository/actions/runs/11",
                   "repository": {"full_name": "owner/repository"}}], "total_count": 1}),
            authorization,
        )
        .await;
        mock_answering(
            server,
            &format!("commits/{COMMIT}/check-runs"),
            json!({"check_runs": [{"id": 21, "name": "test", "head_sha": COMMIT, "status": "completed", "conclusion": "success"}]}),
            authorization,
        )
        .await;
        mock_answering(
            server,
            &format!("commits/{COMMIT}/status"),
            json!({"statuses": [], "total_count": 0}),
            authorization,
        )
        .await;
    }

    #[tokio::test]
    async fn a_source_without_a_credential_reads_without_an_authorization_header() {
        let server = MockServer::start().await;
        mock_answering(&server, "commits/main", json!({"sha": COMMIT}), None).await;
        mock_green_ci_answering(&server, None).await;
        let github = github_with_token(&server, None);
        assert_eq!(
            github.resolve(None).await.unwrap(),
            ("main".to_string(), COMMIT.to_string())
        );
        assert_eq!(
            github.settled(None).await.unwrap(),
            ("main".to_string(), COMMIT.to_string(), SUCCESS_ASSESSMENT)
        );
        assert_sent_without_authorization(&server).await;
    }

    #[tokio::test]
    async fn a_source_with_an_empty_token_reads_anonymously() {
        let server = MockServer::start().await;
        mock_answering(&server, "commits/main", json!({"sha": COMMIT}), None).await;
        mock_green_ci_answering(&server, None).await;
        let (reference, sha, state) = github_with_token(&server, Some(""))
            .settled(None)
            .await
            .unwrap();
        assert_eq!((reference.as_str(), sha.as_str()), ("main", COMMIT));
        assert_eq!(state, SUCCESS_ASSESSMENT);
        assert_sent_without_authorization(&server).await;
    }

    #[tokio::test]
    async fn a_tokenless_graphql_read_is_refused_unsent_as_unauthorized() {
        let server = MockServer::start().await;
        mock_answering(&server, "pulls/7", pull_detail(COMMIT), None).await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(thread_counts(true, false)))
            .expect(0)
            .mount(&server)
            .await;
        let error = github_with_token(&server, None)
            .observe(Operation::PullRequests, &readiness_args(Some(7)))
            .await
            .unwrap_err();
        assert_eq!(error, status_error(UNAUTHORIZED));
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.url.path() != "/graphql"),
            "GitHub answers no GraphQL call without a token, so none may be sent"
        );
        assert_sent_without_authorization(&server).await;
    }

    #[tokio::test]
    async fn a_tokenless_job_log_is_fetched_without_an_authorization_header() {
        let server = MockServer::start().await;
        mock_answering(&server, "commits/main", json!({"sha": COMMIT}), None).await;
        mock_answering(
            &server,
            &format!("actions/jobs/{JOB_ID}"),
            job(COMMIT),
            None,
        )
        .await;
        mock_log(&server, noisy_log(40, 10)).await;
        let result = github_with_token(&server, None)
            .observe(Operation::CheckLogs, &check_args(Some(JOB_ID), None, None))
            .await
            .unwrap();
        assert!(
            result["log"]["excerpt"]
                .as_str()
                .is_some_and(|text| text.contains("##[error]")),
            "{result}"
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .any(|request| request.url.path().ends_with("/logs")),
            "the log's first hop must have been sent"
        );
        assert_sent_without_authorization(&server).await;
    }

    #[tokio::test]
    async fn a_refused_job_log_request_names_its_status() {
        for status in [500, 502] {
            let server = MockServer::start().await;
            mock(&server, "commits/main", json!({"sha": COMMIT})).await;
            mock(&server, &format!("actions/jobs/{JOB_ID}"), job(COMMIT)).await;
            Mock::given(method("GET"))
                .and(path(format!(
                    "/repos/owner/repository/actions/jobs/{JOB_ID}/logs"
                )))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(json!({"message": "test-secret"})),
                )
                .expect(1)
                .mount(&server)
                .await;
            let error = github(&server)
                .observe(Operation::CheckLogs, &check_args(Some(JOB_ID), None, None))
                .await
                .unwrap_err();
            assert_eq!(error, status_error(status), "HTTP {status}");
            assert!(!error.contains("test-secret"), "HTTP {status}: {error}");
        }
    }

    #[tokio::test]
    async fn coherent_green_evidence_is_ready_and_cites_the_head_commit() {
        let server = MockServer::start().await;
        mock_pull(&server, COMMIT).await;
        mock_review(&server, true, &summary(5, COMMIT), false).await;
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
        mock(&server, "pulls", json!([listed_pull(false)])).await;
        mock_pull(&server, BLOB).await;
        mock_review(&server, true, &summary(5, BLOB), false).await;
        mock_green_ci(&server).await;
        let result = github(&server)
            .observe(Operation::PullRequests, &readiness_args(None))
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
        mock_review(&server, true, &summary(5, COMMIT), true).await;
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
        assert_eq!(error, REVIEW_QUERY_REJECTED);
        assert!(!error.contains("test-secret"));
        server.reset().await;
        mock_pull(&server, COMMIT).await;
        mock_graphql(&server, json!({"data": {"repository": Value::Null}})).await;
        assert_eq!(
            github(&server)
                .observe(Operation::PullRequests, &readiness_args(Some(7)))
                .await
                .unwrap_err(),
            REVIEW_QUERY_REJECTED,
            "a repository GitHub's review query cannot see reads as a rejected query"
        );
        server.reset().await;
        let mut other = pull_detail(COMMIT);
        other["number"] = json!(8);
        mock(&server, "pulls/7", other).await;
        mock_review(&server, true, &summary(5, COMMIT), false).await;
        assert_eq!(
            github(&server)
                .observe(Operation::PullRequests, &readiness_args(Some(7)))
                .await
                .unwrap_err(),
            MISMATCHED_PULL
        );
    }

    #[tokio::test]
    async fn a_pull_request_the_review_query_cannot_find_says_so() {
        for answer in [
            json!({"data": {"repository": {"pullRequest": Value::Null}}}),
            json!({"data": {"repository": {"pullRequest": Value::Null}}, "errors": [{
                "type": "NOT_FOUND",
                "path": ["repository", "pullRequest"],
                "message": "Could not resolve to a PullRequest with the number of 7."
            }]}),
        ] {
            let server = MockServer::start().await;
            mock_pull(&server, COMMIT).await;
            mock_graphql(&server, answer.clone()).await;

            assert_eq!(
                github(&server)
                    .observe(Operation::PullRequests, &readiness_args(Some(7)))
                    .await
                    .unwrap_err(),
                MISSING_PULL,
                "{answer}"
            );
        }
    }

    #[tokio::test]
    async fn review_threads_counted_for_another_pull_request_fail_closed() {
        for number in [json!(8), Value::Null] {
            let server = MockServer::start().await;
            mock_pull(&server, COMMIT).await;
            let mut counts = thread_counts(true, false);
            counts["data"]["repository"]["pullRequest"]["number"] = number.clone();
            mock_graphql(&server, counts).await;

            assert_eq!(
                github(&server)
                    .observe(Operation::PullRequests, &readiness_args(Some(7)))
                    .await
                    .unwrap_err(),
                MISMATCHED_PULL,
                "threads numbered {number} are not pull request 7's"
            );
        }
    }

    #[tokio::test]
    async fn pull_requests_without_a_number_or_draft_state_fail_closed() {
        let server = MockServer::start().await;
        let mut thin = listed_pull(false);
        thin.as_object_mut().unwrap().remove("draft");
        mock(&server, "pulls", json!([thin])).await;
        assert_eq!(
            github(&server)
                .observe(Operation::PullRequests, &readiness_args(None))
                .await
                .unwrap_err(),
            UNEXPECTED_RESPONSE,
            "a listed pull request that does not say whether it is a draft is unreadable"
        );
        server.reset().await;
        mock(&server, "pulls/7", json!({"draft": false})).await;
        assert_eq!(
            github(&server)
                .observe(Operation::PullRequests, &readiness_args(Some(7)))
                .await
                .unwrap_err(),
            UNEXPECTED_RESPONSE,
            "a pull request without a number is unreadable"
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
    async fn a_pull_request_read_by_number_without_its_draft_state_reads_as_not_a_draft() {
        let server = MockServer::start().await;
        let mut detail = pull_detail(COMMIT);
        detail.as_object_mut().unwrap().remove("draft");
        mock(&server, "pulls/7", detail).await;
        mock_review(&server, true, &summary(5, COMMIT), false).await;
        mock_green_ci(&server).await;
        let result = github(&server)
            .observe(Operation::PullRequests, &readiness_args(Some(7)))
            .await
            .unwrap();
        let pull = &result["pull_requests"][0];
        assert_eq!(pull["draft"], false, "{result}");
        assert_eq!(pull["ready"], true, "{result}");
    }

    #[tokio::test]
    async fn a_hundred_review_comments_are_complete_and_a_hundred_and_one_are_not() {
        for (count, complete) in [(100, true), (101, false)] {
            let server = MockServer::start().await;
            mock_pull(&server, COMMIT).await;
            mock_graphql(&server, thread_counts(true, false)).await;
            mock_green_ci(&server).await;
            let mut comments: Vec<Value> = (1..count)
                .map(|id| conversation_comment(id, "reviewer", "Looks reasonable."))
                .collect();
            comments.push(conversation_comment(
                count,
                "greptile-apps",
                &summary(5, COMMIT),
            ));
            let rest = comments.split_off(PAGE_SIZE.min(comments.len()));
            for (page, rows) in [("1", comments), ("2", rest)] {
                Mock::given(method("GET"))
                    .and(path("/repos/owner/repository/issues/7/comments"))
                    .and(query_param("per_page", "100"))
                    .and(query_param("page", page))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!(rows)))
                    .expect(1)
                    .mount(&server)
                    .await;
            }
            let result = github(&server)
                .observe(Operation::PullRequests, &readiness_args(Some(7)))
                .await
                .unwrap();
            let pull = &result["pull_requests"][0];
            assert_eq!(pull["comments_complete"], complete, "{count}: {result}");
            assert_eq!(
                take_array(pull, "blockers")
                    .iter()
                    .any(|blocker| blocker["code"] == "comments_incomplete"),
                !complete,
                "{count}: {result}"
            );
            assert_eq!(pull["ready"], complete, "{count}: {result}");
        }
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
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([listed_pull(true)])))
            .expect(1)
            .mount(&server)
            .await;
        mock_pull(&server, COMMIT).await;
        mock_review(&server, true, &summary(5, COMMIT), false).await;
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
        let commit = Revision::parse(COMMIT).unwrap();
        let run = WorkflowRun::new(
            NonZeroU64::new(11).unwrap(),
            NonZeroU64::new(1).unwrap(),
            ".github/workflows/ci.yml",
            "push",
            commit.clone(),
            "2026-09-05T00:00:00Z",
            "2026-09-05T00:05:00Z",
            "https://github.com/owner/repository/actions/runs/11",
            "owner/repository",
        );
        assert_eq!(
            run_token(
                &run.clone()
                    .with_status("completed")
                    .with_conclusion("failure")
            ),
            "failure"
        );
        assert_eq!(run_token(&run.clone().with_status("completed")), "unknown");
        assert_eq!(
            run_token(&run),
            "unknown",
            "a run without a status has not said anything"
        );
        assert_eq!(check_token(&CheckRun::new("queued")), "queued");
        assert_eq!(check_token(&CheckRun::new("in_progress")), "in_progress");
        assert_eq!(
            check_token(&CheckRun::new("completed").with_conclusion("success")),
            "success"
        );
        let statuses = [CommitStatus::new("lint", "pending")];
        assert_eq!(conclusions(&[run], &[], &statuses), ["unknown", "pending"]);
        assert_eq!(ci_priority(&json!({"status": "queued"})), 1);
        assert_eq!(
            ci_priority(&json!({"status": "completed", "conclusion": "failure"})),
            0
        );
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
                offline: false,
                origin: crate::db::tasks::Origin::Member,
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
            directory: None,
            number,
            resolved: None,
            comment: None,
            after: None,
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
            json!({"number": number, "node_id": format!("PR_{number}"), "title": "Fix",
                   "state": "open", "draft": false,
                   "html_url": format!("https://github.com/owner/repository/pull/{number}"),
                   "head": {"sha": head_sha, "ref": "feature"},
                   "base": {"sha": COMMIT, "ref": "main"}}),
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
        let at = |run: u64, job: u64| -> String {
            format!("https://github.com/owner/repository/actions/runs/{run}/job/{job}")
        };
        let fetched = |id: u64, head_sha: &str, url: String| {
            WorkflowJob::new(
                NonZeroU64::new(id).unwrap(),
                NonZeroU64::new(RUN_ID).unwrap(),
                "build",
                "completed",
                Revision::parse(head_sha).unwrap(),
            )
            .with_conclusion("failure")
            .with_url(url)
        };
        let foreign_repository = fetched(
            JOB_ID,
            COMMIT,
            format!("https://github.com/attacker/private/actions/runs/{RUN_ID}/job/{JOB_ID}"),
        );
        let renumbered = fetched(JOB_ID + 1, COMMIT, at(RUN_ID, JOB_ID));
        let elsewhere = fetched(JOB_ID, BLOB, at(RUN_ID, JOB_ID));
        let unlinked = WorkflowJob::new(
            NonZeroU64::new(JOB_ID).unwrap(),
            NonZeroU64::new(RUN_ID).unwrap(),
            "build",
            "completed",
            Revision::parse(COMMIT).unwrap(),
        );
        for metadata in [foreign_repository, renumbered, elsewhere, unlinked] {
            assert_eq!(
                github(&server).verify_job(&metadata, JOB_ID, COMMIT),
                Err(FOREIGN_JOB.to_string())
            );
        }
        assert_eq!(
            github(&server).verify_job(
                &fetched(JOB_ID, COMMIT, at(RUN_ID, JOB_ID)),
                JOB_ID,
                COMMIT
            ),
            Ok(RUN_ID)
        );
        assert!(
            github(&server)
                .verify_job(
                    &fetched(JOB_ID, COMMIT, at(RUN_ID, JOB_ID).to_uppercase()),
                    JOB_ID,
                    COMMIT
                )
                .is_ok()
        );

        mock(&server, "commits/main", json!({"sha": COMMIT})).await;
        mock_log(&server, "secret log".into()).await;
        for (metadata, expected) in [
            (
                json!({"id": JOB_ID, "head_sha": COMMIT}),
                UNEXPECTED_RESPONSE,
            ),
            (
                json!({"id": JOB_ID, "run_id": RUN_ID, "name": "build",
                       "status": "completed", "head_sha": "abc123"}),
                JOB_WITHOUT_COMMIT,
            ),
        ] {
            let thin = Mock::given(method("GET"))
                .and(path(format!(
                    "/repos/owner/repository/actions/jobs/{JOB_ID}"
                )))
                .respond_with(ResponseTemplate::new(200).set_body_json(metadata.clone()))
                .mount_as_scoped(&server)
                .await;
            assert_eq!(
                github(&server)
                    .observe(Operation::CheckLogs, &check_args(Some(JOB_ID), None, None))
                    .await
                    .unwrap_err(),
                expected,
                "{metadata}"
            );
            drop(thin);
        }
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| !request.url.path().ends_with("/logs")),
            "a job that cannot be read is never followed to its log"
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
                    .map(|filename| json!({"filename": filename, "status": "modified", "additions": 1, "deletions": 0}))
                    .collect::<Vec<_>>()
            ),
        )
        .await;
    }

    async fn assess_green_pull(server: &MockServer) -> Value {
        mock_pull(server, COMMIT).await;
        mock_review(server, true, &summary(5, COMMIT), false).await;
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
            json!([
                {"filename": "src/auth/token.rs", "additions": 1, "deletions": 0},
                {"status": "modified", "additions": 1, "deletions": 0}
            ]),
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
                mock_review(
                    &server,
                    true,
                    &summary(if green { 5 } else { 3 }, COMMIT),
                    false,
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
        mock_review(&server, true, &summary(5, COMMIT), false).await;
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
        let config = crate::state::test_config();
        assert_eq!(
            Github::new(configuration(None), &config)
                .unwrap()
                .signals
                .reviewers(),
            ReviewSignals::recognized().reviewers()
        );
        assert_eq!(
            Github::new(configuration(Some(vec!["greptile".into()])), &config)
                .unwrap()
                .signals
                .reviewers(),
            vec!["greptile-apps".to_string()]
        );
        let error = Github::new(configuration(Some(vec!["sonarcloud".into()])), &config)
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
                "draft": false,
                "prerelease": false,
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
                "draft": false,
                "prerelease": false,
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
            json!([{"id": 90, "tag_name": "", "published_at": "2026-09-05T00:00:00Z",
                    "html_url": "https://github.com/owner/repository/releases/tag/",
                    "draft": false, "prerelease": false}]),
        )
        .await;
        assert_eq!(
            github(&server)
                .observe(Operation::ReleasePipelines, &release_args(None))
                .await
                .unwrap_err(),
            UNUSABLE_RELEASE
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
                    "draft": false,
                    "prerelease": false,
                },
                {
                    "id": 89,
                    "tag_name": "v1.3.0",
                    "published_at": "2026-09-01T00:00:00Z",
                    "html_url": "https://github.com/owner/repository/releases/tag/v1.3.0",
                    "draft": false,
                    "prerelease": false,
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

    async fn commit_and_empty_ci(server: &MockServer) {
        mock(server, "commits/main", json!({"sha": COMMIT})).await;
        mock(
            server,
            "actions/runs",
            json!({"workflow_runs": [], "total_count": 0}),
        )
        .await;
        mock_statuses(server, json!([])).await;
    }

    async fn mock_statuses(server: &MockServer, statuses: Value) {
        let total = statuses.as_array().map_or(0, Vec::len);
        mock(
            server,
            &format!("commits/{COMMIT}/status"),
            json!({"statuses": statuses, "total_count": total}),
        )
        .await;
    }

    async fn check_runs(server: &MockServer, rows: Value) {
        mock(
            server,
            &format!("commits/{COMMIT}/check-runs"),
            json!({ "check_runs": rows }),
        )
        .await;
    }

    #[tokio::test]
    async fn a_running_check_does_not_settle_until_it_succeeds() {
        let server = MockServer::start().await;
        commit_and_empty_ci(&server).await;
        Mock::given(path(format!(
            "/repos/owner/repository/commits/{COMMIT}/check-runs"
        )))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                json!({"check_runs": [{"head_sha": COMMIT, "status": "in_progress"}]}),
            ),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
        check_runs(
            &server,
            json!([{"head_sha": COMMIT, "status": "completed", "conclusion": "success"}]),
        )
        .await;
        let github = github(&server);
        let (reference, sha, running) = github.settled(None).await.unwrap();
        assert_eq!(reference, "main");
        assert_eq!(sha, COMMIT);
        assert_eq!(running, "pending");
        assert!(
            !SETTLED_ASSESSMENTS.contains(&running),
            "a check still running must not end a wait"
        );
        let (_, _, finished) = github.settled(None).await.unwrap();
        assert_eq!(finished, "success");
        assert!(
            SETTLED_ASSESSMENTS.contains(&finished),
            "a successful check must end a wait"
        );
    }

    #[tokio::test]
    async fn a_failed_check_settles_the_wait() {
        let server = MockServer::start().await;
        commit_and_empty_ci(&server).await;
        check_runs(
            &server,
            json!([{"head_sha": COMMIT, "status": "completed", "conclusion": "failure"}]),
        )
        .await;
        let (_, _, state) = github(&server).settled(None).await.unwrap();
        assert_eq!(state, "failure");
        assert!(
            SETTLED_ASSESSMENTS.contains(&state),
            "a failed check must end a wait"
        );
    }

    /// The whole point of the finding: at the Checks level, a run GitHub has
    /// completed with a skipped conclusion ends a wait, and one it has only
    /// queued does not. The first used to be indistinguishable from a
    /// repository with no CI at all.
    #[tokio::test]
    async fn a_completed_skipped_run_settles_the_wait_and_a_queued_one_does_not() {
        let server = MockServer::start().await;
        commit_and_empty_ci(&server).await;
        check_runs(
            &server,
            json!([{"head_sha": COMMIT, "status": "completed", "conclusion": "skipped"}]),
        )
        .await;
        let (_, _, state) = github(&server).settled(None).await.unwrap();
        assert_eq!(state, "skipped");
        assert!(
            SETTLED_ASSESSMENTS.contains(&state),
            "a run that completed and said skipped has reported, so the wait is over"
        );

        let pending = MockServer::start().await;
        commit_and_empty_ci(&pending).await;
        check_runs(
            &pending,
            json!([{"head_sha": COMMIT, "status": "queued", "conclusion": null}]),
        )
        .await;
        let (_, _, state) = github(&pending).settled(None).await.unwrap();
        assert!(
            !SETTLED_ASSESSMENTS.contains(&state),
            "a queued run has not reported anything yet: {state}"
        );
    }

    #[tokio::test]
    async fn a_commit_with_no_checks_is_unknown_and_never_settles() {
        let server = MockServer::start().await;
        commit_and_empty_ci(&server).await;
        check_runs(&server, json!([])).await;
        let (_, _, state) = github(&server).settled(None).await.unwrap();
        assert_eq!(state, "unknown");
        assert!(
            !SETTLED_ASSESSMENTS.contains(&state),
            "a commit GitHub has registered no run for is not a settled wait"
        );
    }

    #[tokio::test]
    async fn settled_refuses_checks_reported_for_another_commit() {
        let server = MockServer::start().await;
        commit_and_empty_ci(&server).await;
        check_runs(
            &server,
            json!([{"head_sha": BLOB, "status": "completed", "conclusion": "success"}]),
        )
        .await;
        assert!(
            github(&server)
                .settled(None)
                .await
                .unwrap_err()
                .contains("different")
        );
    }

    #[tokio::test]
    async fn check_rows_that_shift_between_pages_never_settle_as_success() {
        let server = MockServer::start().await;
        commit_and_empty_ci(&server).await;
        let first: Vec<Value> = (1..=100)
            .map(|id| {
                json!({"id": id, "head_sha": COMMIT, "status": "completed", "conclusion": "success"})
            })
            .collect();
        Mock::given(path(format!(
            "/repos/owner/repository/commits/{COMMIT}/check-runs"
        )))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"check_runs": first})))
        .mount(&server)
        .await;
        Mock::given(path(format!(
            "/repos/owner/repository/commits/{COMMIT}/check-runs"
        )))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"check_runs": [
                {"id": 100, "head_sha": COMMIT, "status": "completed", "conclusion": "success"}
            ]})),
        )
        .mount(&server)
        .await;
        let github = github(&server);

        let (_, _, state) = github.settled(None).await.unwrap();
        assert_eq!(state, PENDING_ASSESSMENT);
        assert!(
            !SETTLED_ASSESSMENTS.contains(&state),
            "a check read twice may have pushed another past the pages read"
        );

        let build = github.build(COMMIT).await.unwrap();
        assert_eq!(build["complete"], false);
        assert_ne!(build["state"], SUCCESS_ASSESSMENT);

        let evidence = github.check_evidence(COMMIT).await.unwrap();
        assert!(!evidence.complete);
        assert_ne!(evidence.normalize().state, readiness::CheckState::Success);
    }

    async fn rate_limited(server: &MockServer, response: ResponseTemplate) {
        Mock::given(path("/repos/owner/repository/commits/main"))
            .respond_with(response)
            .up_to_n_times(1)
            .with_priority(1)
            .mount(server)
            .await;
        mock(server, "commits/main", json!({"sha": COMMIT})).await;
    }

    #[tokio::test]
    async fn a_retry_after_in_seconds_is_honoured_once() {
        let server = MockServer::start().await;
        rate_limited(
            &server,
            ResponseTemplate::new(429).insert_header(RETRY_AFTER_HEADER, "3"),
        )
        .await;
        let started = Instant::now();
        let (_, sha) = github(&server).resolve(None).await.unwrap();
        let waited = started.elapsed();
        assert_eq!(sha, COMMIT);
        assert!(
            waited >= Duration::from_secs(3),
            "Retry-After was not honoured, waited {waited:?}"
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            2,
            "a rate-limited request is retried exactly once, never in a loop"
        );
    }

    #[tokio::test]
    async fn a_retry_after_http_date_is_honoured_once() {
        const DATE_FORMAT: &str = "%a, %d %b %Y %H:%M:%S GMT";
        const DISTANCE_SECONDS: i64 = 4;

        let server = MockServer::start().await;
        let at = (Utc::now() + chrono::Duration::seconds(DISTANCE_SECONDS))
            .format(DATE_FORMAT)
            .to_string();
        // A date names an instant, so the retry is held to that instant and not
        // to an elapsed floor a slow run exhausts before the wait even begins.
        let date = DateTime::parse_from_rfc2822(&at)
            .unwrap()
            .with_timezone(&Utc);
        rate_limited(
            &server,
            ResponseTemplate::new(429).insert_header(RETRY_AFTER_HEADER, at.as_str()),
        )
        .await;
        let (_, sha) = github(&server).resolve(None).await.unwrap();
        let retried = Utc::now();
        assert_eq!(sha, COMMIT);
        assert!(
            retried >= date,
            "an HTTP-date Retry-After was not honoured, retried at {retried} before {date}"
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            2,
            "a rate-limited request is retried exactly once, never in a loop"
        );
    }

    #[tokio::test]
    async fn an_exhausted_rate_limit_is_reported_and_never_retried() {
        let server = MockServer::start().await;
        for status in [403, 429] {
            server.reset().await;
            rate_limited(
                &server,
                ResponseTemplate::new(status)
                    .insert_header(RATE_LIMIT_REMAINING_HEADER, "0")
                    .insert_header(RETRY_AFTER_HEADER, "1"),
            )
            .await;
            let error = github(&server).resolve(None).await.unwrap_err();
            assert_eq!(error, RATE_LIMIT_EXHAUSTED, "HTTP {status}");
            assert_eq!(
                server.received_requests().await.unwrap().len(),
                1,
                "an exhausted quota must not be retried"
            );
        }
    }

    #[tokio::test]
    async fn a_forbidden_response_without_an_exhausted_quota_keeps_its_status() {
        let server = MockServer::start().await;
        mock(&server, "commits/main", json!({"sha": COMMIT})).await;
        Mock::given(path("/repos/owner/repository/commits/main"))
            .respond_with(ResponseTemplate::new(403))
            .with_priority(1)
            .mount(&server)
            .await;
        let error = github(&server).resolve(None).await.unwrap_err();
        assert!(error.contains("HTTP 403"), "{error}");
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    async fn retried(retry_after: &str) -> (Result<(String, String), String>, usize) {
        let server = MockServer::start().await;
        rate_limited(
            &server,
            ResponseTemplate::new(429).insert_header(RETRY_AFTER_HEADER, retry_after),
        )
        .await;
        let resolved = github(&server).resolve(None).await;
        (resolved, server.received_requests().await.unwrap().len())
    }

    fn soon(format: &str) -> String {
        (Utc::now() + chrono::Duration::seconds(2))
            .format(format)
            .to_string()
    }

    #[tokio::test]
    async fn retry_after_values_the_crate_will_not_wait_on_fail_at_once() {
        const IMF_FIXDATE: &str = "%a, %d %b %Y %H:%M:%S GMT";
        let distant = (Utc::now() + chrono::Duration::seconds(120))
            .format(IMF_FIXDATE)
            .to_string();
        for value in [
            soon("%a, %d %b %Y %H:%M:%S +0000"),
            "61".to_string(),
            "18446744073709551616".to_string(),
            "Thu, 01 Jan 1969 00:00:00 GMT".to_string(),
            soon(IMF_FIXDATE).to_lowercase().replace("gmt", "GMT"),
            soon("%d %b %Y %H:%M:%S GMT"),
            String::new(),
            "soon".to_string(),
            distant,
        ] {
            let (resolved, requests) = retried(&value).await;
            assert_eq!(
                resolved.unwrap_err(),
                status_error(TOO_MANY_REQUESTS),
                "{value:?}"
            );
            assert_eq!(requests, 1, "{value:?} was waited on");
        }
    }

    #[tokio::test]
    async fn retry_after_dates_in_every_rfc_9110_form_are_waited_on_once() {
        for value in [
            "Sun, 06 Nov 1994 08:48:00 GMT".to_string(),
            " 1 ".to_string(),
            soon("%A, %d-%b-%y %H:%M:%S GMT"),
            soon("%a %b %e %H:%M:%S %Y"),
        ] {
            let (resolved, requests) = retried(&value).await;
            assert_eq!(resolved.unwrap().1, COMMIT, "{value:?}");
            assert_eq!(requests, 2, "{value:?} was not retried exactly once");
        }
    }
}
