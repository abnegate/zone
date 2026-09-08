//! PR creation worker
//!
//! Creates pull requests when a task completes with code changes, records how
//! each one was received once people had a look at it, and repairs a branch that
//! stopped merging with its base.
//!
//! The reception facts are a separate pass rather than part of creation, because
//! at the moment a pull request is opened nothing has happened to it yet: there is
//! no merge time, no review round and no approval. They are merged into the run's
//! existing `pr` artifact key one at a time, so a later sync never overwrites
//! `pr_url` or `branch_name` and never disturbs the other artifact keys the run
//! wrote.

use std::path::Path;
use uuid::Uuid;

use serde_json::{Value, json};

use crate::db::{ai_settings, projects, tasks, workspaces};
use crate::services::stages;
use crate::state::AppState;
use crate::workers::conflict::agent::ModelRepairAgent;
use crate::workers::conflict::{RepairOutcome, RepairRequest, repair};
use crate::workers::learning::artifacts::{PULL_REQUEST_KEY, REVIEW_KEY};
use zone_core::llm::{LlmClient, LlmConfig};
use zone_vcs::conflict::{BranchName, ConflictService};
use zone_vcs::git::{GitError, GitService};
use zone_vcs::pull_request::{PrError, PrService, PullRequestReception, PullRequestReference};

/// Temperature for a repair: a merge resolution is a mechanical edit, not a draft.
const REPAIR_TEMPERATURE: f32 = 0.0;

/// Tokens a repair turn may spend on its reply.
const REPAIR_TOKENS: u32 = 8_192;

/// Result of PR creation attempt
#[derive(Debug)]
pub enum PrCreationResult {
    /// PR was created successfully
    Created { pr_url: String, branch_name: String },
    /// No changes to commit
    NoChanges,
    /// No repository configured for project
    NoRepository,
    /// PR already exists for this branch
    PrAlreadyExists { pr_url: String },
    /// Error during PR creation
    Error(String),
}

