//! PR creation worker
//!
//! Creates pull requests when a task completes with code changes.
//! Called by the task worker after successful task execution.

use std::path::Path;
use uuid::Uuid;

use crate::db::{projects, tasks};
use crate::services::git::{GitError, GitService};
use crate::services::pr::{PrError, PrService};
use crate::state::AppState;

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

    // Get task details
    let task = match tasks::get_task(state.db(), task_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return PrCreationResult::Error(format!("Task {} not found", task_id));
        }
        Err(e) => {
            return PrCreationResult::Error(format!("Failed to get task: {}", e));
        }
    };

    // Get first associated project to find GitHub repo info
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

    // Check if GitHub is configured
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

    // Check if workspace is a git repo
    match git_service.is_git_repo(workspace_path).await {
        Ok(true) => {}
        Ok(false) => {
            return PrCreationResult::Error("Workspace is not a git repository".to_string());
        }
        Err(e) => {
            return PrCreationResult::Error(format!("Failed to check git repo: {}", e));
        }
    }

    // Check for uncommitted changes
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

    // Generate branch name
    let branch_name = git_service.generate_branch_name(task_id, &task.title);

    // Get diff summary for PR body
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

    // Store original branch to restore on error
    let original_branch = match git_service.current_branch(workspace_path).await {
        Ok(b) => b,
        Err(e) => {
            return PrCreationResult::Error(format!("Failed to get current branch: {}", e));
        }
    };

    // Create branch
    match git_service
        .create_branch(workspace_path, &branch_name)
        .await
    {
        Ok(()) => {}
        Err(GitError::BranchExists(_)) => {
            // Branch exists, try to checkout and continue
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

    // Stage all changes
    if let Err(e) = git_service.stage_all(workspace_path).await {
        // Try to restore original branch
        let _ = git_service.checkout(workspace_path, &original_branch).await;
        return PrCreationResult::Error(format!("Failed to stage changes: {}", e));
    }

    // Commit changes
    let commit_message = format!(
        "[Zone] {}\n\nTask ID: {}\n\nAutomatically committed by Zone after task completion.",
        task.title, task_id
    );

    match git_service.commit(workspace_path, &commit_message).await {
        Ok(_sha) => {}
        Err(GitError::NoChanges) => {
            // Restore original branch
            let _ = git_service.checkout(workspace_path, &original_branch).await;
            return PrCreationResult::NoChanges;
        }
        Err(e) => {
            let _ = git_service.checkout(workspace_path, &original_branch).await;
            return PrCreationResult::Error(format!("Failed to commit: {}", e));
        }
    }

    // Update task with branch name
    if let Err(e) = tasks::update_task_branch(state.db(), task_id, &branch_name).await {
        tracing::error!("Failed to update task branch: {}", e);
    }

    // Push to remote
    if let Err(e) = git_service
        .push_with_token(workspace_path, &branch_name, &repo_url, &access_token)
        .await
    {
        let _ = git_service.checkout(workspace_path, &original_branch).await;
        return PrCreationResult::Error(format!("Failed to push: {}", e));
    }

    // Parse GitHub URL
    let (owner, repo) = match pr_service.parse_github_url(&repo_url) {
        Ok((o, r)) => (o, r),
        Err(e) => {
            return PrCreationResult::Error(format!("Invalid GitHub URL: {}", e));
        }
    };

    // Check if PR already exists
    match pr_service
        .pr_exists_for_branch(&owner, &repo, &access_token, &branch_name)
        .await
    {
        Ok(Some(existing_url)) => {
            // Update task with existing PR URL
            if let Err(e) =
                tasks::update_task_pr(state.db(), task_id, &existing_url, &branch_name, "open")
                    .await
            {
                tracing::error!("Failed to update task PR info: {}", e);
            }
            return PrCreationResult::PrAlreadyExists {
                pr_url: existing_url,
            };
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!("Failed to check for existing PR: {}", e);
            // Continue to try creating PR
        }
    }

    // Get default branch for PR base
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

    // Generate PR title and body
    let pr_title = pr_service.generate_pr_title(&task.title, task_id);
    let pr_body = pr_service.generate_pr_body(
        &task.title,
        &task.description,
        task_id,
        Some(&diff_summary),
        None, // TODO: Add Zone task URL when available
    );

    // Create PR
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
            // Update task with PR info
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

#[cfg(test)]
mod tests {
    use super::*;

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
