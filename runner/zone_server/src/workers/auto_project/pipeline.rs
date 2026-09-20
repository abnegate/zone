//! One task's change on its way from a run to a merge.
//!
//! Every step reads the pull request as it is now and moves the task's row by
//! one stage at most; the next tick takes the next step. A step that cannot
//! decide leaves the stage alone with a reason, and a step that needs a person
//! pauses the task with one.

use chrono::Utc;
use zone_vcs::conflict::{BranchName, ConflictError, ConflictRequest};
use zone_vcs::pull_request::{
    ChecksOutcome, MergeMethod, MergedPr, PrError, PullRequestDetail, PullRequestReference,
    ReviewEvent,
};

use crate::db::auto_projects::{
    self, Finding, Kind, ReviewInsert, ReviewRow, ReviewerKind, Stage, TaskAutomation,
    Verdict as Recorded,
};
use crate::db::tasks::{self, TaskRow};
use crate::services::stages;
use crate::workers::conflict::RepairOutcome;
use crate::workers::pr::{access_token, repair_conflicts_for_task, sync_reception};

use super::driver::Drive;
use super::notification::{self, MergeReport};
use super::review::{self, Outcome, ReviewError, ReviewRequest, bots, model, verdict};
use super::summary;

/// Bytes of diff a reviewer is shown.
const DIFF_BYTES: usize = 60_000;
/// Paths the merge notice names.
const TOP_PATHS: usize = 8;

pub const CI_TASK_TITLE: &str = "Add continuous integration";
pub const CI_TASK_DESCRIPTION: &str = "The repository has no checks: nothing runs on a pull request, so nothing can prove a change \
     before it merges. Add a GitHub Actions workflow under .github/workflows that runs on every \
     pull request and on pushes to the default branch, and runs the project's test suite, its \
     linter and its type checks (whichever apply to the stack). Add the test scaffolding the \
     suite needs if there is none yet, with at least one real test that passes. Cache \
     dependencies. The workflow must run green on its own pull request.";
pub const CI_TASK_CRITERIA: &str = "A pull_request workflow exists and passes on the pull request that adds it; tests, lint and \
     type checks run in it; a deliberately broken test would fail it.";

const NO_BOT_REVIEW_PREFIX: &str = "no review from: ";
const REFRESHED_PREFIX: &str = "refreshed from base";

/// Everything one step needs, read once.
struct Step<'a> {
    drive: &'a Drive<'a>,
    task: &'a TaskAutomation,
    row: TaskRow,
    reference: PullRequestReference,
    token: String,
    pr_url: String,
}

impl Step<'_> {
    /// Move the task to a stage, with the reason it is there.
    async fn set(&self, stage: Stage, reason: Option<&str>) -> Result<(), String> {
        auto_projects::set_stage(
            self.drive.state.db(),
            self.task.task_id,
            self.drive.project.project_id,
            stage,
            reason,
        )
        .await
        .map_err(|error| error.to_string())
    }

    /// Stop this task and tell people, without stopping the project.
    async fn pause(&self, reason: &str) -> Result<(), String> {
        tracing::warn!(task_id = %self.task.task_id, reason, "Auto project task paused");
        self.set(Stage::Paused, Some(reason)).await?;
        self.drive
            .notify(
                "paused",
                notification::paused(
                    &self.drive.project.name,
                    &format!("task \"{}\": {reason}", self.row.title),
                    Some(&self.pr_url),
                ),
            )
            .await;
        Ok(())
    }

    /// How long checks have been absent or pending on this head.
    fn seconds_since_checks(&self) -> i64 {
        self.task
            .checks_since
            .map(|since| (Utc::now() - since).num_seconds())
            .unwrap_or(0)
    }

    /// The pull request as GitHub sees it now.
    async fn pull(&self) -> Result<PullRequestDetail, String> {
        self.drive
            .services
            .pr
            .fetch_pull(&self.reference, &self.token)
            .await
            .map_err(|error| error.to_string())
    }

    /// Whether the pull request's head is no longer the one recorded.
    fn head_moved(&self, pull: &PullRequestDetail) -> bool {
        self.task.head.as_deref() != Some(pull.head_sha.as_str())
    }
}