/// Create a PR for task changes
///
/// This function:
/// 1. Checks if the project has a GitHub repository configured
/// 2. Checks if there are uncommitted changes in the workspace
/// 3. Creates a branch named after the task
/// 4. Commits and pushes changes
/// 5. Creates a pull request
/// 6. Updates the task with PR information
pub async fn create_pr_for_task(
    state: &AppState,
    task_id: Uuid,
    workspace_path: &Path,
) -> PrCreationResult {
    let git_service = GitService::new();
    let pr_service = PrService::new();

    let task = match tasks::get_task(state.db(), task_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return PrCreationResult::Error(format!("Task {} not found", task_id));
        }
        Err(e) => {
            return PrCreationResult::Error(format!("Failed to get task: {}", e));
        }
    };

    let project_id = match task.project_ids.first() {
        Some(id) => *id,
        None => {
            tracing::info!("No project associated with task {}", task_id);
            return PrCreationResult::NoRepository;
        }
    };

    let project = match projects::get_project(state.db(), project_id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return PrCreationResult::Error(format!("Project {} not found", project_id));
        }
        Err(e) => {
            return PrCreationResult::Error(format!("Failed to get project: {}", e));
        }
    };

    let repo_url = match &project.github_repo_url {
        Some(url) => url.clone(),
        None => {
            tracing::info!("No GitHub repository configured for project {}", project_id);
            return PrCreationResult::NoRepository;
        }
    };

    let access_token = match &project.github_access_token {
        Some(token) => token.clone(),
        None => {
            tracing::warn!(
                "No GitHub access token configured for project {}",
                project_id
            );
            return PrCreationResult::Error("No GitHub access token configured".to_string());
        }
    };

    match git_service.is_git_repo(workspace_path).await {
        Ok(true) => {}
        Ok(false) => {
            return PrCreationResult::Error("Workspace is not a git repository".to_string());
        }
        Err(e) => {
            return PrCreationResult::Error(format!("Failed to check git repo: {}", e));
        }
    }

    match git_service.has_changes(workspace_path).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::info!("No changes to commit for task {}", task_id);
            return PrCreationResult::NoChanges;
        }
        Err(e) => {
            return PrCreationResult::Error(format!("Failed to check for changes: {}", e));
        }
    }

    let branch_name = git_service.generate_branch_name(task_id, &task.title);

    let diff_summary = match git_service.diff_summary(workspace_path).await {
        Ok(summary) => {
            let file_list = summary
                .files_changed
                .iter()
                .map(|f| format!("- `{}`", f))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{} files changed, {} insertions(+), {} deletions(-)\n\n{}",
                summary.files_changed.len(),
                summary.insertions,
                summary.deletions,
                file_list
            )
        }
        Err(e) => {
            tracing::warn!("Failed to get diff summary: {}", e);
            "Unable to generate change summary".to_string()
        }
    };

    let original_branch = match git_service.current_branch(workspace_path).await {
        Ok(b) => b,
        Err(e) => {
            return PrCreationResult::Error(format!("Failed to get current branch: {}", e));
        }
    };

    match git_service
        .create_branch(workspace_path, &branch_name)
        .await
    {
        Ok(()) => {}
        Err(GitError::BranchExists(_)) => {
            if let Err(e) = git_service.checkout(workspace_path, &branch_name).await {
                return PrCreationResult::Error(format!(
                    "Failed to checkout existing branch: {}",
                    e
                ));
            }
        }
        Err(e) => {
            return PrCreationResult::Error(format!("Failed to create branch: {}", e));
        }
    }

    if let Err(e) = git_service.stage_all(workspace_path).await {
        let _ = git_service.checkout(workspace_path, &original_branch).await;
        return PrCreationResult::Error(format!("Failed to stage changes: {}", e));
    }

    let commit_message = format!(
        "[Zone] {}\n\nTask ID: {}\n\nAutomatically committed by Zone after task completion.",
        task.title, task_id
    );

    match git_service.commit(workspace_path, &commit_message).await {
        Ok(_sha) => {}
        Err(GitError::NoChanges) => {
            let _ = git_service.checkout(workspace_path, &original_branch).await;
            return PrCreationResult::NoChanges;
        }
        Err(e) => {
            let _ = git_service.checkout(workspace_path, &original_branch).await;
            return PrCreationResult::Error(format!("Failed to commit: {}", e));
        }
    }

    if let Err(e) = tasks::update_task_branch(state.db(), task_id, &branch_name).await {
        tracing::error!("Failed to update task branch: {}", e);
    }

    if let Err(e) = git_service
        .push_with_token(workspace_path, &branch_name, &repo_url, &access_token)
        .await
    {
        let _ = git_service.checkout(workspace_path, &original_branch).await;
        return PrCreationResult::Error(format!("Failed to push: {}", e));
    }

    let (owner, repo) = match pr_service.parse_github_url(&repo_url) {
        Ok((o, r)) => (o, r),
        Err(e) => {
            return PrCreationResult::Error(format!("Invalid GitHub URL: {}", e));
        }
    };

    match pr_service
        .pr_exists_for_branch(&owner, &repo, &access_token, &branch_name)
        .await
    {
        Ok(Some(existing_url)) => {
            if let Err(e) =
                tasks::update_task_pr(state.db(), task_id, &existing_url, &branch_name, "open")
                    .await
            {
                tracing::error!("Failed to update task PR info: {}", e);
            }
            report_repair(state, task_id).await;
            return PrCreationResult::PrAlreadyExists {
                pr_url: existing_url,
            };
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!("Failed to check for existing PR: {}", e);
        }
    }

    let base_branch = match pr_service
        .get_default_branch(&owner, &repo, &access_token)
        .await
    {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("Failed to get default branch, using 'main': {}", e);
            "main".to_string()
        }
    };

    let pr_title = pr_service.generate_pr_title(&task.title, task_id);
    let pr_body = pr_service.generate_pr_body(
        &task.title,
        &task.description,
        task_id,
        Some(&diff_summary),
        None, // TODO: Add Zone task URL when available
    );

    match pr_service
        .create_pr(
            &owner,
            &repo,
            &access_token,
            &branch_name,
            &base_branch,
            &pr_title,
            &pr_body,
            false, // Not a draft
        )
        .await
    {
        Ok(created) => {
            if let Err(e) = tasks::update_task_pr(
                state.db(),
                task_id,
                &created.url,
                &branch_name,
                &created.state,
            )
            .await
            {
                tracing::error!("Failed to update task PR info: {}", e);
            }

            tracing::info!(
                "Created PR for task {}: {} ({})",
                task_id,
                created.url,
                branch_name
            );

            report_repair(state, task_id).await;

            PrCreationResult::Created {
                pr_url: created.url,
                branch_name,
            }
        }
        Err(PrError::PrAlreadyExists(_)) => {
            // This shouldn't happen since we checked, but handle it
            PrCreationResult::Error("PR already exists".to_string())
        }
        Err(e) => PrCreationResult::Error(format!("Failed to create PR: {}", e)),
    }
}

