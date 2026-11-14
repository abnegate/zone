mod review;

use abnegate_secret::SecretValue;
use abnegate_vcs::BranchName;
use abnegate_vcs::CheckRun;
use abnegate_vcs::Collected;
use abnegate_vcs::CommitSha as Revision;
use abnegate_vcs::CommitStatus;
use abnegate_vcs::IssueComment;
use abnegate_vcs::IssueReference;
use abnegate_vcs::JobLogError;
use abnegate_vcs::ListedPullRequest;
use abnegate_vcs::MergeableState;
use abnegate_vcs::Page;
use abnegate_vcs::ParseError;
use abnegate_vcs::PullRequestDetail;
use abnegate_vcs::PullRequestError;
use abnegate_vcs::PullRequestReference;
use abnegate_vcs::PullRequestService;
use abnegate_vcs::Release;
use abnegate_vcs::Repository;
use abnegate_vcs::ReviewThreadCount;
use abnegate_vcs::ReviewThreadRecord;
use abnegate_vcs::WorkflowJob;
use abnegate_vcs::WorkflowRun;
use abnegate_vcs::WorkflowRunFilter;
use std::num::NonZeroU8;
use std::num::NonZeroU32;
use std::num::NonZeroU64;
use std::sync::LazyLock;
use std::time::Duration;

use super::configuration::Configuration;
use super::readiness;
use super::readiness::CheckEvidence;
use super::readiness::CommentEvidence;
use super::readiness::CommitSha;
use super::readiness::Mergeability;
use super::readiness::PullEvidence;
use super::readiness::ReviewComment;
use super::readiness::ReviewSignals;
use super::readiness::ReviewThread;
use super::readiness::ThreadEvidence;
use super::release::ReleaseIdentity;
use super::release::ReleasePipeline;
use super::release::RunEvidence;
use crate::config::Config;
use crate::services::prioritisation::Configuration as Prioritisation;
use crate::services::prioritisation::Prioritiser;
use crate::services::prioritisation::RiskSignal;
use crate::services::prioritisation::pull_request;
use crate::utils::text::truncate;
use review::Review;

const API_VERSION: &str = "2026-03-10";
pub(crate) const PER_PAGE: NonZeroU8 = NonZeroU8::new(100).expect("a page holds rows");
const PULL_TITLE_CHARS: usize = 160;
const REVIEW_PAGE_SIZE: usize = 100;
const NEWEST_COMMENT_PAGES: u32 = 2;
const PULL_FILE_PAGES: u32 = 30;
pub(crate) const MEBIBYTE: u64 = 1024 * 1024;
const MAX_LOG_BYTES: u64 = 16 * MEBIBYTE;
const REVIEW_QUERY_REJECTED: &str =
    "GitHub rejected the review query. Check source access, permissions and rate limits.";
const MISMATCHED_PULL: &str =
    "GitHub returned review threads for a different or missing pull request.";
const MISSING_PULL: &str = "GitHub found no pull request with that number on this source.";
const ANSWERED_ANOTHER_PULL: &str =
    "GitHub answered for a pull request other than the one asked for";
pub(crate) const SUCCESS_ASSESSMENT: &str = "success";
pub(crate) const FAILURE_ASSESSMENT: &str = "failure";
pub(crate) const PENDING_ASSESSMENT: &str = "pending";
pub(crate) const UNKNOWN_ASSESSMENT: &str = "unknown";

/// Conclusions a completed run reports that are neither a pass nor a failure.
/// The run finished and said so, which is the opposite of the silence
/// `unknown` stands for, so a wait on them ends — named, and never as a pass.
const TERMINAL_ASSESSMENTS: [&str; 2] = ["neutral", "skipped"];

