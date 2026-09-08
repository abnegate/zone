//! PR creation worker
//!
//! Creates pull requests when a task completes with code changes.
//! Called by the task worker after successful task execution.

use std::path::Path;

use crate::db::tasks;
use crate::services::checkout::{Baseline, Repository};
use crate::state::AppState;
use zone_vcs::git::GitService;
use zone_vcs::pull_request::PrService;

/// Result of PR creation attempt
#[derive(Debug)]
pub enum PrCreationResult {
    /// PR was created successfully
    Created { pr_url: String, branch_name: String },
    /// No changes to commit
    NoChanges,
    /// Legacy creator-null tasks intentionally execute only in the sandbox.
    Sandbox,
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
    baseline: Option<&Baseline>,
) -> PrCreationResult {
    let git = GitService::new();
    Publication {
        state,
        execution,
        path: workspace_path,
        baseline,
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
    baseline: Option<&'a Baseline>,
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

    async fn identity(&self, baseline: &Baseline) -> Result<(), String> {
        self.authorized().await?;
        let task = tasks::get_task(self.state.db(), self.execution.task)
            .await
            .map_err(|_| "Cannot verify publication identity")?
            .ok_or("Task not found")?;
        let repository = Repository::resolve(self.state.db(), &task)
            .await?
            .ok_or("Task repository changed during execution")?;
        if repository.url != baseline.repository
            || task.branch_name.as_deref() != Some(&baseline.branch)
        {
            return Err("Task repository or branch changed during execution".into());
        }
        if self
            .git
            .get_remote_url(self.path, "origin")
            .await
            .map_err(|error| error.to_string())?
            != baseline.repository
        {
            return Err("Checkout repository no longer matches the task repository".into());
        }
        if self
            .git
            .current_branch(self.path)
            .await
            .map_err(|error| error.to_string())?
            != baseline.branch
        {
            return Err(
                "Task changed the checkout branch; publication requires the original task branch"
                    .into(),
            );
        }
        if !self
            .git
            .is_ancestor(self.path, &baseline.commit, "HEAD")
            .await
            .map_err(|error| error.to_string())?
        {
            return Err(
                "Task rewrote the checkout baseline; publication requires its original history"
                    .into(),
            );
        }
        self.authorized().await
    }

    async fn run(&self) -> Result<PrCreationResult, String> {
        let task = tasks::get_task(self.state.db(), self.execution.task)
            .await
            .map_err(|_| "Cannot load publication task")?
            .ok_or("Task not found")?;
        if !self
            .execution
            .authorized(self.state.db(), false)
            .await
            .map_err(|_| "Cannot verify task authority")?
        {
            return Err("Task publication lost its execution lease or writer access".into());
        }
        if task.created_by.is_none() {
            return Ok(PrCreationResult::Sandbox);
        }
        if self.baseline.is_none() && Repository::resolve(self.state.db(), &task).await?.is_none() {
            self.authorized().await?;
            return Ok(PrCreationResult::NoRepository);
        }
        let baseline = self
            .baseline
            .ok_or("Task has no captured checkout baseline")?;
        self.identity(baseline).await?;
        let dirty = self
            .git
            .has_changes(self.path)
            .await
            .map_err(|error| error.to_string())?;
        let head = self
            .git
            .revision(self.path, "HEAD")
            .await
            .map_err(|error| error.to_string())?;
        // A retry may resume already-pushed work whose PR was never created.
        // Only a clean branch already included in the captured default base is a no-op.
        if !dirty
            && head == baseline.commit
            && self
                .git
                .is_ancestor(self.path, &head, &baseline.base)
                .await
                .map_err(|error| error.to_string())?
        {
            self.identity(baseline).await?;
            return Ok(PrCreationResult::NoChanges);
        }
        let repository = Repository::resolve(self.state.db(), &task)
            .await?
            .ok_or("Task repository changed during execution")?;
        let token = repository
            .token
            .ok_or("No access token configured for the selected repository")?;
        let branch = &baseline.branch;
        let (owner, repository_name) = self
            .service
            .parse_github_url(&baseline.repository)
            .map_err(|error| error.to_string())?;
        self.authorized().await?;
        let base = self
            .service
            .get_default_branch(&owner, &repository_name, &token)
            .await
            .map_err(|error| error.to_string())?;
        self.authorized().await?;
        let existing = self
            .service
            .pr_exists_for_branch(&owner, &repository_name, &token, branch)
            .await
            .map_err(|error| error.to_string())?;
        self.identity(baseline).await?;
        if dirty {
            self.git
                .stage_all(self.path)
                .await
                .map_err(|error| error.to_string())?;
            self.authorized().await?;
            let message = format!(
                "[Zone] {}\n\nTask ID: {}\n\nAutomatically committed by Zone after task completion.",
                task.title, task.id
            );
            self.git
                .commit(self.path, &message)
                .await
                .map_err(|error| error.to_string())?;
        }
        self.identity(baseline).await?;
        let files = self
            .git
            .changed_files(self.path, &baseline.base)
            .await
            .map_err(|error| error.to_string())?;
        self.authorized().await?;
        self.remote
            .push(self.path, branch, &baseline.repository, &token)
            .await?;
        self.identity(baseline).await?;
        if let Some(url) = existing {
            self.record(&url, branch, "open").await?;
            return Ok(PrCreationResult::PrAlreadyExists { pr_url: url });
        }
        let changes = files
            .iter()
            .map(|file| format!("- `{file}`"))
            .collect::<Vec<_>>()
            .join("\n");
        let title = self.service.generate_pr_title(&task.title, task.id);
        let body = self.service.generate_pr_body(
            &task.title,
            &task.description,
            task.id,
            Some(&changes),
            None,
        );
        self.authorized().await?;
        let created = self
            .service
            .create_pr(
                &owner,
                &repository_name,
                &token,
                branch,
                &base,
                &title,
                &body,
                false,
            )
            .await
            .map_err(|error| error.to_string())?;
        self.identity(baseline).await?;
        self.record(&created.url, branch, &created.state).await?;
        Ok(PrCreationResult::Created {
            pr_url: created.url,
            branch_name: branch.clone(),
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

    pub(super) struct Fixture {
        pub(super) state: AppState,
        pub(super) execution: tasks::Execution,
        organization: Uuid,
        user: Uuid,
        pub(super) workspace: Uuid,
        pub(super) path: PathBuf,
        pub(super) baseline: Baseline,
        _checkout: crate::services::checkout::Checkout,
    }

    impl Fixture {
        pub(super) async fn new() -> Self {
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
            let execution = tasks::Execution {
                task: task.id,
                run: run.id,
                owner,
                actor: Some(user.id),
            };
            let mut sandbox = task.clone();
            sandbox.project_ids.clear();
            let checkout = crate::services::checkout::Checkout::prepare(&pool, &sandbox, execution)
                .await
                .unwrap();
            let path = checkout.path().to_path_buf();
            for arguments in [
                vec!["init", "--quiet", "--initial-branch=main"],
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
            let execution = tasks::Execution {
                task: task.id,
                run: run.id,
                owner,
                actor: Some(user.id),
            };
            let baseline = Baseline::prepare(
                &pool,
                &task,
                execution,
                &path,
                "https://github.com/owner/repository.git".into(),
            )
            .await
            .unwrap();
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
                user: user.id,
                workspace: workspace.id,
                path,
                baseline,
                _checkout: checkout,
            }
        }

        pub(super) async fn cleanup(&self) {
            sqlx::query("DELETE FROM organizations WHERE id=$1")
                .bind(self.organization)
                .execute(self.state.db())
                .await
                .unwrap();
            sqlx::query("DELETE FROM users WHERE id=$1")
                .bind(self.user)
                .execute(self.state.db())
                .await
                .unwrap();
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
        let baseline = fixture.baseline.clone();
        let remote = RecordingRemote(pushes.clone());
        let mut pipeline = tokio::spawn(async move {
            let git = GitService::new();
            Publication {
                state: &state,
                execution,
                path: &path,
                baseline: Some(&baseline),
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

#[cfg(test)]
mod publication_tests {
    use super::tests::Fixture;
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn git(directory: &Path, arguments: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(arguments)
            .current_dir(directory)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    struct LocalRemote(std::path::PathBuf);

    impl LocalRemote {
        fn new(fixture: &Fixture) -> Self {
            let path = fixture.path.with_extension("git");
            git(
                &fixture.path,
                &["clone", "--bare", ".", path.to_str().unwrap()],
            );
            git(&path, &["symbolic-ref", "HEAD", "refs/heads/main"]);
            Self(path)
        }
    }

    impl Drop for LocalRemote {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[async_trait::async_trait]
    impl Remote for LocalRemote {
        async fn push(
            &self,
            directory: &Path,
            branch: &str,
            _url: &str,
            _token: &str,
        ) -> Result<(), String> {
            let output = std::process::Command::new("git")
                .args(["push", "--", self.0.to_str().unwrap(), branch])
                .current_dir(directory)
                .output()
                .unwrap();
            if output.status.success() {
                Ok(())
            } else {
                Err("Git push rejected; remote work was not overwritten".to_string())
            }
        }
    }

    async fn api(existing: bool) -> MockServer {
        let server = MockServer::start().await;
        let response = serde_json::json!({"id":1,"number":1,"html_url":"https://github.com/owner/repository/pull/1","state":"open","title":"Task","body":null,"head":{"ref":"task","sha":"abc"},"base":{"ref":"main","sha":"def"}});
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"default_branch":"main"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repository/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(if existing {
                serde_json::json!([response.clone()])
            } else {
                serde_json::json!([])
            }))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repository/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(response))
            .mount(&server)
            .await;
        server
    }

    async fn publish(
        fixture: &Fixture,
        remote: &LocalRemote,
        server: &MockServer,
    ) -> PrCreationResult {
        let git = GitService::new();
        Publication {
            state: &fixture.state,
            execution: fixture.execution,
            path: &fixture.path,
            baseline: Some(&fixture.baseline),
            git: &git,
            remote,
            service: PrService::with_base_url(server.uri()),
        }
        .run()
        .await
        .unwrap_or_else(PrCreationResult::Error)
    }

    #[tokio::test]
    async fn publication_preserves_agent_commits_on_clean_checkout() {
        let fixture = Fixture::new().await;
        let remote = LocalRemote::new(&fixture);
        let server = api(false).await;
        git(&fixture.path, &["add", "."]);
        git(&fixture.path, &["commit", "-m", "agent commit"]);
        let result = publish(&fixture, &remote, &server).await;
        fixture.cleanup().await;
        assert!(
            matches!(result, PrCreationResult::Created { .. }),
            "{result:?}"
        );
        let branch = GitService::new().generate_branch_name(fixture.execution.task, "Publication");
        assert_eq!(
            git(&remote.0, &["show", &format!("{branch}:sentinel")]),
            "changed"
        );
    }

    #[tokio::test]
    async fn publication_updates_existing_pr_before_returning() {
        let fixture = Fixture::new().await;
        let remote = LocalRemote::new(&fixture);
        let server = api(true).await;
        let result = publish(&fixture, &remote, &server).await;
        fixture.cleanup().await;
        assert!(
            matches!(result, PrCreationResult::PrAlreadyExists { .. }),
            "{result:?}"
        );
        let branch = GitService::new().generate_branch_name(fixture.execution.task, "Publication");
        assert_eq!(
            git(&remote.0, &["show", &format!("{branch}:sentinel")]),
            "changed"
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.method != "POST")
        );
    }

    #[tokio::test]
    async fn publication_project_without_git_remains_a_sandbox() {
        let fixture = Fixture::new().await;
        let remote = LocalRemote::new(&fixture);
        let server = api(false).await;
        sqlx::query("UPDATE projects SET github_repo_url=NULL, github_access_token=NULL WHERE workspace_id=$1").bind(fixture.workspace).execute(fixture.state.db()).await.unwrap();
        let git = GitService::new();
        let result = Publication {
            state: &fixture.state,
            execution: fixture.execution,
            path: &fixture.path,
            baseline: None,
            git: &git,
            remote: &remote,
            service: PrService::with_base_url(server.uri()),
        }
        .run()
        .await
        .unwrap_or_else(PrCreationResult::Error);
        fixture.cleanup().await;
        assert!(
            matches!(result, PrCreationResult::NoRepository),
            "{result:?}"
        );
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn publication_failure_cannot_finish_replacement_execution() {
        let fixture = Fixture::new().await;
        sqlx::query(
            "UPDATE task_runs SET owner=$2, artifacts='{\"replacement\":true}'::jsonb WHERE id=$1",
        )
        .bind(fixture.execution.run)
        .bind(uuid::Uuid::new_v4())
        .execute(fixture.state.db())
        .await
        .unwrap();
        super::super::task::complete_publication(
            &fixture.state,
            fixture.execution,
            "stale".into(),
            0,
            PrCreationResult::Error("stale publication".into()),
        )
        .await;
        let run = tasks::get_task_run(fixture.state.db(), fixture.execution.run)
            .await
            .unwrap()
            .unwrap();
        fixture.cleanup().await;
        assert_eq!(run.status, "running");
        assert_eq!(
            run.artifacts,
            Some(serde_json::json!({"replacement": true}))
        );
        assert!(run.error_message.is_none());
    }

    #[tokio::test]
    async fn publication_no_change_is_noop() {
        let fixture = Fixture::new().await;
        let remote = LocalRemote::new(&fixture);
        let server = api(false).await;
        git(&fixture.path, &["checkout", "--", "sentinel"]);
        sqlx::query("UPDATE projects SET github_access_token=NULL WHERE workspace_id=$1")
            .bind(fixture.workspace)
            .execute(fixture.state.db())
            .await
            .unwrap();
        assert!(matches!(
            publish(&fixture, &remote, &server).await,
            PrCreationResult::NoChanges
        ));
        assert!(server.received_requests().await.unwrap().is_empty());
        let task = tasks::get_task(fixture.state.db(), fixture.execution.task)
            .await
            .unwrap()
            .unwrap();
        fixture.cleanup().await;
        assert!(
            task.pr_status.is_none(),
            "a no-op is not pending publication"
        );
    }

    #[tokio::test]
    async fn publication_resumes_previous_attempt_after_title_change() {
        let mut fixture = Fixture::new().await;
        let remote = LocalRemote::new(&fixture);
        let first_api = api(false).await;
        let result = publish(&fixture, &remote, &first_api).await;
        let PrCreationResult::Created { branch_name, .. } = result else {
            panic!("{result:?}")
        };
        let first = git(&remote.0, &["rev-parse", &branch_name]);
        tasks::complete_owned_task_run(
            fixture.state.db(),
            fixture.execution.run,
            Some(fixture.execution.owner),
            "completed",
            None,
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE tasks SET title='Changed title' WHERE id=$1")
            .bind(fixture.execution.task)
            .execute(fixture.state.db())
            .await
            .unwrap();
        let run = tasks::create_task_run_as(
            fixture.state.db(),
            fixture.execution.task,
            fixture.execution.actor,
        )
        .await
        .unwrap();
        fixture.execution.run = run.id;
        fixture.execution.owner = uuid::Uuid::new_v4();
        assert!(
            tasks::claim_task_run(fixture.state.db(), run.id, fixture.execution.owner)
                .await
                .unwrap()
        );
        std::fs::remove_dir_all(&fixture.path).unwrap();
        git(
            &remote.0,
            &[
                "clone",
                "--",
                remote.0.to_str().unwrap(),
                fixture.path.to_str().unwrap(),
            ],
        );
        git(
            &fixture.path,
            &[
                "remote",
                "set-url",
                "origin",
                "https://github.com/owner/repository.git",
            ],
        );
        let task = tasks::get_task(fixture.state.db(), fixture.execution.task)
            .await
            .unwrap()
            .unwrap();
        fixture.baseline = Baseline::prepare(
            fixture.state.db(),
            &task,
            fixture.execution,
            &fixture.path,
            "https://github.com/owner/repository.git".into(),
        )
        .await
        .unwrap();
        let resumed = tasks::get_task(fixture.state.db(), fixture.execution.task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resumed.pr_status.as_deref(), Some("open"));
        assert_eq!(
            std::fs::read_to_string(fixture.path.join("sentinel")).unwrap(),
            "changed",
            "second attempt did not resume first attempt's work"
        );
        std::fs::write(fixture.path.join("second"), "second attempt").unwrap();
        let second_api = api(true).await;
        let result = publish(&fixture, &remote, &second_api).await;
        let task = tasks::get_task(fixture.state.db(), fixture.execution.task)
            .await
            .unwrap()
            .unwrap();
        fixture.cleanup().await;
        assert!(
            matches!(result, PrCreationResult::PrAlreadyExists { .. }),
            "{result:?}"
        );
        assert_eq!(task.branch_name.as_deref(), Some(branch_name.as_str()));
        assert_eq!(
            git(&remote.0, &["show", &format!("{branch_name}:second")]),
            "second attempt"
        );
        git(
            &remote.0,
            &["merge-base", "--is-ancestor", &first, &branch_name],
        );
        assert!(
            second_api
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|request| request.method != "POST")
        );
    }

    #[tokio::test]
    async fn publication_handles_agent_switching_branches_without_losing_commits() {
        let fixture = Fixture::new().await;
        let remote = LocalRemote::new(&fixture);
        let server = api(false).await;
        git(&fixture.path, &["checkout", "-b", "agent-work"]);
        git(&fixture.path, &["add", "."]);
        git(&fixture.path, &["commit", "-m", "agent branch commit"]);
        let result = publish(&fixture, &remote, &server).await;
        fixture.cleanup().await;
        assert!(
            matches!(&result, PrCreationResult::Error(error) if error.contains("changed the checkout branch")),
            "{result:?}"
        );
        if matches!(result, PrCreationResult::Created { .. }) {
            let branch =
                GitService::new().generate_branch_name(fixture.execution.task, "Publication");
            assert_eq!(
                git(&remote.0, &["show", &format!("{branch}:sentinel")]),
                "changed"
            );
        }
    }

    #[tokio::test]
    async fn publication_recovers_pushed_branch_without_new_edits_or_pr() {
        let mut fixture = Fixture::new().await;
        let remote = LocalRemote::new(&fixture);
        git(&fixture.path, &["add", "."]);
        git(&fixture.path, &["commit", "-m", "pushed before crash"]);
        remote
            .push(&fixture.path, &fixture.baseline.branch, "", "")
            .await
            .unwrap();
        std::fs::remove_dir_all(&fixture.path).unwrap();
        git(
            &remote.0,
            &[
                "clone",
                "--",
                remote.0.to_str().unwrap(),
                fixture.path.to_str().unwrap(),
            ],
        );
        git(
            &fixture.path,
            &[
                "remote",
                "set-url",
                "origin",
                "https://github.com/owner/repository.git",
            ],
        );
        let task = tasks::get_task(fixture.state.db(), fixture.execution.task)
            .await
            .unwrap()
            .unwrap();
        fixture.baseline = Baseline::prepare(
            fixture.state.db(),
            &task,
            fixture.execution,
            &fixture.path,
            "https://github.com/owner/repository.git".into(),
        )
        .await
        .unwrap();
        let server = api(false).await;
        let result = publish(&fixture, &remote, &server).await;
        let task = tasks::get_task(fixture.state.db(), fixture.execution.task)
            .await
            .unwrap()
            .unwrap();
        fixture.cleanup().await;
        assert!(
            matches!(result, PrCreationResult::Created { .. }),
            "{result:?}"
        );
        assert_eq!(task.pr_status.as_deref(), Some("open"));
        assert_eq!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|request| request.method == "POST")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn publication_api_failure_keeps_diagnostics_and_cleans_checkout() {
        let fixture = Fixture::new().await;
        let remote = LocalRemote::new(&fixture);
        let server = api(false).await;
        Mock::given(method("POST"))
            .and(path("/repos/owner/repository/pulls"))
            .respond_with(ResponseTemplate::new(503).set_body_string("API unavailable"))
            .with_priority(1)
            .mount(&server)
            .await;
        let result = publish(&fixture, &remote, &server).await;
        assert!(matches!(result, PrCreationResult::Error(_)), "{result:?}");
        super::super::task::complete_publication(
            &fixture.state,
            fixture.execution,
            "Completed local changes".into(),
            2,
            result,
        )
        .await;
        let run = tasks::get_task_run(fixture.state.db(), fixture.execution.run)
            .await
            .unwrap()
            .unwrap();
        let task = tasks::get_task(fixture.state.db(), fixture.execution.task)
            .await
            .unwrap()
            .unwrap();
        let directory = fixture.path.clone();
        fixture.cleanup().await;
        drop(fixture);
        assert!(!directory.exists(), "failed publication checkout leaked");
        assert_eq!(run.status, "failed");
        assert_eq!(task.status, "blocked");
        assert_eq!(
            run.artifacts.as_ref().unwrap()["summary"],
            "Completed local changes"
        );
        assert_eq!(run.artifacts.as_ref().unwrap()["tool_calls"], 2);
        assert_eq!(
            run.artifacts.as_ref().unwrap()["pr"]["pr_error"],
            run.error_message.unwrap()
        );
    }

    #[tokio::test]
    async fn publication_rejects_changed_identity_or_rewritten_history() {
        for change in ["task branch", "task repository", "origin", "history"] {
            let fixture = Fixture::new().await;
            let remote = LocalRemote::new(&fixture);
            let server = api(false).await;
            match change {
                "task branch" => {
                    sqlx::query("UPDATE tasks SET branch_name='different' WHERE id=$1")
                        .bind(fixture.execution.task)
                        .execute(fixture.state.db())
                        .await
                        .unwrap();
                }
                "task repository" => {
                    sqlx::query("UPDATE tasks SET github_repo_url='https://github.com/other/repository' WHERE id=$1").bind(fixture.execution.task).execute(fixture.state.db()).await.unwrap();
                }
                "origin" => {
                    git(
                        &fixture.path,
                        &[
                            "remote",
                            "set-url",
                            "origin",
                            "https://github.com/other/repository.git",
                        ],
                    );
                }
                "history" => {
                    git(&fixture.path, &["checkout", "--orphan", "replacement"]);
                    git(&fixture.path, &["add", "."]);
                    git(&fixture.path, &["commit", "-m", "rewritten history"]);
                    git(&fixture.path, &["branch", "-M", &fixture.baseline.branch]);
                }
                _ => unreachable!(),
            }
            let result = publish(&fixture, &remote, &server).await;
            fixture.cleanup().await;
            assert!(
                matches!(result, PrCreationResult::Error(_)),
                "{change}: {result:?}"
            );
            assert!(
                server.received_requests().await.unwrap().is_empty(),
                "{change}: contacted GitHub"
            );
        }
    }

    #[tokio::test]
    async fn publication_legacy_sandbox_prepares_and_completes_without_authority() {
        for actor in [false, true] {
            let mut fixture = Fixture::new().await;
            let remote = LocalRemote::new(&fixture);
            let server = api(false).await;
            sqlx::query(
                "UPDATE tasks SET created_by=NULL, branch_name=NULL, pr_status=NULL WHERE id=$1",
            )
            .bind(fixture.execution.task)
            .execute(fixture.state.db())
            .await
            .unwrap();
            if !actor {
                sqlx::query("UPDATE task_runs SET triggered_by=NULL WHERE id=$1")
                    .bind(fixture.execution.run)
                    .execute(fixture.state.db())
                    .await
                    .unwrap();
                fixture.execution.actor = None;
            }
            let mut task = tasks::get_task(fixture.state.db(), fixture.execution.task)
                .await
                .unwrap()
                .unwrap();
            task.project_ids.clear();
            // Reuse the same live execution after dropping its original fixture checkout.
            std::fs::remove_dir_all(&fixture.path).unwrap();
            let checkout = crate::services::checkout::Checkout::prepare(
                fixture.state.db(),
                &task,
                fixture.execution,
            )
            .await
            .unwrap();
            assert!(checkout.path().exists());
            assert!(checkout.baseline().is_none());
            let result = publish(&fixture, &remote, &server).await;
            assert!(matches!(result, PrCreationResult::Sandbox), "{result:?}");
            super::super::task::complete_publication(
                &fixture.state,
                fixture.execution,
                "Sandbox completed".into(),
                0,
                result,
            )
            .await;
            let run = tasks::get_task_run(fixture.state.db(), fixture.execution.run)
                .await
                .unwrap()
                .unwrap();
            let task = tasks::get_task(fixture.state.db(), fixture.execution.task)
                .await
                .unwrap()
                .unwrap();
            let directory = checkout.path().to_path_buf();
            drop(checkout);
            fixture.cleanup().await;
            assert!(!directory.exists());
            assert_eq!(run.status, "completed");
            assert_eq!(task.status, "review");
            assert!(
                task.branch_name.is_none() && task.pr_url.is_none() && task.pr_status.is_none()
            );
            assert_eq!(run.artifacts.unwrap()["pr"]["skipped"], "legacy_sandbox");
            assert!(server.received_requests().await.unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn publication_rejected_push_fails_the_run() {
        let fixture = Fixture::new().await;
        let remote = LocalRemote::new(&fixture);
        let server = api(false).await;
        let branch = GitService::new().generate_branch_name(fixture.execution.task, "Publication");
        let baseline = git(&fixture.path, &["rev-parse", "HEAD"]);
        std::fs::write(fixture.path.join("remote-work"), "preserve").unwrap();
        git(&fixture.path, &["add", "remote-work"]);
        git(&fixture.path, &["commit", "-m", "concurrent remote commit"]);
        git(
            &fixture.path,
            &[
                "push",
                remote.0.to_str().unwrap(),
                &format!("HEAD:refs/heads/{branch}"),
            ],
        );
        let moved = git(&fixture.path, &["rev-parse", "HEAD"]);
        git(&fixture.path, &["reset", "--mixed", &baseline]);
        std::fs::remove_file(fixture.path.join("remote-work")).unwrap();
        let result = publish(&fixture, &remote, &server).await;
        assert!(matches!(result, PrCreationResult::Error(_)), "{result:?}");
        super::super::task::complete_publication(
            &fixture.state,
            fixture.execution,
            "Agent finished".into(),
            3,
            result,
        )
        .await;
        let run = tasks::get_task_run(fixture.state.db(), fixture.execution.run)
            .await
            .unwrap()
            .unwrap();
        let task = tasks::get_task(fixture.state.db(), fixture.execution.task)
            .await
            .unwrap()
            .unwrap();
        let directory = fixture.path.clone();
        fixture.cleanup().await;
        drop(fixture);
        assert!(!directory.exists(), "rejected publication checkout leaked");
        assert_eq!(git(&remote.0, &["rev-parse", &branch]), moved);
        assert_eq!(run.status, "failed");
        assert_eq!(task.status, "blocked");
        assert!(run.error_message.unwrap().contains("push rejected"));
        assert_eq!(run.artifacts.as_ref().unwrap()["summary"], "Agent finished");
        assert_eq!(run.artifacts.as_ref().unwrap()["tool_calls"], 3);
        assert!(
            run.artifacts.as_ref().unwrap()["pr"]["pr_error"]
                .as_str()
                .unwrap()
                .contains("push rejected")
        );
    }
}