/// How a reception sync ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceptionSyncResult {
    Recorded(Box<PullRequestReception>),
    NoPullRequest,
    NoCredentials,
    Error(String),
}

/// The `pr` fields a reception adds, and the `review` block beside it.
///
/// The two are separate because they are merged differently: the pull request
/// fields are folded into whatever `pr` already holds, while the review comments
/// replace the previous `review` block wholesale.
pub fn reception_artifacts(reception: &PullRequestReception) -> (Value, Value) {
    let mut pull_request = serde_json::Map::new();
    if let Some(opened_at) = &reception.opened_at {
        pull_request.insert("opened_at".to_string(), json!(opened_at));
    }
    if let Some(merged_at) = &reception.merged_at {
        pull_request.insert("merged_at".to_string(), json!(merged_at));
    }
    if let Some(minutes) = reception.minutes_to_merge {
        pull_request.insert("minutes_to_merge".to_string(), json!(minutes));
    }
    if let Some(state) = &reception.state {
        pull_request.insert("pr_state".to_string(), json!(state));
    }
    pull_request.insert("review_cycles".to_string(), json!(reception.review_cycles));
    pull_request.insert("approvals".to_string(), json!(reception.approvals));

    (
        Value::Object(pull_request),
        json!({ "comments": reception.comments }),
    )
}

/// Merge one reception into a run's artifacts, leaving every other key untouched.
///
/// The `pr` key is folded into rather than replaced, so the URL and branch name
/// the creation pass wrote survive; anything already stored under `pr` that is not
/// an object is discarded rather than concatenated into nonsense.
async fn record_reception(
    state: &AppState,
    run_id: Uuid,
    reception: &PullRequestReception,
) -> Result<bool, sqlx::Error> {
    let (pull_request, review) = reception_artifacts(reception);

    let outcome = sqlx::query(
        r#"
        UPDATE task_runs
        SET artifacts = COALESCE(artifacts, '{}'::jsonb)
            || jsonb_build_object(
                $2::text,
                CASE
                    WHEN jsonb_typeof(artifacts -> $2::text) = 'object' THEN artifacts -> $2::text
                    ELSE '{}'::jsonb
                END || $3::jsonb
            )
            || jsonb_build_object($4::text, $5::jsonb)
        WHERE id = $1
        "#,
    )
    .bind(run_id)
    .bind(PULL_REQUEST_KEY)
    .bind(pull_request.to_string())
    .bind(REVIEW_KEY)
    .bind(review.to_string())
    .execute(state.db())
    .await?;

    Ok(outcome.rows_affected() > 0)
}

