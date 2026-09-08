//! PR creation worker
//!
//! Creates pull requests when a task completes with code changes.
//! Called by the task worker after successful task execution.

use std::path::Path;

use crate::db::tasks;
use crate::services::checkout::Repository;
use crate::state::AppState;
use zone_vcs::git::{GitError, GitService};
use zone_vcs::pull_request::PrService;

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

#[async_trait::async_trait]
trait Remote: Send + Sync {
    async fn push(&self, path: &Path, branch: &str, url: &str, token: &str) -> Result<(), String>;
}

#[async_trait::async_trait]
impl Remote for GitService {
    async fn push(&self, path: &Path, branch: &str, url: &str, token: &str) -> Result<(), String> {
        self.push_with_token(path, branch, url, token)
            .await
            .map_err(|error| error.to_string())
    }
}

/// Publish only on behalf of the current run and its still-authorized writer.
pub async fn create_pr_for_task(
    state: &AppState,
    execution: tasks::Execution,
    workspace_path: &Path,
) -> PrCreationResult {
    let git = GitService::new();
    Publication {
        state,
        execution,
        path: workspace_path,
        git: &git,
        remote: &git,
        service: PrService::new(),
    }
    .run()
    .await
    .unwrap_or_else(PrCreationResult::Error)
}

struct Publication<'a> {
    state: &'a AppState,
    execution: tasks::Execution,
    path: &'a Path,
    git: &'a GitService,
    remote: &'a dyn Remote,
    service: PrService,
}

