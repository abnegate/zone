//! Per-run workspaces. The guard owns the directory until publication finishes
//! and removes it when execution completes, fails, or is cancelled. Durable run
//! and owner identifiers also let recovery remove local directories after a crash.

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zone_vcs::git::GitService;

use crate::db::{
    projects,
    tasks::{self, TaskRow},
};

const ROOT: &str = "zone-checkouts-v1";

pub struct Repository {
    pub url: String,
    pub token: Option<String>,
}

impl Repository {
    pub async fn resolve(pool: &PgPool, task: &TaskRow) -> Result<Option<Self>, String> {
        let requested = task
            .github_repo_url
            .as_deref()
            .map(GitService::repository_url)
            .transpose()
            .map_err(|error| error.to_string())?;
        let mut repositories = Vec::new();
        for id in &task.project_ids {
            let project = projects::get_project(pool, *id)
                .await
                .map_err(|_| "Cannot load task repository".to_string())?
                .ok_or("Task project is missing")?;
            if project.workspace_id != Some(task.workspace_id) {
                return Err("Task project belongs to another workspace".to_string());
            }
            if let Some(url) = project.github_repo_url {
                let url = GitService::repository_url(&url).map_err(|error| error.to_string())?;
                repositories.push(Self {
                    url,
                    token: project.github_access_token,
                });
            }
        }
        Self::select(requested, repositories)
    }

    fn select(requested: Option<String>, repositories: Vec<Self>) -> Result<Option<Self>, String> {
        if let Some(url) = requested {
            let token = repositories
                .into_iter()
                .find(|repository| repository.url == url)
                .and_then(|repository| repository.token);
            return Ok(Some(Self { url, token }));
        }
        let mut repositories = repositories.into_iter();
        let selected = repositories.next();
        if let Some(selected) = &selected
            && repositories.any(|repository| repository.url != selected.url)
        {
            return Err("Task has multiple repositories; select one explicitly".to_string());
        }
        Ok(selected)
    }
}

/// Immutable identity captured before any task tools can change the repository.
#[derive(Clone, Debug)]
pub struct Baseline {
    pub repository: String,
    pub branch: String,
    pub commit: String,
    pub base: String,
}

impl Baseline {
    pub(crate) async fn prepare(
        pool: &PgPool,
        task: &TaskRow,
        execution: tasks::Execution,
        path: &Path,
        repository: String,
    ) -> Result<Self, String> {
        if !execution
            .authorized(pool, true)
            .await
            .map_err(|_| "Cannot verify checkout authority")?
        {
            return Err("Task checkout lost its execution lease or writer access".into());
        }
        let git = GitService::new();
        let base = git
            .revision(path, "HEAD")
            .await
            .map_err(|error| error.to_string())?;
        let branch = task
            .branch_name
            .clone()
            .unwrap_or_else(|| git.generate_branch_name(task.id, &task.title));
        git.prepare_branch(path, &branch, task.pr_url.is_some())
            .await
            .map_err(|error| error.to_string())?;
        let commit = git
            .revision(path, "HEAD")
            .await
            .map_err(|error| error.to_string())?;
        if !tasks::update_task_branch(pool, &execution, &branch)
            .await
            .map_err(|_| "Cannot record task branch")?
        {
            return Err("Task checkout no longer owns its branch metadata".into());
        }
        Ok(Self {
            repository,
            branch,
            commit,
            base,
        })
    }
}

pub struct Checkout {
    path: PathBuf,
    baseline: Option<Baseline>,
}

impl Checkout {
    pub async fn prepare(
        pool: &PgPool,
        task: &TaskRow,
        execution: tasks::Execution,
    ) -> Result<Self, String> {
        let tasks::Execution { run, owner, .. } = execution;
        let repository = Repository::resolve(pool, task).await?;
        if !execution
            .authorized(pool, false)
            .await
            .map_err(|_| "Cannot verify checkout ownership")?
        {
            return Err("Task execution lost its lease".to_string());
        }
        // The run and owner are durable before a directory can exist. Recovery
        // derives names only from these identifiers, never from a stored path.
        let mut checkout = Self::create(&Self::root(pool), run, owner)
            .map_err(|_| "Cannot create task checkout".to_string())?;
        if let Some(repository) = repository {
            GitService::new()
                .clone_repository(
                    &repository.url,
                    checkout.path(),
                    repository.token.as_deref(),
                )
                .await
                .map_err(|error| error.to_string())?;
            if task.created_by.is_some() {
                checkout.baseline = Some(
                    Baseline::prepare(pool, task, execution, checkout.path(), repository.url)
                        .await?,
                );
            }
        }
        Ok(checkout)
    }