/// Read back how a task's pull request was received and record it on the run.
pub async fn sync_reception(state: &AppState, run_id: Uuid, task_id: Uuid) -> ReceptionSyncResult {
    let task = match tasks::get_task(state.db(), task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => return ReceptionSyncResult::Error(format!("Task {} not found", task_id)),
        Err(error) => {
            return ReceptionSyncResult::Error(format!("Failed to get task: {}", error));
        }
    };

    let Some(pr_url) = task.pr_url.as_deref() else {
        return ReceptionSyncResult::NoPullRequest;
    };

    let reference = match PullRequestReference::parse(pr_url) {
        Ok(reference) => reference,
        Err(error) => {
            return ReceptionSyncResult::Error(format!("Invalid pull request URL: {}", error));
        }
    };

    let Some(access_token) = access_token(state, &task).await else {
        return ReceptionSyncResult::NoCredentials;
    };

    let reception = match PrService::new()
        .fetch_reception(&reference, &access_token)
        .await
    {
        Ok(reception) => reception,
        Err(error) => {
            return ReceptionSyncResult::Error(format!("Failed to read reception: {}", error));
        }
    };

    match record_reception(state, run_id, &reception).await {
        Ok(true) => ReceptionSyncResult::Recorded(Box::new(reception)),
        Ok(false) => ReceptionSyncResult::Error(format!("Task run {} not found", run_id)),
        Err(error) => ReceptionSyncResult::Error(format!("Failed to record reception: {}", error)),
    }
}

/// Repair a task's branch when it no longer merges with its base.
///
/// Everything that makes this safe lives in [`crate::workers::conflict`]: the
/// conflict is reproduced in a throwaway checkout rather than in any repository on
/// this machine, and a resolution that discards a branch's work is never published.
pub async fn repair_conflicts_for_task(state: &AppState, task_id: Uuid) -> RepairOutcome {
    let task = match tasks::get_task(state.db(), task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => return RepairOutcome::Failed(format!("Task {} not found", task_id)),
        Err(error) => return RepairOutcome::Failed(format!("Failed to get task: {}", error)),
    };

    let Some(branch_name) = task.branch_name.as_deref() else {
        return RepairOutcome::Failed("Task has no branch to repair".to_string());
    };

    let Some(project_id) = task.project_ids.first().copied() else {
        return RepairOutcome::Failed("Task has no project".to_string());
    };

    let project = match projects::get_project(state.db(), project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => return RepairOutcome::Failed(format!("Project {} not found", project_id)),
        Err(error) => return RepairOutcome::Failed(format!("Failed to get project: {}", error)),
    };

    let (Some(repo_url), Some(access_token)) =
        (&project.github_repo_url, &project.github_access_token)
    else {
        return RepairOutcome::Failed("No GitHub repository configured".to_string());
    };

    let pr_service = PrService::new();
    let (owner, repo) = match pr_service.parse_github_url(repo_url) {
        Ok(parsed) => parsed,
        Err(error) => return RepairOutcome::Failed(format!("Invalid GitHub URL: {}", error)),
    };

    if !conflicted(&pr_service, task.pr_url.as_deref(), access_token).await {
        return RepairOutcome::NotConflicted;
    }

    let base = match pr_service
        .get_default_branch(&owner, &repo, access_token)
        .await
    {
        Ok(base) => base,
        Err(error) => {
            return RepairOutcome::Failed(format!("Failed to get default branch: {}", error));
        }
    };

    let (head, base) = match (BranchName::parse(branch_name), BranchName::parse(&base)) {
        (Ok(head), Ok(base)) => (head, base),
        _ => return RepairOutcome::Failed("Branch names are not repairable".to_string()),
    };

    let model = repair_model(state, &task).await;
    let repairer = ModelRepairAgent::new(
        LlmClient::new(LlmConfig {
            base_url: state.config().litellm_host.clone(),
            api_key: state.config().litellm_key.clone(),
            default_model: model.clone(),
            temperature: REPAIR_TEMPERATURE,
            max_tokens: REPAIR_TOKENS,
        }),
        model,
    );

    repair(
        &ConflictService::new(),
        &repairer,
        &RepairRequest {
            remote: repo_url.clone(),
            token: Some(access_token.clone()),
            head,
            base,
            expected_head: None,
            expected_base: None,
            pull_request: task.pr_url.clone(),
        },
    )
    .await
}