/// Take the task's next step.
pub async fn advance(drive: &Drive<'_>, task: &TaskAutomation) -> Result<(), String> {
    let pool = drive.state.db();
    let Some(row) = tasks::get_task(pool, task.task_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let Some(pr_url) = row.pr_url.clone() else {
        auto_projects::set_stage(
            pool,
            task.task_id,
            drive.project.project_id,
            Stage::Paused,
            Some("the task has no pull request to drive"),
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(());
    };
    let reference = match drive.services.pr.pull_request(&pr_url) {
        Ok(reference) => reference,
        Err(error) => {
            auto_projects::set_stage(
                pool,
                task.task_id,
                drive.project.project_id,
                Stage::Paused,
                Some(&format!("the pull request URL cannot be read: {error}")),
            )
            .await
            .map_err(|error| error.to_string())?;
            return Ok(());
        }
    };
    let Some(token) = access_token(drive.state, &row).await else {
        auto_projects::set_stage(
            pool,
            task.task_id,
            drive.project.project_id,
            Stage::Paused,
            Some("repository_token_missing: link the repository with a token on the project"),
        )
        .await
        .map_err(|error| error.to_string())?;
        return Ok(());
    };
    let step = Step {
        drive,
        task,
        row,
        reference,
        token,
        pr_url,
    };
    match task.stage() {
        Stage::AwaitingChecks => awaiting_checks(&step).await,
        Stage::AwaitingReviews => awaiting_reviews(&step).await,
        Stage::Fixing => fixing(&step).await,
        Stage::Merging => merging(&step).await,
        Stage::PostMerge => post_merge(&step).await,
        _ => Ok(()),
    }
}

/// Merged or closed somewhere else: nothing here to drive any more.
async fn settled_elsewhere(step: &Step<'_>, pull: &PullRequestDetail) -> Result<bool, String> {
    let pool = step.drive.state.db();
    if pull.merged {
        tasks::complete_merged_task(pool, step.task.task_id, &step.pr_url)
            .await
            .map_err(|error| error.to_string())?;
        if let Some(sha) = &pull.merge_commit_sha {
            auto_projects::set_merge_sha(pool, step.task.task_id, sha)
                .await
                .map_err(|error| error.to_string())?;
        }
        step.set(Stage::Merged, Some("merged outside the pipeline"))
            .await?;
        return Ok(true);
    }
    if pull.state == "closed" {
        tasks::mark_pr_status(pool, step.task.task_id, "closed")
            .await
            .map_err(|error| error.to_string())?;
        step.pause("the pull request was closed on GitHub without merging")
            .await?;
        return Ok(true);
    }
    Ok(false)
}

/// Wait for checks on the head, repairing conflicts and holding for the CI task when there are none.
async fn awaiting_checks(step: &Step<'_>) -> Result<(), String> {
    let pool = step.drive.state.db();
    let config = step.drive.config;
    let pull = step.pull().await?;
    if settled_elsewhere(step, &pull).await? {
        return Ok(());
    }
    let head = pull.head_sha.clone();
    let fresh = step.head_moved(&pull);
    if pull.mergeable.conflicted() {
        match repair_conflicts_for_task(step.drive.state, step.task.task_id).await {
            RepairOutcome::Repaired { commit, .. } => {
                tracing::info!(task_id = %step.task.task_id, %commit, "Repaired a conflict; re-reading the head");
                auto_projects::set_head(pool, step.task.task_id, &head, Some("conflicted"), true)
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
            RepairOutcome::NotConflicted => {}
            other => {
                return step
                    .pause(&format!(
                        "the branch conflicts with its base and the repair did not hold: {other:?}"
                    ))
                    .await;
            }
        }
    }
    let checks = step
        .drive
        .services
        .pr
        .fetch_checks(
            &step.reference.owner,
            &step.reference.repository,
            &head,
            &step.token,
        )
        .await
        .map_err(|error| error.to_string())?;
    auto_projects::set_head(pool, step.task.task_id, &head, Some(checks.label()), fresh)
        .await
        .map_err(|error| error.to_string())?;
    let elapsed = if fresh {
        0
    } else {
        step.seconds_since_checks()
    };
    match checks {
        ChecksOutcome::Success => step.set(Stage::AwaitingReviews, None).await,
        ChecksOutcome::Pending => {
            if elapsed >= i64::try_from(config.checks_timeout_secs).unwrap_or(i64::MAX) {
                step.pause(&format!(
                    "checks on {} were still pending after {} seconds",
                    short(&head),
                    config.checks_timeout_secs
                ))
                .await
            } else {
                step.set(Stage::AwaitingChecks, Some("waiting for checks"))
                    .await
            }
        }
        ChecksOutcome::Failure(names) => {
            step.set(
                Stage::Fixing,
                Some(&format!(
                    "checks failed on {}: {}. Read the failing job, fix the cause on this branch, \
                     and make the checks pass.",
                    short(&head),
                    names.join(", ")
                )),
            )
            .await
        }
        ChecksOutcome::Absent => {
            if fresh || elapsed < i64::try_from(config.checks_grace_secs).unwrap_or(i64::MAX) {
                return step
                    .set(Stage::AwaitingChecks, Some("waiting for checks to report"))
                    .await;
            }
            absent_checks(step, &pull).await
        }
    }
}

/// Nothing reports on this head. Checks are never skipped: the project gets
/// its continuous integration first, and the branch is refreshed to run it.
async fn absent_checks(step: &Step<'_>, pull: &PullRequestDetail) -> Result<(), String> {
    let pool = step.drive.state.db();
    let project = step.drive.project;
    if step.task.kind() == Some(Kind::Ci) {
        return step
            .set(
                Stage::Fixing,
                Some(
                    "no check ran on this head: the workflow this task adds did not run. Make sure a \
                     workflow file under .github/workflows runs on pull_request, is valid YAML, and \
                     names jobs GitHub will start.",
                ),
            )
            .await;
    }
    match auto_projects::ci_task(pool, project.project_id)
        .await
        .map_err(|error| error.to_string())?
    {
        None => {
            if auto_projects::count_auto_created(pool, project.project_id, Kind::Ci)
                .await
                .map_err(|error| error.to_string())?
                == 0
            {
                step.drive
                    .add_task(
                        Kind::Ci,
                        CI_TASK_TITLE,
                        CI_TASK_DESCRIPTION,
                        Some(CI_TASK_CRITERIA),
                        "The repository reports no checks on pull requests, so a task that adds \
                         continuous integration was put ahead of everything still waiting to start.",
                    )
                    .await?;
            }
            step.set(
                Stage::AwaitingChecks,
                Some("waiting for continuous integration to be added to the repository"),
            )
            .await
        }
        Some((_, status)) if status != "complete" => {
            step.set(
                Stage::AwaitingChecks,
                Some("waiting for the continuous-integration task to merge"),
            )
            .await
        }
        Some(_) => {
            if step
                .task
                .reason
                .as_deref()
                .is_some_and(|reason| reason.starts_with(REFRESHED_PREFIX))
            {
                return step
                    .set(
                        Stage::Fixing,
                        Some(
                            "no check ran on this head even after the base's workflows were merged \
                             in: find out why the workflow does not start for this branch and fix it.",
                        ),
                    )
                    .await;
            }
            refresh_from_base(step, pull).await
        }
    }
}

/// Merge the base into the branch so the workflows it now carries run.
async fn refresh_from_base(step: &Step<'_>, pull: &PullRequestDetail) -> Result<(), String> {
    let pool = step.drive.state.db();
    let Some(remote) = step.drive.project.repository_url.clone() else {
        return step
            .pause("the project has no repository URL to refresh from")
            .await;
    };
    let head = BranchName::parse(&pull.head_ref).map_err(|error| error.to_string())?;
    let base = BranchName::parse(&pull.base_ref).map_err(|error| error.to_string())?;
    let request = ConflictRequest {
        remote,
        token: Some(step.token.clone()),
        head: head.clone(),
        base,
        expected_head: None,
        expected_base: None,
    };
    let message = format!(
        "Merge {} into {} to pick up its workflows",
        pull.base_ref, pull.head_ref
    );
    match step
        .drive
        .services
        .conflicts
        .refresh(&request, &head, &message)
        .await
    {
        Ok(refreshed) if refreshed.pushed => {
            auto_projects::set_head(
                pool,
                step.task.task_id,
                refreshed.commit.as_str(),
                Some("absent"),
                true,
            )
            .await
            .map_err(|error| error.to_string())?;
            step.set(
                Stage::AwaitingChecks,
                Some(&format!(
                    "{REFRESHED_PREFIX} as {} to pick up its workflows",
                    short(refreshed.commit.as_str())
                )),
            )
            .await
        }
        Ok(_) => {
            step.set(
                Stage::Fixing,
                Some(
                    "no check ran on this head although the base's workflows are already in the \
                     branch: find out why the workflow does not start for this branch and fix it.",
                ),
            )
            .await
        }
        Err(ConflictError::Conflicted) => {
            match repair_conflicts_for_task(step.drive.state, step.task.task_id).await {
                RepairOutcome::Repaired { .. } | RepairOutcome::NotConflicted => {
                    step.set(
                        Stage::AwaitingChecks,
                        Some(&format!("{REFRESHED_PREFIX} through a conflict repair")),
                    )
                    .await
                }
                other => {
                    step.pause(&format!(
                        "the base could not be merged into the branch: {other:?}"
                    ))
                    .await
                }
            }
        }
        Err(error) => {
            step.set(
                Stage::AwaitingChecks,
                Some(&format!(
                    "could not refresh the branch from its base: {error}"
                )),
            )
            .await
        }
    }
}

/// Gather bot and Zone reviews of the head and decide what happens next.
async fn awaiting_reviews(step: &Step<'_>) -> Result<(), String> {
    let pool = step.drive.state.db();
    let config = step.drive.config;
    let pr = &step.drive.services.pr;
    let pull = step.pull().await?;
    if settled_elsewhere(step, &pull).await? {
        return Ok(());
    }
    if step.head_moved(&pull) {
        return step
            .set(
                Stage::AwaitingChecks,
                Some("the head moved; checking it again"),
            )
            .await;
    }
    let head = pull.head_sha.clone();
    let mut rows = auto_projects::reviews(pool, step.task.task_id)
        .await
        .map_err(|error| error.to_string())?;

    // Bots first: what the repository's own reviewers said about this head.
    let comments = pr
        .fetch_issue_comments(&step.reference, &step.token)
        .await
        .map_err(|error| error.to_string())?;
    let threads = pr
        .fetch_review_threads(&step.reference, &step.token)
        .await
        .map_err(|error| error.to_string())?;
    let mut waiting: Vec<_> = Vec::new();
    for kind in bots::expected(config, &rows, &comments, &threads) {
        if rows
            .iter()
            .any(|row| row.is_bot() && row.reviewer == kind.reviewer() && row.head == head)
        {
            continue;
        }
        match bots::round(kind, &comments, &threads, &head) {
            Some(round) => {
                let next = auto_projects::latest_round(pool, step.task.task_id)
                    .await
                    .map_err(|error| error.to_string())?
                    + 1;
                auto_projects::record_review(
                    pool,
                    ReviewInsert {
                        task_id: step.task.task_id,
                        run_id: step.task.last_run_id,
                        round: next,
                        head: &head,
                        reviewer_kind: ReviewerKind::Bot,
                        reviewer: round.reviewer,
                        author_model: None,
                        same_model: false,
                        verdict: round.verdict,
                        summary: &round.summary,
                        findings: &round.findings,
                        addressed: &[],
                        external_id: round.external_id.as_deref(),
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            }
            None => waiting.push(kind),
        }
    }
    let mut absent: Vec<String> = Vec::new();
    if !waiting.is_empty() {
        let elapsed = step.seconds_since_checks();
        let grace = i64::try_from(config.bot_review_grace_secs).unwrap_or(i64::MAX);
        let names: Vec<String> = waiting
            .iter()
            .map(|kind| bots::display_name(kind.reviewer()))
            .collect();
        if elapsed < grace {
            return step
                .set(
                    Stage::AwaitingReviews,
                    Some(&format!(
                        "waiting for {} to review {}",
                        names.join(", "),
                        short(&head)
                    )),
                )
                .await;
        }
        if step.task.bot_trigger_head.as_deref() != Some(head.as_str()) {
            for kind in &waiting {
                if let Err(error) = pr
                    .post_issue_comment(&step.reference, kind.trigger_command(), &step.token)
                    .await
                {
                    tracing::warn!(task_id = %step.task.task_id, %error, "Could not ask a review bot to review");
                }
            }
            auto_projects::set_bot_trigger_head(pool, step.task.task_id, &head)
                .await
                .map_err(|error| error.to_string())?;
            return step
                .set(
                    Stage::AwaitingReviews,
                    Some(&format!(
                        "asked {} to review {}",
                        names.join(", "),
                        short(&head)
                    )),
                )
                .await;
        }
        if elapsed < grace.saturating_mul(2) {
            return step
                .set(
                    Stage::AwaitingReviews,
                    Some(&format!(
                        "still waiting for {} after asking",
                        names.join(", ")
                    )),
                )
                .await;
        }
        absent = names;
    }

    // Then Zone's own reviewer, once per head.
    rows = auto_projects::reviews(pool, step.task.task_id)
        .await
        .map_err(|error| error.to_string())?;
    let bot_on_head = rows.iter().any(|row| row.is_bot() && row.head == head);
    if !rows.iter().any(|row| !row.is_bot() && row.head == head) {
        let author = match step.task.last_run_id {
            Some(run) => tasks::run_mode(pool, run)
                .await
                .ok()
                .flatten()
                .and_then(|mode| mode.model),
            None => None,
        };
        let (prefs, catalog) = model::preferences(step.drive.state, step.drive.workspace_id).await;
        let round = auto_projects::latest_round(pool, step.task.task_id)
            .await
            .map_err(|error| error.to_string())?
            + 1;
        let reviewer = model::select(
            author.as_deref(),
            &prefs,
            &catalog,
            &config.review_models,
            u32::try_from(round).unwrap_or(1),
        );
        if stages::is_auto(&reviewer.model) {
            return step
                .pause("no completion model is installed to review with")
                .await;
        }
        if reviewer.same_model && config.require_distinct_reviewer && !bot_on_head {
            return step
                .pause(
                    "no model other than the one that wrote the change is available to review it, \
                     and no review bot answered",
                )
                .await;
        }
        let diff = pr
            .fetch_diff(&step.reference, &step.token, DIFF_BYTES)
            .await
            .map_err(|error| error.to_string())?;
        let files = pr
            .fetch_files(&step.reference, &step.token)
            .await
            .unwrap_or_default();
        let open = auto_projects::open_findings_of(&rows);
        let outcome = review::run(
            &step.drive.state.config().litellm_host,
            &step.drive.state.config().litellm_key,
            pr.clone(),
            ReviewRequest {
                task: &step.row,
                brief: step.drive.project.brief.as_ref(),
                pull: &pull,
                diff,
                files,
                open: &open,
                prior: &rows,
                round,
                reviewer: reviewer.model.clone(),
                same_model: reviewer.same_model,
                token: step.token.clone(),
                reference: step.reference.clone(),
            },
        )
        .await;
        match outcome {
            Ok(verdict) => {
                let body = verdict::comment(&verdict, &reviewer.model, round, reviewer.same_model);
                let external = match pr
                    .submit_review(
                        &step.reference,
                        &step.token,
                        &head,
                        ReviewEvent::Comment,
                        &body,
                    )
                    .await
                {
                    Ok(id) => Some(id.to_string()),
                    Err(error) => {
                        tracing::warn!(task_id = %step.task.task_id, %error, "Could not post the review on GitHub");
                        None
                    }
                };
                auto_projects::record_review(
                    pool,
                    ReviewInsert {
                        task_id: step.task.task_id,
                        run_id: step.task.last_run_id,
                        round,
                        head: &head,
                        reviewer_kind: ReviewerKind::Model,
                        reviewer: &reviewer.model,
                        author_model: author.as_deref(),
                        same_model: reviewer.same_model,
                        verdict: match verdict.outcome {
                            Outcome::Approve => Recorded::Approve,
                            Outcome::RequestChanges => Recorded::RequestChanges,
                        },
                        summary: &verdict.summary,
                        findings: &verdict.findings,
                        addressed: &verdict.addressed,
                        external_id: external.as_deref(),
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
                answer_bot_threads(step, &open, &verdict.addressed, &threads, &head).await;
            }
            Err(ReviewError::Unparseable(message)) => {
                auto_projects::record_review(
                    pool,
                    ReviewInsert {
                        task_id: step.task.task_id,
                        run_id: step.task.last_run_id,
                        round,
                        head: &head,
                        reviewer_kind: ReviewerKind::Model,
                        reviewer: &reviewer.model,
                        author_model: author.as_deref(),
                        same_model: reviewer.same_model,
                        verdict: Recorded::Unparseable,
                        summary: &message,
                        findings: &[],
                        addressed: &[],
                        external_id: None,
                    },
                )
                .await
                .map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
        rows = auto_projects::reviews(pool, step.task.task_id)
            .await
            .map_err(|error| error.to_string())?;
    }

    // Decide.
    let open = auto_projects::open_findings_of(&rows);
    let distinct = !config.require_distinct_reviewer
        || auto_projects::distinct_review_on_head(pool, step.task.task_id, &head)
            .await
            .map_err(|error| error.to_string())?;
    match decide(&rows, &open, &head, distinct) {
        Decision::Merge => {
            let note = if absent.is_empty() {
                None
            } else {
                Some(format!("{NO_BOT_REVIEW_PREFIX}{}", absent.join(", ")))
            };
            step.set(Stage::Merging, note.as_deref()).await
        }
        Decision::Fix(reason) => {
            let rounds = auto_projects::bump_review_round(pool, step.task.task_id)
                .await
                .map_err(|error| error.to_string())?;
            if rounds >= i32::try_from(config.max_review_rounds).unwrap_or(i32::MAX) {
                let last = open
                    .first()
                    .map(|finding| format!("{}: {}", finding.id, finding.title))
                    .unwrap_or_else(|| "the reviewer kept requesting changes".to_string());
                return step
                    .pause(&format!(
                        "review did not converge after {rounds} rounds; last open finding: {last}"
                    ))
                    .await;
            }
            step.set(Stage::Fixing, Some(&reason)).await
        }
    }
}

/// Reply on, and resolve, every bot thread the reviewer found addressed.
async fn answer_bot_threads(
    step: &Step<'_>,
    open: &[Finding],
    addressed: &[String],
    threads: &[zone_vcs::pull_request::ReviewThreadRecord],
    head: &str,
) {
    let pr = &step.drive.services.pr;
    for finding in open {
        let Some(thread_id) = &finding.thread_id else {
            continue;
        };
        if !addressed.contains(thread_id) && !addressed.contains(&finding.id) {
            continue;
        }
        if let Some(comment) = threads
            .iter()
            .find(|thread| &thread.id == thread_id)
            .and_then(|thread| thread.comments.first())
            .and_then(|comment| comment.database_id)
            && let Err(error) = pr
                .reply_to_review_comment(
                    &step.reference,
                    comment,
                    &format!(
                        "Addressed in {} (verified by Zone's reviewer).",
                        short(head)
                    ),
                    &step.token,
                )
                .await
        {
            tracing::debug!(task_id = %step.task.task_id, %error, "Could not reply on a review thread");
        }
        if let Err(error) = pr.resolve_review_thread(thread_id, &step.token).await {
            tracing::debug!(task_id = %step.task.task_id, %error, "Could not resolve a review thread");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Merge,
    Fix(String),
}

/// Merge only when the latest Zone review of this head approves and raises
/// nothing, every bot that reviewed this head is at its bar, nothing from any
/// earlier round is still open, and something other than the author reviewed.
pub fn decide(rows: &[ReviewRow], open: &[Finding], head: &str, distinct: bool) -> Decision {
    let latest = rows
        .iter()
        .filter(|row| !row.is_bot() && row.head == head)
        .max_by_key(|row| (row.round, row.created_at));
    let Some(latest) = latest else {
        return Decision::Fix("no review of this head has been recorded".to_string());
    };
    if latest.verdict() != Recorded::Approve {
        return Decision::Fix(format!(
            "round {} requested changes: {}",
            latest.round,
            describe(open)
        ));
    }
    let bots_short: Vec<&ReviewRow> = rows
        .iter()
        .filter(|row| row.is_bot() && row.head == head && row.verdict() != Recorded::Approve)
        .collect();
    if !bots_short.is_empty() {
        return Decision::Fix(format!(
            "{} still has open threads or scored the change below its bar: {}",
            bots_short
                .iter()
                .map(|row| bots::display_name(&row.reviewer))
                .collect::<Vec<_>>()
                .join(", "),
            describe(open)
        ));
    }
    if !open.is_empty() {
        return Decision::Fix(format!(
            "findings from earlier rounds are still open: {}",
            describe(open)
        ));
    }
    if !distinct {
        return Decision::Fix(
            "no review by a model other than the author's has been recorded".to_string(),
        );
    }
    Decision::Merge
}

/// The open findings as one line, for a reason.
fn describe(open: &[Finding]) -> String {
    if open.is_empty() {
        return "no open findings".to_string();
    }
    let listed: Vec<String> = open
        .iter()
        .take(6)
        .map(|finding| format!("{} ({})", finding.id, finding.title))
        .collect();
    let mut text = listed.join("; ");
    if open.len() > 6 {
        text.push_str(&format!(" and {} more", open.len() - 6));
    }
    text
}

/// Admit a fix-up run of the task on its own branch.
async fn fixing(step: &Step<'_>) -> Result<(), String> {
    let config = step.drive.config;
    if step.task.runs >= i32::try_from(config.max_runs_per_task).unwrap_or(i32::MAX) {
        return step
            .pause(&format!(
                "the task used its {} runs and still has: {}",
                step.task.runs,
                step.task.reason.as_deref().unwrap_or("open findings")
            ))
            .await;
    }
    let reason = step.task.reason.clone();
    if !step
        .drive
        .admit(step.task.task_id, reason.as_deref())
        .await?
    {
        step.set(Stage::Running, reason.as_deref()).await?;
    }
    Ok(())
}

/// Squash-merge the pull request, through the administrator path when protection refuses.
async fn merging(step: &Step<'_>) -> Result<(), String> {
    let config = step.drive.config;
    let pr = &step.drive.services.pr;
    let pull = step.pull().await?;
    if settled_elsewhere(step, &pull).await? {
        return Ok(());
    }
    if step.head_moved(&pull) {
        return step
            .set(
                Stage::AwaitingChecks,
                Some("the head moved before the merge"),
            )
            .await;
    }
    let rows = auto_projects::reviews(step.drive.state.db(), step.task.task_id)
        .await
        .map_err(|error| error.to_string())?;
    let review_summary = rows
        .iter()
        .rfind(|row| !row.is_bot())
        .map(|row| row.summary.clone())
        .unwrap_or_default();
    let high_level = summary::high_level(step.drive.state, &step.row, &pull, &review_summary).await;
    let message = format!(
        "{high_level}\n\nTask: {}\nReviewed by: {}",
        step.row.title,
        reviewer_names(&rows).join(", ")
    );
    match pr
        .merge(
            &step.reference,
            &step.token,
            Some(&pull.node_id),
            &pull.head_sha,
            &pull.title,
            &message,
            MergeMethod::Squash,
            config.admin_merge,
        )
        .await
    {
        Ok(merged) => finish_merged(step, &pull, merged, &rows, high_level).await,
        Err(PrError::HeadMoved) => {
            step.set(Stage::AwaitingChecks, Some("the head moved during the merge"))
                .await
        }
        Err(PrError::Protected(reason)) => {
            step.pause(&format!(
                "branch protection refused the merge and the project token cannot bypass it: {reason}. \
                 Grant the token administrator access, relax the rule, or merge by hand."
            ))
            .await
        }
        Err(PrError::NotMergeable(reason)) => {
            if pull.mergeable.conflicted() {
                return step
                    .set(Stage::AwaitingChecks, Some("the branch conflicts; repairing"))
                    .await;
            }
            step.pause(&format!("GitHub refused the merge: {reason}"))
                .await
        }
        // A revoked or under-scoped token fails the same way every tick; a
        // person has to relink the repository, so stop asking.
        Err(PrError::AuthFailed) => {
            step.pause(
                "GitHub no longer accepts the project token for this pull request; relink the \
                 repository with a token that can merge it",
            )
            .await
        }
        Err(error) => Err(error.to_string()),
    }
}

/// Who reviewed the pull request, for the notice.
fn reviewer_names(rows: &[ReviewRow]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for row in rows {
        let name = if row.is_bot() {
            bots::display_name(&row.reviewer)
        } else {
            format!("{} (Zone)", row.reviewer)
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// Everything a merge leaves to do: the branch, the task, reception, the notice.
async fn finish_merged(
    step: &Step<'_>,
    pull: &PullRequestDetail,
    merged: MergedPr,
    rows: &[ReviewRow],
    high_level: String,
) -> Result<(), String> {
    let pool = step.drive.state.db();
    let config = step.drive.config;
    let pr = &step.drive.services.pr;
    if config.delete_branch
        && let Err(error) = pr
            .delete_branch(
                &step.reference.owner,
                &step.reference.repository,
                &pull.head_ref,
                &step.token,
            )
            .await
    {
        tracing::warn!(task_id = %step.task.task_id, %error, "Could not delete the merged branch");
    }
    tasks::complete_merged_task(pool, step.task.task_id, &step.pr_url)
        .await
        .map_err(|error| error.to_string())?;
    auto_projects::set_merge_sha(pool, step.task.task_id, &merged.sha)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(run) = step.task.last_run_id {
        let state = step.drive.state.clone();
        let task_id = step.task.task_id;
        tokio::spawn(async move {
            let _ = sync_reception(&state, run, task_id).await;
        });
    }
    let files = pr
        .fetch_files(&step.reference, &step.token)
        .await
        .unwrap_or_default();
    let mut ranked = files;
    ranked.sort_by(|left, right| {
        (right.additions + right.deletions).cmp(&(left.additions + left.deletions))
    });
    let raised: usize = rows.iter().map(|row| row.findings().len()).sum();
    let open = auto_projects::open_findings_of(rows).len();
    let rounds = rows.iter().map(|row| row.round).max().unwrap_or(0);
    let same_model = rows.iter().any(|row| !row.is_bot() && row.same_model)
        && !rows.iter().any(ReviewRow::is_bot);
    let bots_absent: Vec<String> = step
        .task
        .reason
        .as_deref()
        .and_then(|reason| reason.strip_prefix(NO_BOT_REVIEW_PREFIX))
        .map(|names| names.split(", ").map(str::to_string).collect())
        .unwrap_or_default();
    let report = MergeReport {
        project: step.drive.project.name.clone(),
        task_title: step.row.title.clone(),
        pr_title: pull.title.clone(),
        pr_url: step.pr_url.clone(),
        high_level,
        files_changed: pull.changed_files,
        additions: pull.additions,
        deletions: pull.deletions,
        commits: pull.commits,
        top_paths: ranked
            .iter()
            .take(TOP_PATHS)
            .map(|file| file.filename.clone())
            .collect(),
        review_rounds: u32::try_from(rounds).unwrap_or(0),
        reviewers: reviewer_names(rows),
        same_model,
        bots_absent,
        findings_raised: raised,
        findings_addressed: raised.saturating_sub(open),
        checks: match step.task.checks.as_deref() {
            Some("success") => "passed".to_string(),
            Some("absent") => "no checks reported on the head".to_string(),
            Some(other) => other.to_string(),
            None => "not recorded".to_string(),
        },
        admin_merge: merged.admin,
        merge_sha: merged.sha.clone(),
        post_merge: None,
    };
    step.drive
        .notify("merged", notification::merged(&report))
        .await;
    tracing::info!(task_id = %step.task.task_id, sha = %merged.sha, admin = merged.admin, "Auto project merged a pull request");
    auto_projects::set_head(pool, step.task.task_id, &merged.sha, Some("pending"), true)
        .await
        .map_err(|error| error.to_string())?;
    step.set(Stage::PostMerge, None).await
}

/// Watch the jobs the merge triggered and file a fix task when one fails.
async fn post_merge(step: &Step<'_>) -> Result<(), String> {
    let pool = step.drive.state.db();
    let config = step.drive.config;
    let Some(sha) = step.task.merge_sha.clone() else {
        return step
            .set(Stage::Merged, Some("post-merge: no merge commit recorded"))
            .await;
    };
    if config.post_merge_secs == 0 {
        return step
            .set(Stage::Merged, Some("post-merge: not watched"))
            .await;
    }
    let checks = step
        .drive
        .services
        .pr
        .fetch_checks(
            &step.reference.owner,
            &step.reference.repository,
            &sha,
            &step.token,
        )
        .await
        .map_err(|error| error.to_string())?;
    let elapsed = step.seconds_since_checks();
    match checks {
        ChecksOutcome::Success => {
            step.set(Stage::Merged, Some("post-merge: jobs passed"))
                .await
        }
        ChecksOutcome::Absent => {
            if elapsed >= i64::try_from(config.checks_grace_secs).unwrap_or(i64::MAX) {
                step.set(Stage::Merged, Some("post-merge: no jobs ran on the base"))
                    .await
            } else {
                step.set(Stage::PostMerge, Some("waiting for post-merge jobs"))
                    .await
            }
        }
        ChecksOutcome::Pending => {
            if elapsed >= i64::try_from(config.post_merge_secs).unwrap_or(i64::MAX) {
                step.set(
                    Stage::Merged,
                    Some(&format!(
                        "post-merge: jobs still running after {} seconds",
                        config.post_merge_secs
                    )),
                )
                .await
            } else {
                step.set(Stage::PostMerge, Some("waiting for post-merge jobs"))
                    .await
            }
        }
        ChecksOutcome::Failure(names) => {
            let fixes =
                auto_projects::count_auto_created(pool, step.drive.project.project_id, Kind::Fix)
                    .await
                    .map_err(|error| error.to_string())?;
            if fixes < i64::from(config.max_fix_tasks) {
                let title = format!(
                    "Fix the failing {} job after merging {}",
                    names.join(", "),
                    step.row.title
                );
                let description = format!(
                    "After pull request {} merged as {}, these jobs failed on the base branch: {}. \
                     Read the failing job logs, fix the cause, and make the jobs pass again. Do not \
                     revert the merged change unless nothing else can make the jobs pass.",
                    step.pr_url,
                    short(&sha),
                    names.join(", ")
                );
                step.drive
                    .add_task(
                        Kind::Fix,
                        &title,
                        &description,
                        Some("The named jobs pass on the base branch."),
                        "A job failed on the base branch after a merge, so a fix task was added.",
                    )
                    .await?;
                step.set(
                    Stage::Merged,
                    Some(&format!(
                        "post-merge: {} failed; a fix task was added",
                        names.join(", ")
                    )),
                )
                .await
            } else {
                step.set(
                    Stage::Merged,
                    Some(&format!(
                        "post-merge: {} failed and the project has used its {} fix tasks",
                        names.join(", "),
                        config.max_fix_tasks
                    )),
                )
                .await
            }
        }
    }
}

/// The first seven characters of a commit sha.
fn short(sha: &str) -> &str {
    if sha.len() >= 12 { &sha[..12] } else { sha }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    fn row(
        round: i32,
        bot: bool,
        reviewer: &str,
        verdict: &str,
        same_model: bool,
        head: &str,
    ) -> ReviewRow {
        ReviewRow {
            id: Uuid::new_v4(),
            task_id: Uuid::nil(),
            run_id: None,
            round,
            head: head.into(),
            reviewer_kind: if bot { "bot" } else { "model" }.into(),
            reviewer: reviewer.into(),
            author_model: None,
            same_model,
            verdict: verdict.into(),
            summary: String::new(),
            findings: json!([]),
            addressed: json!([]),
            external_id: None,
            created_at: Utc::now(),
        }
    }

    fn finding(id: &str) -> Finding {
        Finding {
            id: id.into(),
            severity: "major".into(),
            file: None,
            line: None,
            title: "open".into(),
            detail: String::new(),
            thread_id: None,
            reviewer: None,
        }
    }

    #[test]
    fn a_merge_needs_an_approving_review_of_this_head_with_nothing_open() {
        let head = "h1";
        assert!(matches!(decide(&[], &[], head, true), Decision::Fix(_)));
        let approve = [row(1, false, "big", "approve", false, head)];
        assert_eq!(decide(&approve, &[], head, true), Decision::Merge);
        assert!(matches!(
            decide(&approve, &[finding("r1-1")], head, true),
            Decision::Fix(_)
        ));
        let stale = [row(1, false, "big", "approve", false, "h0")];
        assert!(matches!(decide(&stale, &[], head, true), Decision::Fix(_)));
        let changes = [row(1, false, "big", "request_changes", false, head)];
        assert!(matches!(
            decide(&changes, &[], head, true),
            Decision::Fix(_)
        ));
    }

    #[test]
    fn a_bot_below_its_bar_blocks_and_a_distinct_review_is_required_when_asked() {
        let head = "h1";
        let rows = [
            row(1, true, "coderabbitai", "request_changes", false, head),
            row(2, false, "big", "approve", false, head),
        ];
        assert!(
            matches!(decide(&rows, &[], head, true), Decision::Fix(reason) if reason.contains("CodeRabbit"))
        );
        let clean = [
            row(1, true, "coderabbitai", "approve", false, head),
            row(2, false, "same", "approve", true, head),
        ];
        assert_eq!(decide(&clean, &[], head, true), Decision::Merge);
        let same_only = [row(1, false, "same", "approve", true, head)];
        assert!(matches!(
            decide(&same_only, &[], head, false),
            Decision::Fix(_)
        ));
        assert_eq!(decide(&same_only, &[], head, true), Decision::Merge);
    }
}