    fn root(pool: &PgPool) -> PathBuf {
        // Recovery may treat absent rows as deleted only within this database.
        // Frame nonsecret connection fields so distinct identities cannot collide
        // through delimiters. Password rotation keeps the same namespace.
        let options = pool.connect_options();
        let port = options.get_port().to_be_bytes();
        let socket = options
            .get_socket()
            .map(|path| path.as_os_str().as_encoded_bytes())
            .unwrap_or_default();
        let mut digest = Sha256::new();
        for field in [
            options.get_host().as_bytes(),
            &port,
            options
                .get_database()
                .unwrap_or(options.get_username())
                .as_bytes(),
            options.get_username().as_bytes(),
            socket,
        ] {
            digest.update((field.len() as u64).to_be_bytes());
            digest.update(field);
        }
        std::env::temp_dir().join(format!("{ROOT}-{}", hex::encode(digest.finalize())))
    }

    fn validate_root(root: &Path) -> std::io::Result<()> {
        let metadata = std::fs::symlink_metadata(root)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(std::io::Error::other("Checkout root is not a directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != nix::unistd::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
                return Err(std::io::Error::other(
                    "Checkout root is not private to this user",
                ));
            }
        }
        Ok(())
    }

    fn directory(path: &Path) -> std::io::Result<()> {
        let mut directory = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory.create(path)
    }

    fn create(root: &Path, run: Uuid, owner: Uuid) -> std::io::Result<Self> {
        match Self::directory(root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        Self::validate_root(root)?;
        let path = root.join(format!("{run}.{owner}"));
        Self::directory(&path)?;
        Ok(Self {
            path,
            baseline: None,
        })
    }

    /// Reap only this host's generated directories whose runs cannot execute.
    /// Failed removals remain discoverable and are retried on the next sweep.
    pub async fn recover(pool: &PgPool) -> Result<u64, String> {
        Self::recover_root(pool, &Self::root(pool)).await
    }

    async fn filesystem<T: Send + 'static>(
        operation: impl FnOnce() -> std::io::Result<T> + Send + 'static,
    ) -> Result<T, String> {
        tokio::task::spawn_blocking(operation)
            .await
            .map_err(|_| "Checkout filesystem worker stopped".to_string())?
            .map_err(|error| format!("Checkout filesystem operation failed ({:?})", error.kind()))
    }