/// Attempt a repair and say what came of it, without letting the outcome change
/// whether the pull request itself succeeded. A branch that cannot be repaired is
/// still a branch with a pull request open on it.
async fn report_repair(state: &AppState, task_id: Uuid) {
    match repair_conflicts_for_task(state, task_id).await {
        RepairOutcome::Repaired { files, commit } => tracing::info!(
            "Repaired {} conflicted file(s) for task {} as {}",
            files.len(),
            task_id,
            commit
        ),
        RepairOutcome::NotConflicted => {}
        RepairOutcome::Rejected { path, verdict } => tracing::warn!(
            "Refused a conflict repair for task {}: {} was {}",
            task_id,
            path,
            verdict
        ),
        RepairOutcome::Strayed(files) => tracing::warn!(
            "Refused a conflict repair for task {}: it changed {}",
            task_id,
            files.join(", ")
        ),
        RepairOutcome::Failed(reason) => {
            tracing::warn!("Conflict repair for task {} failed: {}", task_id, reason)
        }
    }
}

/// Whether GitHub says this branch has stopped merging with its base.
///
/// One cheap read stands between every successful task and the expensive work of
/// reproducing a merge. A branch GitHub has not finished checking answers no: a
/// pull request opened seconds ago is not yet known to conflict, and repairing on
/// a guess is how a repair ends up running against a tree nobody asked about.
async fn conflicted(service: &PrService, pr_url: Option<&str>, access_token: &str) -> bool {
    let Some(pr_url) = pr_url else {
        return false;
    };

    let Ok(reference) = PullRequestReference::parse(pr_url) else {
        return false;
    };

    match service.fetch_mergeability(&reference, access_token).await {
        Ok(mergeability) => mergeability.conflicted(),
        Err(error) => {
            tracing::warn!(%error, "Could not read pull request mergeability");
            false
        }
    }
}

/// The model a repair runs on: the one the task itself ran on, resolved the same
/// way, because the branch being repaired is that run's own work.
async fn repair_model(state: &AppState, task: &tasks::TaskRow) -> String {
    let catalog = stages::Catalog::load(&state.config().ollama_host).await;
    let settings = match workspaces::get_workspace(state.db(), task.workspace_id).await {
        Ok(Some(workspace)) => ai_settings::get_effective_ai_settings(
            state.db(),
            workspace.organization_id,
            task.workspace_id,
        )
        .await
        .ok(),
        _ => None,
    };

    stages::chat_model(
        task.model_name.as_deref().unwrap_or(stages::AUTO),
        &stages::Preferences::from_optional_settings(
            settings.as_ref(),
            &state.config().comfyui.classifier_model,
        ),
        &catalog,
        &format!("{}\n\n{}", task.title, task.description),
        false,
        true,
    )
}