impl Publication<'_> {
    async fn authorized(&self) -> Result<(), String> {
        match self.execution.authorized(self.state.db(), true).await {
            Ok(true) => Ok(()),
            _ => Err("Task publication lost its execution lease or writer access".to_string()),
        }
    }

    async fn record(&self, url: &str, branch: &str, status: &str) -> Result<(), String> {
        match tasks::update_task_pr(self.state.db(), &self.execution, url, branch, status).await {
            Ok(true) => Ok(()),
            _ => Err("Task publication no longer owns its metadata".to_string()),
        }
    }

    async fn run(&self) -> Result<PrCreationResult, String> {
        let task = tasks::get_task(self.state.db(), self.execution.task)
            .await
            .map_err(|error| error.to_string())?
            .ok_or("Task not found")?;
        if task.github_repo_url.is_none() && task.project_ids.is_empty() {
            return Ok(PrCreationResult::NoRepository);
        }
        self.authorized().await?;
        let Some(repository) = Repository::resolve(self.state.db(), &task).await? else {
            return Ok(PrCreationResult::NoRepository);
        };
        let token = repository
            .token
            .ok_or("No access token configured for the selected repository")?;
        self.authorized().await?;
        if !self
            .git
            .is_git_repo(self.path)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err("Workspace is not a git repository".to_string());
        }
        let origin = self
            .git
            .get_remote_url(self.path, "origin")
            .await
            .map_err(|error| error.to_string())?;
        if origin != repository.url {
            return Err("Checkout repository no longer matches the task repository".to_string());
        }
        if !self
            .git
            .has_changes(self.path)
            .await
            .map_err(|error| error.to_string())?
        {
            return Ok(PrCreationResult::NoChanges);
        }
        let summary = self
            .git
            .diff_summary(self.path)
            .await
            .map_err(|error| error.to_string())?;
        let changes = format!(
            "{} files changed, {} insertions(+), {} deletions(-)\n\n{}",
            summary.files_changed.len(),
            summary.insertions,
            summary.deletions,
            summary
                .files_changed
                .iter()
                .map(|file| format!("- `{file}`"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let branch = self.git.generate_branch_name(task.id, &task.title);
        let (owner, repository_name) = self
            .service
            .parse_github_url(&repository.url)
            .map_err(|error| error.to_string())?;

        // Resolve remote state before mutations; every response is followed by
        // a fresh authorization check before the next publication step.
        self.authorized().await?;
        let base = self
            .service
            .get_default_branch(&owner, &repository_name, &token)
            .await
            .map_err(|error| error.to_string())?;
        self.authorized().await?;
        let existing = self
            .service
            .pr_exists_for_branch(&owner, &repository_name, &token, &branch)
            .await
            .map_err(|error| error.to_string())?;
        self.authorized().await?;
        if let Some(url) = existing {
            self.record(&url, &branch, "open").await?;
            return Ok(PrCreationResult::PrAlreadyExists { pr_url: url });
        }

        self.authorized().await?;
        match self.git.create_branch(self.path, &branch).await {
            Ok(()) => {}
            Err(GitError::BranchExists(_)) => {
                self.authorized().await?;
                self.git
                    .checkout(self.path, &branch)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
        self.authorized().await?;
        self.git
            .stage_all(self.path)
            .await
            .map_err(|error| error.to_string())?;
        self.authorized().await?;
        let message = format!(
            "[Zone] {}\n\nTask ID: {}\n\nAutomatically committed by Zone after task completion.",
            task.title, task.id
        );
        match self.git.commit(self.path, &message).await {
            Ok(_) => {}
            Err(GitError::NoChanges) => return Ok(PrCreationResult::NoChanges),
            Err(error) => return Err(error.to_string()),
        }
        if !tasks::update_task_branch(self.state.db(), &self.execution, &branch)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err("Task publication no longer owns its branch metadata".to_string());
        }
        self.authorized().await?;
        self.remote
            .push(self.path, &branch, &repository.url, &token)
            .await
            .map_err(|error| error.to_string())?;
        self.authorized().await?;
        let title = self.service.generate_pr_title(&task.title, task.id);
        let body = self.service.generate_pr_body(
            &task.title,
            &task.description,
            task.id,
            Some(&changes),
            None,
        );
        let created = self
            .service
            .create_pr(
                &owner,
                &repository_name,
                &token,
                &branch,
                &base,
                &title,
                &body,
                false,
            )
            .await
            .map_err(|error| error.to_string())?;
        self.record(&created.url, &branch, &created.state).await?;
        Ok(PrCreationResult::Created {
            pr_url: created.url,
            branch_name: branch,
        })
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
    use crate::db::{organizations, projects, users, workspace_members, workspaces};
    use sqlx::PgPool;
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;
    use uuid::Uuid;

    struct Fixture {
        state: AppState,
        execution: tasks::Execution,
        organization: Uuid,
        workspace: Uuid,
        path: PathBuf,
    }

    impl Fixture {
        async fn new() -> Self {
            let database =
                std::env::var("TEST_DATABASE_URL").expect("disposable TEST_DATABASE_URL");
            let pool = PgPool::connect(&database).await.unwrap();
            let organization = organizations::create_organization(
                &pool,
                "Publication test",
                &Uuid::new_v4().to_string(),
                None,
            )
            .await
            .unwrap();
            let workspace = workspaces::create_workspace(
                &pool,
                organization.id,
                "Publication",
                &Uuid::new_v4().to_string(),
                None,
            )
            .await
            .unwrap();
            let user = users::create_user(
                &pool,
                &format!("{}@example.test", Uuid::new_v4()),
                "unused",
                Some("Actor"),
                false,
            )
            .await
            .unwrap();
            workspace_members::add_member(
                &pool,
                workspace.id,
                user.id,
                workspace_members::WorkspaceRole::Member,
                None,
            )
            .await
            .unwrap();
            let project = projects::create_project(&pool, "Repository", None, Some(workspace.id))
                .await
                .unwrap();
            projects::link_github(
                &pool,
                project.id,
                "https://github.com/owner/repository.git",
                Some("fixture-token"),
            )
            .await
            .unwrap();
            let task = tasks::create_task_as(
                &pool,
                workspace.id,
                &[project.id],
                "Publication",
                "Fixture",
                None,
                None,
                true,
                None,
                Some(user.id),
            )
            .await
            .unwrap();
            let run = tasks::create_task_run_as(&pool, task.id, Some(user.id))
                .await
                .unwrap();
            let owner = Uuid::new_v4();
            assert!(tasks::claim_task_run(&pool, run.id, owner).await.unwrap());
            let path =
                std::env::temp_dir().join(format!("zone-publication-test-{}", Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            for arguments in [
                vec!["init", "--quiet"],
                vec!["config", "user.name", "Fixture"],
                vec!["config", "user.email", "fixture@example.test"],
                vec![
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/owner/repository.git",
                ],
            ] {
                assert!(
                    std::process::Command::new("git")
                        .args(arguments)
                        .current_dir(&path)
                        .output()
                        .unwrap()
                        .status
                        .success()
                );
            }
            std::fs::write(path.join("sentinel"), "original").unwrap();
            for arguments in [
                vec!["add", "sentinel"],
                vec!["commit", "--quiet", "-m", "fixture"],
            ] {
                assert!(
                    std::process::Command::new("git")
                        .args(arguments)
                        .current_dir(&path)
                        .output()
                        .unwrap()
                        .status
                        .success()
                );
            }
            std::fs::write(path.join("sentinel"), "changed").unwrap();
            Self {
                state: AppState::new(crate::state::test_config(), pool, None),
                execution: tasks::Execution {
                    task: task.id,
                    run: run.id,
                    owner,
                    actor: Some(user.id),
                },
                organization: organization.id,
                workspace: workspace.id,
                path,
            }
        }

        async fn cleanup(&self) {
            sqlx::query("DELETE FROM organizations WHERE id=$1")
                .bind(self.organization)
                .execute(self.state.db())
                .await
                .unwrap();
            sqlx::query("DELETE FROM users WHERE id=$1")
                .bind(self.execution.actor)
                .execute(self.state.db())
                .await
                .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    struct RecordingRemote(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl Remote for RecordingRemote {
        async fn push(
            &self,
            _path: &Path,
            _branch: &str,
            _url: &str,
            _token: &str,
        ) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    async fn blocked_publication(revoke: bool) {
        use axum::{Json, Router, routing::any};
        let fixture = Fixture::new().await;
        let requested = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let requests = Arc::new(AtomicUsize::new(0));
        let pushes = Arc::new(AtomicUsize::new(0));
        let app = Router::new().fallback(any({
            let requested = requested.clone();
            let release = release.clone();
            let requests = requests.clone();
            move || {
                let requested = requested.clone();
                let release = release.clone();
                let requests = requests.clone();
                async move {
                    requests.fetch_add(1, Ordering::SeqCst);
                    requested.notify_one();
                    release.notified().await;
                    Json(serde_json::json!({"default_branch":"main"}))
                }
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let state = fixture.state.clone();
        let execution = fixture.execution;
        let path = fixture.path.clone();
        let remote = RecordingRemote(pushes.clone());
        let mut pipeline = tokio::spawn(async move {
            let git = GitService::new();
            Publication {
                state: &state,
                execution,
                path: &path,
                git: &git,
                remote: &remote,
                service: PrService::with_base_url(endpoint),
            }
            .run()
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), requested.notified())
            .await
            .unwrap();
        if revoke {
            sqlx::query(
                "UPDATE workspace_members SET role='viewer' WHERE workspace_id=$1 AND user_id=$2",
            )
            .bind(fixture.workspace)
            .bind(execution.actor)
            .execute(fixture.state.db())
            .await
            .unwrap();
        } else {
            sqlx::query("UPDATE task_runs SET owner=$2 WHERE id=$1")
                .bind(execution.run)
                .bind(Uuid::new_v4())
                .execute(fixture.state.db())
                .await
                .unwrap();
        }
        sqlx::query(
            "UPDATE tasks SET branch_name='new-owner-branch', pr_url='new-owner-url' WHERE id=$1",
        )
        .bind(execution.task)
        .execute(fixture.state.db())
        .await
        .unwrap();
        release.notify_one();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), &mut pipeline).await;
        pipeline.abort();
        server.abort();
        assert!(result.unwrap().unwrap().is_err());
        assert_eq!(
            requests.load(Ordering::SeqCst),
            1,
            "stale pipeline made a later GitHub request"
        );
        assert_eq!(
            pushes.load(Ordering::SeqCst),
            0,
            "stale pipeline pushed changes"
        );
        let task = tasks::get_task(fixture.state.db(), execution.task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.branch_name.as_deref(), Some("new-owner-branch"));
        assert_eq!(task.pr_url.as_deref(), Some("new-owner-url"));
        fixture.cleanup().await;
    }

    #[tokio::test]
    async fn owner_change_during_remote_read_prevents_publication() {
        blocked_publication(false).await;
    }

    #[tokio::test]
    async fn writer_revocation_during_remote_read_prevents_publication() {
        blocked_publication(true).await;
    }

    #[tokio::test]
    async fn late_publication_metadata_cannot_overwrite_current_owner() {
        let fixture = Fixture::new().await;
        assert!(
            tasks::update_task_branch(fixture.state.db(), &fixture.execution, "first-branch")
                .await
                .unwrap()
        );
        sqlx::query("UPDATE task_runs SET owner=$2 WHERE id=$1")
            .bind(fixture.execution.run)
            .bind(Uuid::new_v4())
            .execute(fixture.state.db())
            .await
            .unwrap();
        assert!(
            !tasks::update_task_branch(fixture.state.db(), &fixture.execution, "late-branch")
                .await
                .unwrap()
        );
        assert!(
            !tasks::update_task_pr(
                fixture.state.db(),
                &fixture.execution,
                "late-url",
                "late-branch",
                "open"
            )
            .await
            .unwrap()
        );
        let task = tasks::get_task(fixture.state.db(), fixture.execution.task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(task.branch_name.as_deref(), Some("first-branch"));
        assert!(task.pr_url.is_none());
        fixture.cleanup().await;
    }
}