    fn entries(root: &Path) -> std::io::Result<Vec<(Uuid, PathBuf)>> {
        match Self::validate_root(root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        }
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some((run, owner)) = name.split_once('.') else {
                continue;
            };
            let (Ok(run_id), Ok(owner_id)) = (Uuid::parse_str(run), Uuid::parse_str(owner)) else {
                continue;
            };
            if run_id.to_string() == run && owner_id.to_string() == owner {
                entries.push((run_id, entry.path()));
            }
        }
        Ok(entries)
    }

    async fn recover_root(pool: &PgPool, root: &Path) -> Result<u64, String> {
        let directory = root.to_path_buf();
        let entries = Self::filesystem(move || Self::entries(&directory)).await?;
        let mut removed = 0;
        for (run, path) in entries {
            let status: Option<String> =
                sqlx::query_scalar("SELECT status FROM task_runs WHERE id = $1")
                    .bind(run)
                    .fetch_optional(pool)
                    .await
                    .map_err(|_| "Cannot inspect task checkout ownership".to_string())?;
            // Deleted tasks cascade to runs; their generated paths are reclaimable.
            if status
                .as_deref()
                .is_some_and(|status| !matches!(status, "completed" | "failed" | "cancelled"))
            {
                continue;
            }
            let directory = root.to_path_buf();
            let result = Self::filesystem(move || {
                Self::validate_root(&directory)?;
                let kind = match std::fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata.file_type(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                    Err(error) => return Err(error),
                };
                let result = if kind.is_symlink() {
                    std::fs::remove_file(&path)
                } else if kind.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    return Ok(false);
                };
                match result {
                    Ok(()) => Ok(true),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
                    Err(error) => Err(error),
                }
            })
            .await;
            match result {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(error) => {
                    tracing::error!(%run, %error, "Failed to recover task checkout; will retry")
                }
            }
        }
        Ok(removed)
    }

    pub fn baseline(&self) -> Option<&Baseline> {
        self.baseline.as_ref()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let Some(root) = self.path.parent() else {
            return;
        };
        if Self::validate_root(root).is_err() {
            tracing::error!(path = %root.display(), "Refusing cleanup through an unsafe checkout root");
            return;
        }
        if let Err(error) = std::fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::error!(path = %self.path.display(), kind = ?error.kind(), "Failed to remove task checkout");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recovery_filesystem_does_not_block_the_async_executor() {
        let started = std::sync::Arc::new(tokio::sync::Notify::new());
        let notified = started.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        let responder = tokio::spawn(async move {
            notified.notified().await;
            let _ = sender.send(());
        });
        let responsive = Checkout::filesystem(move || {
            started.notify_one();
            Ok(receiver
                .recv_timeout(std::time::Duration::from_secs(1))
                .is_ok())
        })
        .await
        .unwrap();
        responder.await.unwrap();
        assert!(responsive, "filesystem work blocked the async executor");
    }

    #[tokio::test]
    async fn recovery_removes_abandoned_checkout() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .unwrap();
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations(id,name,slug) VALUES($1,'Checkout recovery',$1::text)",
        )
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Checkout recovery',$1::text)").bind(workspace).bind(organization).execute(&pool).await.unwrap();
        let task = crate::db::tasks::create_task(
            &pool,
            workspace,
            &[],
            "Recover checkout",
            "Regression",
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
        let run = crate::db::tasks::create_task_run(&pool, task.id)
            .await
            .unwrap();
        let owner = Uuid::new_v4();
        assert!(
            crate::db::tasks::claim_task_run(&pool, run.id, owner)
                .await
                .unwrap()
        );
        let checkout = Checkout::create(&Checkout::root(&pool), run.id, owner).unwrap();
        let path = checkout.path().to_path_buf();
        std::fs::write(path.join("private.txt"), "Private repository content").unwrap();
        std::mem::forget(checkout);
        crate::db::tasks::complete_owned_task_run(
            &pool,
            run.id,
            Some(owner),
            "failed",
            Some("orphaned"),
            None,
        )
        .await
        .unwrap();
        let state = crate::state::AppState::new(
            crate::state::AppState::for_tests().config().clone(),
            pool.clone(),
            None,
        );
        let recovery = crate::workers::task::spawn_recovery(state);
        let recovered = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while path.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();
        recovery.abort();
        let _ = recovery.await;
        if path.exists() {
            std::fs::remove_dir_all(&path).unwrap();
        }
        sqlx::query("DELETE FROM organizations WHERE id=$1")
            .bind(organization)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            recovered,
            "production recovery must remove a checkout when Drop never ran"
        );
    }

    #[tokio::test]
    async fn recovery_cannot_remove_another_databases_live_checkout() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .unwrap();
        // Database identifiers contain only this prefix and UUID hexadecimal.
        let database = format!("checkout_{}", Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
            .execute(&pool)
            .await
            .unwrap();
        let other = PgPool::connect_with((*pool.connect_options()).clone().database(&database))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE task_runs(id uuid PRIMARY KEY, status text NOT NULL)")
            .execute(&other)
            .await
            .unwrap();
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations(id,name,slug) VALUES($1,'Checkout database isolation',$1::text)").bind(organization).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Checkout isolation',$1::text)").bind(workspace).bind(organization).execute(&pool).await.unwrap();
        let task = tasks::create_task(
            &pool,
            workspace,
            &[],
            "Isolated checkout",
            "Regression",
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
        let run = tasks::create_task_run(&pool, task.id).await.unwrap();
        let owner = Uuid::new_v4();
        assert!(tasks::claim_task_run(&pool, run.id, owner).await.unwrap());
        let checkout = Checkout::prepare(
            &pool,
            &task,
            tasks::Execution {
                task: task.id,
                run: run.id,
                owner,
                actor: None,
            },
        )
        .await
        .unwrap();
        std::fs::write(checkout.path().join("keep.txt"), "Live private content").unwrap();
        let removed = Checkout::recover(&other).await.unwrap();
        let preserved = checkout.path().join("keep.txt").exists();
        drop(checkout);
        sqlx::query("DELETE FROM organizations WHERE id=$1")
            .bind(organization)
            .execute(&pool)
            .await
            .unwrap();
        other.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {database}")))
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            preserved,
            "another database recovery deleted this database's live checkout"
        );
        assert_eq!(removed, 0);
    }

    #[test]
    fn empty_checkout_exists_and_is_isolated_for_each_run() {
        let run = Uuid::new_v4();
        let root = std::env::temp_dir().join(format!("zone-checkout-test-{}", Uuid::new_v4()));
        let first = Checkout::create(&root, run, Uuid::new_v4()).unwrap();
        let second = Checkout::create(&root, run, Uuid::new_v4()).unwrap();
        assert!(first.path().is_dir());
        assert!(second.path().is_dir());
        assert_ne!(first.path(), second.path());
        std::fs::write(first.path().join("sentinel"), "first").unwrap();
        assert!(!second.path().join("sentinel").exists());
        let path = first.path().to_path_buf();
        drop(first);
        assert!(!path.exists());
        assert!(second.path().exists());
        drop(second);
        std::fs::remove_dir(root).unwrap();
    }

    #[tokio::test]
    async fn recovery_preserves_active_and_unrelated_paths() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .unwrap();
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations(id,name,slug) VALUES($1,'Checkout safety',$1::text)",
        )
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Checkout safety',$1::text)").bind(workspace).bind(organization).execute(&pool).await.unwrap();
        let task = tasks::create_task(
            &pool,
            workspace,
            &[],
            "Active checkout",
            "Regression",
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
        let run = tasks::create_task_run(&pool, task.id).await.unwrap();
        let root = std::env::temp_dir().join(format!("zone-checkout-test-{}", Uuid::new_v4()));
        Checkout::directory(&root).unwrap();
        let live = root.join(format!("{}.{}", run.id, Uuid::new_v4()));
        Checkout::directory(&live).unwrap();
        let unrelated = root.join("unrelated");
        Checkout::directory(&unrelated).unwrap();
        std::fs::write(unrelated.join("keep.txt"), "Unrelated content").unwrap();
        assert_eq!(Checkout::recover_root(&pool, &root).await.unwrap(), 0);
        assert!(live.exists());
        assert!(unrelated.join("keep.txt").exists());
        tasks::complete_task_run(&pool, run.id, "completed", None, None)
            .await
            .unwrap();
        assert_eq!(Checkout::recover_root(&pool, &root).await.unwrap(), 1);
        assert!(!live.exists());
        assert!(unrelated.join("keep.txt").exists());
        assert_eq!(Checkout::recover_root(&pool, &root).await.unwrap(), 0);
        std::fs::remove_dir_all(root).unwrap();
        sqlx::query("DELETE FROM organizations WHERE id=$1")
            .bind(organization)
            .execute(&pool)
            .await
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn recovery_refuses_root_symlinks_and_never_follows_entry_links() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .unwrap();
        let root = std::env::temp_dir().join(format!("zone-checkout-test-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("zone-checkout-outside-{}", Uuid::new_v4()));
        let alias = std::env::temp_dir().join(format!("zone-checkout-alias-{}", Uuid::new_v4()));
        Checkout::directory(&root).unwrap();
        Checkout::directory(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), "Outside content").unwrap();
        symlink(&outside, &alias).unwrap();
        assert!(Checkout::recover_root(&pool, &alias).await.is_err());
        let link = root.join(format!("{}.{}", Uuid::new_v4(), Uuid::new_v4()));
        symlink(&outside, &link).unwrap();
        assert_eq!(Checkout::recover_root(&pool, &root).await.unwrap(), 1);
        assert!(!link.exists());
        assert!(outside.join("keep.txt").exists());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Checkout::recover_root(&pool, &root).await.is_err());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Root bypasses Unix mode bits, so permission-failure injection only
        // applies to the unprivileged runtime used by the server image.
        if !nix::unistd::geteuid().is_root() {
            let pending = root.join(format!("{}.{}", Uuid::new_v4(), Uuid::new_v4()));
            Checkout::directory(&pending).unwrap();
            std::fs::write(pending.join("private.txt"), "Retry content").unwrap();
            std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o000)).unwrap();
            assert_eq!(Checkout::recover_root(&pool, &root).await.unwrap(), 0);
            assert!(pending.exists());
            std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(Checkout::recover_root(&pool, &root).await.unwrap(), 1);
            assert!(!pending.exists());
        }
        std::fs::remove_file(alias).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn credentials_are_never_used_for_another_repository() {
        let repository = Repository::select(
            Some("https://github.com/owner/requested.git".to_string()),
            vec![Repository {
                url: "https://github.com/owner/other.git".to_string(),
                token: Some("secret".to_string()),
            }],
        )
        .unwrap()
        .unwrap();
        assert!(repository.token.is_none());
    }

    #[test]
    fn multiple_project_repositories_require_an_explicit_selection() {
        assert!(
            Repository::select(
                None,
                vec![
                    Repository {
                        url: "https://github.com/owner/first.git".to_string(),
                        token: None
                    },
                    Repository {
                        url: "https://github.com/owner/second.git".to_string(),
                        token: None
                    }
                ]
            )
            .is_err()
        );
    }
}