pub(crate) const SETTLED_ASSESSMENTS: [&str; 4] = [
    SUCCESS_ASSESSMENT,
    FAILURE_ASSESSMENT,
    TERMINAL_ASSESSMENTS[0],
    TERMINAL_ASSESSMENTS[1],
];
pub(crate) const RATE_LIMIT_EXHAUSTED: &str = "GitHub's API rate limit is exhausted for this source's credential. Retrying now will not help; wait for the limit to reset.";
pub(crate) const REQUEST_FAILED: &str = "GitHub request failed or timed out.";
const INVALID_RESPONSE: &str = "GitHub returned an invalid response.";
pub(crate) const UNEXPECTED_RESPONSE: &str = "GitHub returned an unexpected response. Check source access, permissions, rate limits and the requested resource.";
const INVALID_REFERENCE: &str = "ref is invalid.";
pub(crate) const FILE_TOO_LARGE: &str = "The file is too large to read.";
const INVALID_ORIGIN: &str = "The configured GitHub API origin is invalid.";
const INVALID_REPOSITORY: &str = "The source owner or repository name is invalid.";
const CLIENT_UNAVAILABLE: &str = "Could not initialize the GitHub client.";
const MISSING_DEFAULT_BRANCH: &str = "GitHub did not return a default branch.";
const MISSING_COMMIT: &str = "GitHub did not return an immutable commit SHA.";
const NUMBER_NOT_POSITIVE: &str = "number must be positive.";
const LOG_UNDIRECTED: &str = "GitHub redirected the job log without a destination.";
const LOG_UNEXPECTED_HOST: &str = "GitHub redirected the job log to an unexpected host.";
const LOG_UNREACHABLE: &str = "The job log download failed or timed out.";
const LOG_INTERRUPTED: &str = "The job log download failed part way through.";
pub(crate) const COMPLETED_STATUS: &str = "completed";
const UNIDENTIFIED_CI: &str = "GitHub returned CI records without their identities.";
const FOREIGN_CHECKS: &str = "GitHub returned checks for a different or missing commit SHA.";
pub(crate) const PARTIAL_WORKFLOW_RUNS: &str = "GitHub's workflow runs could not be read in full (the 1,000-result search limit, or runs shifting between pages), so the build cannot be assessed completely.";
pub(crate) const INVALID_TAG: &str = "tag is invalid.";
const UNUSABLE_RELEASE: &str = "GitHub returned a release without a usable identity.";
const RELEASE_EVENT: &str = "release";
const FOREIGN_JOB: &str =
    "That job does not belong to the pull request or commit being asked about.";
const MAX_RETRY_AFTER: Duration = Duration::from_secs(60);
pub(crate) const UNAUTHORIZED: u16 = 401;
pub(super) const SERVER_ERRORS: std::ops::Range<u16> = 500..600;
const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;
pub(crate) const UNPROCESSABLE_ENTITY: u16 = 422;
const TOO_MANY_REQUESTS: u16 = 429;

/// Compiling the path vocabulary is the expensive part, so the prioritiser
/// behind a blast radius is built once and shared by every assessment.
static PRIORITISER: LazyLock<Prioritiser> =
    LazyLock::new(|| Prioritiser::new(Prioritisation::default()));

pub(crate) struct Github {
    service: PullRequestService,
    repository: Repository,
    token: SecretValue,
    log_bytes: u64,
    signals: ReviewSignals,
    configuration: Configuration,
}

impl Github {
    pub(crate) fn service(&self) -> &PullRequestService {
        &self.service
    }

    pub(crate) fn repository(&self) -> &Repository {
        &self.repository
    }

    pub(crate) fn token(&self) -> &SecretValue {
        &self.token
    }

    pub(crate) fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    pub(crate) fn log_bytes(&self) -> u64 {
        self.log_bytes
    }

    #[cfg(test)]
    pub(crate) fn with_signals(self, signals: ReviewSignals) -> Self {
        Self { signals, ..self }
    }

    #[cfg(test)]
    pub(crate) fn with_log_bytes(self, log_bytes: u64) -> Self {
        Self { log_bytes, ..self }
    }

    /// This source's client against the GitHub origin the server is
    /// configured for.
    pub(crate) fn new(configuration: Configuration, config: &Config) -> Result<Self, String> {
        let service = config.pull_requests().map_err(|error| match error {
            PullRequestError::InvalidOrigin => INVALID_ORIGIN.to_string(),
            _ => CLIENT_UNAVAILABLE.to_string(),
        })?;
        Self::serving(configuration, service)
    }