async fn access_token(state: &AppState, task: &tasks::TaskRow) -> Option<String> {
    for project_id in &task.project_ids {
        if let Ok(Some(project)) = projects::get_project(state.db(), *project_id).await
            && let Some(token) = project.github_access_token
        {
            return Some(token);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::learning::artifacts;

    /// Apply a reception to an artifacts document the way the merge query does.
    fn merged(existing: Value, reception: &PullRequestReception) -> Value {
        let (pull_request, review) = reception_artifacts(reception);
        let mut artifacts = existing;

        let mut pr = match artifacts.get(PULL_REQUEST_KEY) {
            Some(Value::Object(existing)) => existing.clone(),
            _ => serde_json::Map::new(),
        };
        if let Value::Object(fields) = pull_request {
            pr.extend(fields);
        }

        artifacts[PULL_REQUEST_KEY] = Value::Object(pr);
        artifacts[REVIEW_KEY] = review;
        artifacts
    }

    fn reception() -> PullRequestReception {
        PullRequestReception {
            opened_at: Some("2026-09-04T09:00:00Z".to_string()),
            merged_at: Some("2026-09-04T11:00:00Z".to_string()),
            minutes_to_merge: Some(120),
            review_cycles: 1,
            approvals: 2,
            state: Some("closed".to_string()),
            comments: vec![
                "needs a regression test".to_string(),
                "rename this".to_string(),
            ],
        }
    }

    #[test]
    fn what_is_written_is_what_the_quality_score_reads_back() {
        let artifacts = merged(
            json!({ "pr": { "pr_url": "https://github.com/acme/project/pull/7" } }),
            &reception(),
        );

        let read = artifacts::reception(Some(&artifacts), None);
        assert_eq!(read.minutes_to_merge, Some(120));
        assert_eq!(read.review_cycles, 1);
        assert_eq!(read.approvals, 2);
    }

    #[test]
    fn a_merge_time_survives_even_without_the_recorded_minutes() {
        let mut without_minutes = reception();
        without_minutes.minutes_to_merge = None;

        let artifacts = merged(json!({}), &without_minutes);
        assert_eq!(
            artifacts::reception(Some(&artifacts), None).minutes_to_merge,
            Some(120),
            "the timestamps alone must be enough for the consumer to derive the duration"
        );
    }

    #[test]
    fn what_is_written_is_what_the_review_classifier_reads_back() {
        let artifacts = merged(json!({}), &reception());
        assert_eq!(
            artifacts::review_comments(Some(&artifacts)),
            vec!["needs a regression test", "rename this"]
        );
    }

    #[test]
    fn the_object_comment_shape_reads_back_the_same_way() {
        let artifacts = json!({
            "review": { "comments": [{ "body": "needs a regression test" }, { "body": "rename this" }] }
        });
        assert_eq!(
            artifacts::review_comments(Some(&artifacts)),
            vec!["needs a regression test", "rename this"],
            "both accepted comment shapes must reach the classifier identically"
        );
    }

    #[test]
    fn recording_a_reception_keeps_what_pull_request_creation_wrote() {
        let artifacts = merged(
            json!({
                "pr": {
                    "pr_url": "https://github.com/acme/project/pull/7",
                    "branch_name": "zone/task-123",
                },
                "attempts": 2,
                "evaluation": { "verdict": "improved" },
            }),
            &reception(),
        );

        assert_eq!(
            artifacts["pr"]["pr_url"],
            json!("https://github.com/acme/project/pull/7")
        );
        assert_eq!(artifacts["pr"]["branch_name"], json!("zone/task-123"));
        assert_eq!(artifacts["attempts"], json!(2));
        assert_eq!(artifacts["evaluation"]["verdict"], json!("improved"));
    }

    #[test]
    fn a_pull_request_key_that_is_not_an_object_is_replaced_rather_than_corrupted() {
        let artifacts = merged(json!({ "pr": "https://example.test/pull/1" }), &reception());
        assert_eq!(artifacts["pr"]["approvals"], json!(2));
        assert_eq!(
            artifacts::reception(Some(&artifacts), None).approvals,
            2,
            "a malformed earlier write must not stop the reception being readable"
        );
    }

    #[test]
    fn an_unmerged_pull_request_records_no_merge_time_at_all() {
        let open = PullRequestReception {
            opened_at: Some("2026-09-04T09:00:00Z".to_string()),
            state: Some("open".to_string()),
            ..PullRequestReception::default()
        };

        let artifacts = merged(json!({}), &open);
        assert!(artifacts["pr"].get("merged_at").is_none());
        assert!(artifacts["pr"].get("minutes_to_merge").is_none());
        assert_eq!(
            artifacts::reception(Some(&artifacts), None).minutes_to_merge,
            None,
            "an open pull request must score neutrally rather than instantly"
        );
    }

    #[test]
    fn a_reception_with_no_comments_writes_an_empty_list_not_a_missing_one() {
        let artifacts = merged(json!({}), &PullRequestReception::default());
        assert_eq!(artifacts["review"]["comments"], json!([]));
        assert!(artifacts::review_comments(Some(&artifacts)).is_empty());
    }

    #[test]
    fn test_pr_creation_result_debug() {
        let result = PrCreationResult::Created {
            pr_url: "https://github.com/test/repo/pull/1".to_string(),
            branch_name: "zone/task-123-test".to_string(),
        };
        let debug = format!("{:?}", result);
        assert!(debug.contains("Created"));
    }

    #[test]
    fn test_pr_creation_result_no_changes() {
        let result = PrCreationResult::NoChanges;
        let debug = format!("{:?}", result);
        assert!(debug.contains("NoChanges"));
    }
}