    pub(super) fn serving(
        mut configuration: Configuration,
        service: PullRequestService,
    ) -> Result<Self, String> {
        if !segment(&configuration.owner) || !segment(&configuration.repository) {
            return Err(INVALID_REPOSITORY.to_string());
        }
        let signals = match &configuration.review_signals {
            Some(names) => ReviewSignals::select(names)?,
            None => ReviewSignals::recognized(),
        };
        let token = credential(configuration.token.take());
        let repository = Repository::new(&configuration.owner, &configuration.repository)
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

    pub(crate) fn reference(&self, number: u64) -> Result<PullRequestReference, String> {
        let number = NonZeroU64::new(number).ok_or(NUMBER_NOT_POSITIVE)?;
        Ok(PullRequestReference::new(self.repository.clone(), number))
    }

    pub(crate) fn issue_reference(&self, number: u64) -> Result<IssueReference, String> {
        let number = NonZeroU64::new(number).ok_or(NUMBER_NOT_POSITIVE)?;
        Ok(IssueReference::new(self.repository.clone(), number))
    }

    /// The `owner/repo` name GitHub reports records under.
    fn full_name(&self) -> String {
        format!(
            "{}/{}",
            self.configuration.owner, self.configuration.repository
        )
    }

    pub(crate) async fn resolve(
        &self,
        reference: Option<&str>,
    ) -> Result<(String, String), String> {
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
    pub(crate) async fn settled(
        &self,
        reference: Option<&str>,
    ) -> Result<(String, String, &'static str), String> {
        let (reference, sha) = self.resolve(reference).await?;
        let (workflows, checks, statuses, complete) = self.ci_rows(&sha).await?;
        let state = assessed(&conclusions(&workflows, &checks, &statuses), complete);
        Ok((reference, sha, state))
    }

    pub(crate) async fn ci_rows(
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
            return Err(PARTIAL_WORKFLOW_RUNS.to_string());
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

    async fn check_evidence(&self, sha: &str) -> Result<CheckEvidence, String> {
        let (workflows, checks, statuses, complete) = self.ci_rows(sha).await?;
        Ok(CheckEvidence {
            complete,
            ..tally(conclusions(&workflows, &checks, &statuses).into_iter())
        })
    }

    /// One pull request's readiness. `detail` is the pull request as read by
    /// number, when it already was; a listed row is read again by number, and
    /// a head the two reads disagree on is never verified.
    pub(crate) async fn assess(
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
                    mergeable: mergeability(detail.mergeable_state),
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

    pub(crate) async fn pull(
        &self,
        reference: &PullRequestReference,
    ) -> Result<PullRequestDetail, String> {
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

    pub(crate) async fn threads(
        &self,
        number: u64,
    ) -> Result<Collected<ReviewThreadRecord>, String> {
        self.service
            .collect_review_threads(&self.reference(number)?, &self.token)
            .await
            .map_err(thread_described)
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
    /// bots post their summaries, so a conversation longer than a readiness
    /// verdict weighs reads as incomplete rather than as all of it.
    ///
    /// GitHub lists them oldest first, so the second page says whether the
    /// first held all of them, and past two full pages the conversation's
    /// own count says which pages hold the newest.
    async fn review_comments(&self, reference: &IssueReference) -> Result<CommentEvidence, String> {
        let first = self.comment_page(reference, NonZeroU32::MIN).await?;
        let Some(second) = first.next else {
            return Ok(comment_evidence(first.items, true));
        };
        let rest = self.comment_page(reference, second).await?;
        if rest.items.is_empty() {
            return Ok(comment_evidence(first.items, true));
        }
        let comments = match rest.next {
            None => {
                let mut comments = first.items;
                comments.extend(rest.items);
                comments
            }
            Some(_) => self.newest_comments(reference).await?,
        };
        Ok(comment_evidence(comments, false))
    }

    /// The last pages of a conversation, found from how many comments GitHub
    /// counts on it.
    async fn newest_comments(
        &self,
        reference: &IssueReference,
    ) -> Result<Vec<IssueComment>, String> {
        let counted = self
            .service
            .fetch_issue(reference, &self.token)
            .await
            .map_err(described)?
            .comments;
        let pages = u32::try_from(counted.div_ceil(u64::from(PER_PAGE.get())))
            .map_err(|_| INVALID_RESPONSE.to_string())?
            .max(NEWEST_COMMENT_PAGES);
        let mut comments = Vec::new();
        for page in (pages - NEWEST_COMMENT_PAGES + 1..=pages).filter_map(NonZeroU32::new) {
            comments.extend(self.comment_page(reference, page).await?.items);
        }
        Ok(comments)
    }

    async fn comment_page(
        &self,
        reference: &IssueReference,
        page: NonZeroU32,
    ) -> Result<Page<IssueComment>, String> {
        self.service
            .list_comments(reference, &self.token, page, PER_PAGE)
            .await
            .map_err(described)
    }

    pub(crate) fn release_identity(&self, release: &Release) -> Result<ReleaseIdentity, String> {
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
    pub(crate) async fn pipeline_sweep(
        &self,
        releases: &[ReleaseIdentity],
    ) -> Option<Vec<RunEvidence>> {
        let earliest = releases
            .iter()
            .min_by_key(|release| release.published())
            .map(|release| release.published_at.as_str())?;
        self.pipeline_runs(WorkflowRunFilter::new().with_created(format!(">={earliest}")))
            .await
            .ok()
    }

    /// The runs GitHub itself attributes to one release's tag.
    pub(crate) async fn exact_pipeline(
        &self,
        release: &ReleaseIdentity,
        checked_at: &str,
    ) -> ReleasePipeline {
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
            return Err(PARTIAL_WORKFLOW_RUNS.to_string());
        }
        Ok(runs.items.iter().map(run_evidence).collect())
    }

    pub(crate) fn verify_job(
        &self,
        job: &WorkflowJob,
        job_id: u64,
        sha: &str,
    ) -> Result<u64, String> {
        let run_id = job.run_id.get();
        if job.id.get() != job_id
            || !job.head_sha.as_str().eq_ignore_ascii_case(sha)
            || !is_job_url(
                job.url.as_deref().unwrap_or_default(),
                &self.configuration.owner,
                &self.configuration.repository,
                run_id,
                job_id,
            )
        {
            return Err(FOREIGN_JOB.to_string());
        }
        Ok(run_id)
    }
}

fn is_job_url(value: &str, owner: &str, repository: &str, run_id: u64, job_id: u64) -> bool {
    value.eq_ignore_ascii_case(&format!(
        "https://github.com/{owner}/{repository}/actions/runs/{run_id}/job/{job_id}"
    ))
}

pub(crate) fn segment(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

pub(crate) fn listed(
    reference: PullRequestReference,
    detail: &PullRequestDetail,
) -> ListedPullRequest {
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

/// What GitHub's `mergeable_state` means to a readiness verdict. A state the
/// crate does not recognise arrives as `Unknown` and stays unknown here: a
/// word nothing has read is not evidence that the branch merges.
fn mergeability(state: MergeableState) -> Mergeability {
    match state {
        MergeableState::Clean | MergeableState::HasHooks => Mergeability::Clean,
        MergeableState::Blocked => Mergeability::Blocked,
        MergeableState::Behind => Mergeability::Behind,
        MergeableState::Dirty => Mergeability::Conflicted,
        MergeableState::Unstable => Mergeability::Unstable,
        MergeableState::Draft => Mergeability::Draft,
        _ => Mergeability::Unknown,
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

/// The newest of `comments`, as many as a readiness verdict weighs.
fn comment_evidence(comments: Vec<IssueComment>, complete: bool) -> CommentEvidence {
    let excess = comments.len().saturating_sub(REVIEW_PAGE_SIZE);
    CommentEvidence {
        complete,
        comments: comments
            .into_iter()
            .skip(excess)
            .map(issue_comment)
            .collect(),
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

fn credential(token: Option<SecretValue>) -> SecretValue {
    token.unwrap_or_else(|| SecretValue::new(""))
}

pub(crate) fn revision(sha: &str) -> Result<Revision, String> {
    Revision::parse(sha).map_err(|_| MISSING_COMMIT.to_string())
}

/// What a crate error says in Zone's words. GitHub's own words in
/// `GitHubApi` and `Refused` can quote the request, so they are never
/// repeated.
pub(crate) fn described(error: PullRequestError) -> String {
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
            JobLogError::Refused(status) => log_status_error(status),
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

pub(crate) fn status_error(status: u16) -> String {
    format!(
        "GitHub returned HTTP {status}. Check source access, permissions, rate limits and the requested resource."
    )
}

fn log_status_error(status: u16) -> String {
    format!("The job log download returned HTTP {status}.")
}

fn thread_described(error: PullRequestError) -> String {
    match error {
        PullRequestError::PullRequestNotFound => MISSING_PULL.to_string(),
        PullRequestError::GitHubApi(message) if message == ANSWERED_ANOTHER_PULL => {
            MISMATCHED_PULL.to_string()
        }
        PullRequestError::GitHubApi(_) | PullRequestError::RepositoryNotFound => {
            REVIEW_QUERY_REJECTED.to_string()
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

pub(crate) fn conclusions<'a>(
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
pub(crate) fn assessed(conclusions: &[&str], complete: bool) -> &'static str {
    match assessment(conclusions) {
        FAILURE_ASSESSMENT => FAILURE_ASSESSMENT,
        _ if !complete => PENDING_ASSESSMENT,
        state => state,
    }
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

/// What a commit's checks add up to, in one word a wait can act on.
///
/// `unknown` is silence — nothing registered, or a token this does not know —
/// and a wait tolerates it for a grace period before giving up on it. A
/// completed run that concluded neutral or skipped is not silence: it reported,
/// and it is over. Folding those into `unknown` had a wait sit out the whole
/// grace period and then say no checks were configured or reporting, about
/// checks that had reported. They settle under their own name instead, which
/// reads as the non-pass they are.
pub(crate) fn assessment(conclusions: &[&str]) -> &'static str {
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

pub(crate) fn ci_token<'a>(status: Option<&'a str>, conclusion: Option<&'a str>) -> &'a str {
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

#[cfg(test)]
mod tests;
