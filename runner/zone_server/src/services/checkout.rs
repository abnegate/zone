//! Per-run workspaces. The guard owns the directory until publication finishes
//! and removes it when execution completes, fails, or is cancelled. Durable run
//! and owner identifiers also let recovery remove local directories after a crash.
//!
//! A run with a repository works in a worktree rather than a clone of its own:
//! one base clone per repository lives under `repos/` in the checkout root,
//! fetched before each run, and the run's directory is a detached worktree of
//! it, so a run costs a fetch and a checkout instead of a clone and the
//! history is shared. What that changes about cleanup is the rule: a worktree
//! is removed when it holds nothing nobody else has, and kept while it holds
//! changes no commit has or commits no remote has, because removing those
//! would be the one way a run's work could vanish without anyone having
//! chosen that (CC 349-410, 1487-1640; CX 19). Only recovery removes a
//! worktree, once no run works in its base clone any more: removing one
//! means restoring the base's shared configuration first, which a sibling
//! run still working there may rely on.
//!
//! What the runs share is not trusted, since a run's own git commands reach
//! it: the base clone's configuration is rewritten from what Zone knows
//! before every use, the fetch is bound to the URL Zone knows rather than to
//! the remote the clone names, a run starts from the commit the remote
//! reports for its default branch rather than from `origin/HEAD`, and
//! readying a base is serialised across runs by a file lock beside it. The
//! same goes for the removal rule: whether a worktree's HEAD is published is
//! decided from what the service recorded about the run — the commit it
//! started on and the commit the service pushed — never from refs in the
//! clone, which a run could move to make its commits look published.

use abnegate_secret::MasterKey;
use abnegate_secret::SecretValue;
use abnegate_vcs::Checkout as GitCheckout;
use abnegate_vcs::worktree;
use abnegate_vcs::{BranchFormat, BranchName, CommitSha, GitError, GitService, RepositoryUrl};
use nix::fcntl::{Flock, FlockArg};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::db::{
    projects,
    tasks::{self, TaskRow},
};
use crate::secret::Unseal;

const ROOT: &str = "zone-checkouts-v1";

const AUTHOR_NAME: &str = "Zone";

const AUTHOR_EMAIL: &str = "zone@localhost";

const BRANCH_PREFIX: &str = "zone/task";

pub(crate) const MISMATCHED_ORIGIN: &str =
    "Checkout repository no longer matches the task repository";

/// Git as a task run uses it: commits made as Zone, and branches named
/// `zone/task-{short id}-{slug}`.
pub fn git() -> GitService {
    GitService::new()
        .with_author(AUTHOR_NAME, AUTHOR_EMAIL)
        .with_branch_prefix(
            BranchName::parse(BRANCH_PREFIX).expect("the branch prefix is a branch name"),
        )
        .with_branch_format(BranchFormat::Joined)
}

/// Put a base clone's shared configuration back to what Zone knows: its
/// origin and the settings git chose for the clone, nothing a run wrote.
///
/// A run's tools share the base's configuration with every other run of the
/// repository and with the host, and they write it as a matter of course:
/// `npm install` with husky sets `core.hooksPath`, `git submodule update`
/// adds `submodule.*`, an agent sets `user.email`. The host's git refuses a
/// repository configured beyond a clone, so before the host touches a run's
/// worktree again the configuration is rewritten, under the base's lock so
/// no run readies the base meanwhile. A base whose origin is no longer
/// `origin` is refused rather than rewritten: what the run's commits were
/// meant for is then no longer certain.
pub async fn sanitize(base: &Path, origin: &RepositoryUrl) -> Result<(), String> {
    let _lock = Checkout::lock_base(base).await?;
    restore(base, origin).await
}

/// [`sanitize`]'s rewrite, for a caller already holding the base's lock.
async fn restore(base: &Path, origin: &RepositoryUrl) -> Result<(), String> {
    let file = base.join(".git").join("config");
    match Checkout::filesystem(move || Ok(GitService::configured_origin(&file))).await? {
        Ok(Some(configured)) if configured == origin.as_str() => {}
        Ok(_) | Err(GitError::UnsafeConfig(_)) => return Err(MISMATCHED_ORIGIN.to_string()),
        Err(error) => return Err(error.to_string()),
    }
    git()
        .reset_config(base, origin)
        .await
        .map_err(|error| error.to_string())
}

pub struct Repository {
    pub url: RepositoryUrl,
    pub token: Option<SecretValue>,
}

impl Repository {
    /// The repository a task works in, its token opened from how the project
    /// stores it (encrypted at rest, or plain from before it was).
    pub async fn resolve(
        pool: &PgPool,
        key: &MasterKey,
        task: &TaskRow,
    ) -> Result<Option<Self>, String> {
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
                    token: project
                        .github_access_token
                        .map(|stored| key.open_lenient(&stored)),
                });
            }
        }
        Self::select(requested, repositories)
    }

    fn select(
        requested: Option<RepositoryUrl>,
        repositories: Vec<Self>,
    ) -> Result<Option<Self>, String> {
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
    pub worktree: GitCheckout,
    pub repository: RepositoryUrl,
    pub branch: BranchName,
    pub commit: CommitSha,
    pub base: CommitSha,
}

impl Baseline {
    pub(crate) async fn prepare(
        pool: &PgPool,
        task: &TaskRow,
        execution: tasks::Execution,
        worktree: &GitCheckout,
        repository: RepositoryUrl,
    ) -> Result<Self, String> {
        if !execution
            .authorized(pool, true)
            .await
            .map_err(|_| "Cannot verify checkout authority")?
        {
            return Err("Task checkout lost its execution lease or writer access".into());
        }
        let git = git();
        let base = git
            .revision(worktree, "HEAD")
            .await
            .map_err(|error| error.to_string())?;
        let branch = match task.branch_name.as_deref() {
            Some(name) => BranchName::parse(name).map_err(|error| error.to_string())?,
            None => git
                .generate_branch_name(task.id, &task.title)
                .map_err(|error| error.to_string())?,
        };
        git.prepare_branch(worktree, &branch, task.pr_url.is_some())
            .await
            .map_err(|error| error.to_string())?;
        let commit = git
            .revision(worktree, "HEAD")
            .await
            .map_err(|error| error.to_string())?;
        if !tasks::update_task_branch(pool, &execution, branch.as_str())
            .await
            .map_err(|_| "Cannot record task branch")?
        {
            return Err("Task checkout no longer owns its branch metadata".into());
        }
        Ok(Self {
            worktree: worktree.clone(),
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
    /// This directory as a worktree of its repository's base clone, once it
    /// is one. A run with no repository has none, and its directory is
    /// removed as a plain directory is.
    worktree: Option<GitCheckout>,
    /// Whether [`Checkout::finish`] already ended the checkout, leaving
    /// `Drop` nothing to do.
    finished: bool,
}

impl Checkout {
    pub async fn prepare(
        pool: &PgPool,
        key: &MasterKey,
        task: &TaskRow,
        execution: tasks::Execution,
    ) -> Result<Self, String> {
        let tasks::Execution { run, owner, .. } = execution;
        let repository = Repository::resolve(pool, key, task).await?;
        if !execution
            .authorized(pool, false)
            .await
            .map_err(|_| "Cannot verify checkout ownership")?
        {
            return Err("Task execution lost its lease".to_string());
        }
        // The run and owner are durable before a directory can exist. Recovery
        // derives names only from these identifiers, never from a stored path.
        let root = Self::root(pool);
        let mut checkout = Self::create(&root, run, owner)
            .map_err(|_| "Cannot create task checkout".to_string())?;
        if let Some(repository) = repository {
            let base = Self::base(&root, repository.url.as_str())
                .map_err(|_| "Cannot create the repository's base clone".to_string())?;
            // One run of a repository readies its base at a time, from checking
            // the clone to adding the worktree, so two first runs cannot clone
            // into one directory and nothing lands between a run's fetch and
            // its checkout.
            let lock = Self::lock_base(&base).await?;
            let start = Self::refresh(&base, &repository).await?;
            let (from, at) = (base.clone(), checkout.path().to_path_buf());
            let worktree =
                Self::filesystem(move || worktree::add(&from, &at, start.as_str())).await?;
            drop(lock);
            checkout.worktree = Some(worktree.clone());
            if task.created_by.is_some() {
                checkout.baseline = Some(
                    Baseline::prepare(pool, task, execution, &worktree, repository.url).await?,
                );
            }
            // The commit the run starts on is recorded by the service, under
            // the run's lease, before any tool of the run can move HEAD: it
            // is what recovery will trust when it asks whether the worktree
            // may go.
            let head = git()
                .revision(&worktree, "HEAD")
                .await
                .map_err(|error| error.to_string())?;
            if !tasks::record_run_checkout(pool, run, owner, head.as_str())
                .await
                .map_err(|_| "Cannot record the run's checkout")?
            {
                return Err("Task checkout lost its lease before its start was recorded".into());
            }
        }
        Ok(checkout)
    }

    /// The base clone a repository's runs are worktrees of, under the same
    /// private root as the runs. Named by a digest of the URL, so two
    /// repositories cannot share one and a URL never becomes a path.
    fn base(root: &Path, url: &str) -> std::io::Result<PathBuf> {
        for directory in [root.to_path_buf(), root.join("repos")] {
            match Self::directory(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
            Self::validate_root(&directory)?;
        }
        let base = root
            .join("repos")
            .join(hex::encode(Sha256::digest(url.as_bytes())));
        match Self::directory(&base) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        Self::validate_root(&base)?;
        Ok(base)
    }

    /// An exclusive lock on the base clone, held by whoever readies it. It is
    /// a file lock beside the clone, so it is shared by every process that
    /// shares the checkout root and released with the process if one dies
    /// holding it.
    pub(crate) async fn lock_base(base: &Path) -> Result<Flock<std::fs::File>, String> {
        let base = base.to_path_buf();
        Self::filesystem(move || {
            Flock::lock(Self::open_lock(&base)?, FlockArg::LockExclusive)
                .map_err(|(_, errno)| std::io::Error::from(errno))
        })
        .await
    }

    /// The file beside a base clone that its lock is taken on.
    fn open_lock(base: &Path) -> std::io::Result<std::fs::File> {
        let mut options = std::fs::OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(base.with_extension("lock"))
    }

    /// Bring the base clone to the repository's current state and say which
    /// commit a run starts from. The clone is made the first time and fetched
    /// after that; a directory a crashed clone left without a `.git` is
    /// emptied and cloned again rather than failing every run after it. What
    /// the runs share is not trusted: the clone's configuration is rewritten
    /// from what Zone knows before any command reads it, the fetch is bound to
    /// the URL Zone knows rather than to the remote the clone names, and the
    /// commit a run starts from is the one the remote reports for its default
    /// branch, never `origin/HEAD`, which a run's git commands share.
    async fn refresh(base: &Path, repository: &Repository) -> Result<CommitSha, String> {
        let git = git();
        let token = repository.token.as_ref();
        let fresh = !base.join(".git").is_dir();
        if fresh {
            let directory = base.to_path_buf();
            Self::filesystem(move || {
                for entry in std::fs::read_dir(&directory)? {
                    let entry = entry?;
                    if entry.file_type()?.is_dir() {
                        std::fs::remove_dir_all(entry.path())?;
                    } else {
                        std::fs::remove_file(entry.path())?;
                    }
                }
                Ok(())
            })
            .await?;
            git.clone_repository(&repository.url, base, token)
                .await
                .map_err(|error| error.to_string())?;
        }
        git.reset_config(base, &repository.url)
            .await
            .map_err(|error| error.to_string())?;
        let head = git
            .remote_head(base, &repository.url, token)
            .await
            .map_err(|error| error.to_string())?;
        if !fresh {
            git.fetch(base, &repository.url, token)
                .await
                .map_err(|error| error.to_string())?;
        }
        if !git
            .has_commit(&GitCheckout::base(base), &head.commit)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err(
                "The repository's default branch moved while it was being fetched; run the task again"
                    .to_string(),
            );
        }
        git.set_remote_head(base, &head.branch)
            .await
            .map_err(|error| error.to_string())?;
        Ok(head.commit)
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

    pub(crate) fn validate_root(root: &Path) -> std::io::Result<()> {
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

    pub(crate) fn directory(path: &Path) -> std::io::Result<()> {
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
            worktree: None,
            finished: false,
        })
    }

    /// Reap only this host's generated directories whose runs cannot execute.
    /// Failed removals remain discoverable and are retried on the next sweep.
    pub async fn recover(pool: &PgPool) -> Result<u64, String> {
        Self::recover_root(pool, &Self::root(pool)).await
    }

    pub(crate) async fn filesystem<T: Send + 'static>(
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
        let mut finished = Vec::new();
        let mut live = HashSet::new();
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
                // A base a live run works in is not sanitized under it: the
                // run's own tools may have configured it on purpose.
                let (directory, at) = (root.to_path_buf(), path.clone());
                if let Ok(Some(base)) =
                    Self::filesystem(move || Self::base_of(&directory, &at)).await
                {
                    live.insert(base);
                }
                continue;
            }
            finished.push((run, path));
        }
        let mut removed = 0;
        for (run, path) in finished {
            // What the service recorded about the run's worktree, read here
            // and not from the clone: a run whose start was never recorded
            // has nothing anyone can vouch for, and its worktree is kept.
            let known: Vec<String> = tasks::run_checkout(pool, run)
                .await
                .map_err(|_| "Cannot inspect task checkout facts".to_string())?
                .map(|checkout| {
                    std::iter::once(checkout.head)
                        .chain(checkout.published)
                        .collect()
                })
                .unwrap_or_default();
            match Self::recover_path(root, &path, known, &live).await {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(error) => {
                    tracing::error!(%run, %error, "Failed to recover task checkout; will retry")
                }
            }
        }
        Ok(removed)
    }

    /// Reclaim one finished run's directory. A plain directory is removed. A
    /// worktree is reclaimed once its base clone's configuration is put back
    /// to what Zone knows, but not while another run still works in that base,
    /// whose configuration that run's tools may have written on purpose: a
    /// later sweep takes it. A worktree that names no base under `root` is
    /// left and said so; one whose base cannot be read, trusted or bound is
    /// an error, which the sweep logs and retries.
    async fn recover_path(
        root: &Path,
        path: &Path,
        known: Vec<String>,
        live: &HashSet<PathBuf>,
    ) -> Result<bool, String> {
        let (directory, at) = (root.to_path_buf(), path.to_path_buf());
        let base = Self::detailed(move || {
            Self::validate_root(&directory)?;
            let worktree = std::fs::symlink_metadata(&at).is_ok_and(|metadata| metadata.is_dir())
                && worktree::is_worktree(&at);
            match worktree {
                true => Self::base_of(&directory, &at).map(|base| Some(base.ok_or(at))),
                false => Ok(None),
            }
        })
        .await?;
        let base = match base {
            None => {
                let (directory, at) = (root.to_path_buf(), path.to_path_buf());
                return Self::detailed(move || Self::remove_entry(&directory, &at)).await;
            }
            Some(Err(at)) => {
                tracing::warn!(path = %at.display(), "A finished run's worktree names no base clone under this root; leaving it");
                return Ok(false);
            }
            Some(Ok(base)) if live.contains(&base) => {
                tracing::debug!(path = %path.display(), "Another run still works in this worktree's base clone; reclaiming it later");
                return Ok(false);
            }
            Some(Ok(base)) => base,
        };
        let origin = {
            let base = base.clone();
            Self::detailed(move || Self::origin_of(&base)).await?
        };
        sanitize(&base, &origin).await?;
        let (directory, at) = (root.to_path_buf(), path.to_path_buf());
        Self::detailed(move || match Self::bound(&directory, &at)? {
            Some(worktree) => Self::reclaim_worktree(&worktree, &known),
            None => Ok(false),
        })
        .await
    }

    /// Remove a finished run's plain directory, or a link standing in its
    /// place, never following the link.
    fn remove_entry(root: &Path, path: &Path) -> std::io::Result<bool> {
        Self::validate_root(root)?;
        let kind = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata.file_type(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let result = if kind.is_symlink() {
            std::fs::remove_file(path)
        } else if kind.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            return Ok(false);
        };
        match result {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// [`Self::filesystem`] for recovery, keeping what went wrong: its errors
    /// reach only the operator's log.
    async fn detailed<T: Send + 'static>(
        operation: impl FnOnce() -> std::io::Result<T> + Send + 'static,
    ) -> Result<T, String> {
        tokio::task::spawn_blocking(operation)
            .await
            .map_err(|_| "Checkout filesystem worker stopped".to_string())?
            .map_err(|error| error.to_string())
    }

    /// Remove a finished run's worktree when nothing in it is lost, and leave
    /// it — saying so — while it still holds work nobody else has. `known`
    /// are the commits the service recorded for the run; a HEAD that is none
    /// of them is unpublished work. A worktree git cannot read is left too:
    /// recovery reclaims what it can prove is reclaimable, and a directory
    /// it cannot read is not that.
    fn reclaim_worktree(worktree: &GitCheckout, known: &[String]) -> std::io::Result<bool> {
        let path = worktree.top();
        let known: Vec<&str> = known.iter().map(String::as_str).collect();
        match worktree::unfinished(worktree, &known) {
            Ok(unfinished) if unfinished.any() => {
                tracing::info!(
                    path = %path.display(),
                    uncommitted = unfinished.uncommitted,
                    unpublished = unfinished.unpublished,
                    "Leaving a finished run's worktree that still holds work nobody else has"
                );
                Ok(false)
            }
            Ok(_) => {
                worktree::remove(worktree)?;
                Ok(true)
            }
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "Cannot read a finished run's worktree; leaving it");
                Ok(false)
            }
        }
    }

    /// The worktree at `path`, bound to the base clone under `root` its
    /// `.git` file names, once that clone admits it as one of its own
    /// worktrees. A worktree that names no base here is `None`; a base that
    /// refuses it, or cannot be read, is the error it gave.
    fn bound(root: &Path, path: &Path) -> std::io::Result<Option<GitCheckout>> {
        let Some(base) = Self::base_of(root, path)? else {
            return Ok(None);
        };
        let checkout = GitCheckout::linked(path, base);
        worktree::repository_of(&checkout)?;
        Ok(Some(checkout))
    }

    /// The base clone under `root/repos` whose worktree record the `.git`
    /// file at `path` points into: `gitdir: <base>/.git/worktrees/<name>`.
    /// Only the directory it names is asked about, and only when that sits
    /// directly under `root/repos`.
    fn base_of(root: &Path, path: &Path) -> std::io::Result<Option<PathBuf>> {
        let marker = path.join(".git");
        match std::fs::symlink_metadata(&marker) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        }
        let content = std::fs::read_to_string(&marker)?;
        let Some(record) = content.trim_end().strip_prefix("gitdir: ") else {
            return Ok(None);
        };
        let record = path.join(record);
        let Some(directory) = record
            .parent()
            .filter(|records| records.file_name() == Some(std::ffi::OsStr::new("worktrees")))
            .and_then(Path::parent)
            .filter(|directory| directory.file_name() == Some(std::ffi::OsStr::new(".git")))
        else {
            return Ok(None);
        };
        let (Some(base), Some(name)) = (
            directory.parent(),
            directory.parent().and_then(Path::file_name),
        ) else {
            return Ok(None);
        };
        let repositories = root.join("repos");
        match base.parent().map(std::fs::canonicalize) {
            Some(Ok(parent)) if parent == std::fs::canonicalize(&repositories)? => {
                Ok(Some(repositories.join(name)))
            }
            Some(Err(error)) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(None),
        }
    }

    /// The repository a base clone was made for, read from its configuration
    /// and trusted only when it is the one the base is named after: a run can
    /// rewrite the configuration, but not the digest its base was named by.
    fn origin_of(base: &Path) -> std::io::Result<RepositoryUrl> {
        let named = base.file_name().and_then(std::ffi::OsStr::to_str);
        GitService::configured_origin(&base.join(".git").join("config"))
            .map_err(std::io::Error::other)?
            .filter(|url| named == Some(hex::encode(Sha256::digest(url.as_bytes())).as_str()))
            .and_then(|url| RepositoryUrl::parse(&url).ok())
            .ok_or_else(|| {
                std::io::Error::other(
                    "The base clone's origin is not the repository it was made for",
                )
            })
    }

    /// End the run's checkout. A plain directory is removed. A worktree is
    /// left to recovery, the one path that knows whether another run still
    /// works in its base clone: removing it means restoring the base's shared
    /// configuration first, which would undo what a sibling run's tools
    /// wrote there while that run still relies on it.
    pub async fn finish(mut self) {
        self.finished = true;
        if self.worktree.is_some() || !self.root_is_safe() {
            return;
        }
        let path = self.path.clone();
        if Self::filesystem(move || {
            Self::release(&path);
            Ok(())
        })
        .await
        .is_err()
        {
            tracing::error!(path = %self.path.display(), "The checkout's cleanup stopped");
        }
    }

    /// Whether the checkout's root is still the private directory it was
    /// made in; cleanup goes through nothing else.
    fn root_is_safe(&self) -> bool {
        match self.path.parent() {
            Some(root) if Self::validate_root(root).is_ok() => true,
            _ => {
                tracing::error!(path = %self.path.display(), "Refusing cleanup through an unsafe checkout root");
                false
            }
        }
    }

    /// Remove a run's plain directory.
    fn release(path: &Path) {
        if let Err(error) = std::fs::remove_dir_all(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::error!(path = %path.display(), kind = ?error.kind(), "Failed to remove task checkout");
        }
    }

    pub fn baseline(&self) -> Option<&Baseline> {
        self.baseline.as_ref()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Checkout {
    /// The fallback for a run that never reached [`Checkout::finish`]: it
    /// panicked or was cancelled. It does what `finish` does without waiting
    /// on anything: a plain directory is removed and a worktree is left to
    /// recovery.
    fn drop(&mut self) {
        if self.finished || self.worktree.is_some() || !self.root_is_safe() {
            return;
        }
        Self::release(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(source: &str) -> RepositoryUrl {
        RepositoryUrl::parse(source).unwrap()
    }

    /// Run git in a fixture with a fixed identity, panicking on failure.
    fn git(path: &Path, arguments: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.test")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.test")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(arguments)
            .current_dir(path)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// The repository a fixture's base clone is named for.
    const FIXTURE_ORIGIN: &str = "https://github.com/fixture/repository.git";

    /// Hooks that leave `sentinel` behind if git ever runs them, and the
    /// settings a run's own tools write into the configuration it shares
    /// with the host: husky's hooks path, a submodule and an identity.
    fn write_shared_configuration(checkout: &Path, sentinel: &Path) {
        let hooks = sentinel.with_extension("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        for hook in [
            "pre-commit",
            "post-checkout",
            "reference-transaction",
            "pre-push",
        ] {
            let script = hooks.join(hook);
            std::fs::write(
                &script,
                format!("#!/bin/sh\ntouch '{}'\n", sentinel.display()),
            )
            .unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        git(
            checkout,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );
        git(
            checkout,
            &[
                "config",
                "submodule.vendor.url",
                "https://github.com/fixture/vendor.git",
            ],
        );
        git(checkout, &["config", "user.email", "agent@example.test"]);
    }

    /// A remote with one commit and a base clone of it under `root/repos`,
    /// laid out the way `prepare` lays them out, without the network.
    fn base_clone(root: &Path) -> PathBuf {
        let remote = root.join("remote");
        std::fs::create_dir(&remote).unwrap();
        git(&remote, &["init", "-q", "-b", "main"]);
        std::fs::write(remote.join("README"), "fixture\n").unwrap();
        git(&remote, &["add", "README"]);
        git(&remote, &["commit", "-q", "-m", "fixture"]);
        let base = Checkout::base(root, FIXTURE_ORIGIN).unwrap();
        git(
            root,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                base.to_str().unwrap(),
            ],
        );
        git(&base, &["remote", "set-url", "origin", FIXTURE_ORIGIN]);
        base
    }

    /// A run's worktree of `base`, as `prepare` makes one, on a branch of its
    /// own the way `Baseline::prepare` leaves it.
    fn run_worktree(root: &Path, base: &Path, branch: &str) -> Checkout {
        let mut checkout = Checkout::create(root, Uuid::new_v4(), Uuid::new_v4()).unwrap();
        checkout.worktree = Some(worktree::add(base, checkout.path(), "origin/HEAD").unwrap());
        git(checkout.path(), &["checkout", "-q", "-b", branch]);
        checkout
    }

    /// A private root of the shape production makes: a temporary directory
    /// is made with the process umask, which is not the 0700 the root demands.
    fn private_root() -> (tempfile::TempDir, PathBuf) {
        let holder = tempfile::tempdir().unwrap();
        let root = holder.path().join("root");
        Checkout::directory(&root).unwrap();
        (holder, root)
    }

    /// Readying a base is exclusive: a second locker is refused while the
    /// first holds it and admitted once the first lets go.
    #[tokio::test]
    async fn readying_a_base_clone_is_exclusive_across_lockers() {
        let (_holder, root) = private_root();
        let base = Checkout::base(&root, "https://github.com/fixture/locked.git").unwrap();
        let held = Checkout::lock_base(&base).await.unwrap();
        let lock = base.with_extension("lock");
        let attempt = |lock: PathBuf| {
            tokio::task::spawn_blocking(move || {
                let file = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&lock)
                    .unwrap();
                Flock::lock(file, FlockArg::LockExclusiveNonblock).is_ok()
            })
        };
        assert!(
            !attempt(lock.clone()).await.unwrap(),
            "a second locker is refused while the first holds the base"
        );
        drop(held);
        assert!(attempt(lock).await.unwrap(), "and admitted once it lets go");
    }

    /// A finished run's worktree is reclaimed when it holds nothing nobody
    /// else has, and kept when it does -- a file no commit holds, or a commit
    /// the service never pushed -- because removing it is the one way a run's
    /// work could vanish without anyone choosing that. What the service
    /// pushed is what it recorded, not what the clone's refs say: a
    /// remote-tracking ref the run planted proves nothing.
    #[test]
    fn a_worktree_is_reclaimed_only_when_it_holds_nothing_nobody_else_has() {
        let (_holder, root) = private_root();
        let root = root.as_path();
        let base = base_clone(root);
        let start = git(&base, &["rev-parse", "HEAD"]);
        let reclaim = |checkout: &Checkout, known: &[String]| {
            Checkout::reclaim_worktree(checkout.worktree.as_ref().unwrap(), known).unwrap()
        };

        let committed = run_worktree(root, &base, "zone/committed");
        let kept = committed.path().to_path_buf();
        std::fs::write(kept.join("work.txt"), "committed, never pushed\n").unwrap();
        git(&kept, &["add", "work.txt"]);
        git(&kept, &["commit", "-q", "-m", "work"]);
        git(
            &kept,
            &["update-ref", "refs/remotes/origin/zone/committed", "HEAD"],
        );
        assert!(
            !reclaim(&committed, std::slice::from_ref(&start)),
            "a commit the service never pushed is kept, whatever ref the run planted"
        );
        assert!(kept.exists());
        assert!(
            git(&base, &["branch", "--list", "zone/committed"]).contains("zone/committed"),
            "the kept worktree keeps its branch"
        );

        let published = run_worktree(root, &base, "zone/published");
        let published_path = published.path().to_path_buf();
        std::fs::write(published_path.join("done.txt"), "pushed\n").unwrap();
        git(&published_path, &["add", "done.txt"]);
        git(&published_path, &["commit", "-q", "-m", "done"]);
        let pushed = git(&published_path, &["rev-parse", "HEAD"]);
        assert!(
            reclaim(&published, &[start.clone(), pushed]),
            "a worktree left on the commit the service pushed is removed"
        );
        assert!(!published_path.exists());

        let dirty = run_worktree(root, &base, "zone/dirty");
        let dirty_path = dirty.path().to_path_buf();
        std::fs::write(dirty_path.join("notes.txt"), "uncommitted\n").unwrap();
        assert!(
            !reclaim(&dirty, std::slice::from_ref(&start)),
            "a file no commit has is kept"
        );
        assert!(dirty_path.exists());

        let clean = run_worktree(root, &base, "zone/clean");
        let clean_path = clean.path().to_path_buf();
        assert!(
            reclaim(&clean, std::slice::from_ref(&start)),
            "a worktree holding nothing of its own is removed"
        );
        assert!(!clean_path.exists());
        let listed = git(&base, &["worktree", "list", "--porcelain"]);
        assert!(listed.contains(kept.to_str().unwrap()), "{listed}");
        assert!(!listed.contains(clean_path.to_str().unwrap()), "{listed}");
        assert_eq!(
            git(&base, &["branch", "--list", "zone/clean"]),
            "",
            "a removed worktree's branch goes with it, so the next run can make it again"
        );
    }

    /// Zone names a run's branch `zone/task-{short id}-{slug}`, as it always
    /// has: existing tasks resume the branches they recorded under it.
    #[test]
    fn a_run_s_branch_is_named_under_zone_task() {
        let task = Uuid::parse_str("1a2b3c4d-5e6f-4a1b-8c2d-3e4f5a6b7c8d").unwrap();
        assert_eq!(
            super::git()
                .generate_branch_name(task, "Add rate limiting!")
                .unwrap()
                .as_str(),
            "zone/task-1a2b3c4d-add-rate-limiting"
        );
    }

    /// What a run publishes is committed as Zone, not as whoever the crate
    /// would commit as by default.
    #[tokio::test]
    async fn a_run_commits_as_zone() {
        let (_holder, root) = private_root();
        let base = base_clone(&root);
        let run = run_worktree(&root, &base, "zone/identity");
        let worktree = run.worktree.clone().expect("the run is a worktree");
        std::fs::write(run.path().join("work.txt"), "work\n").unwrap();
        super::git().stage_all(&worktree).await.unwrap();
        super::git()
            .commit(&worktree, "(feat): add work")
            .await
            .unwrap();
        assert_eq!(
            git(run.path(), &["log", "-1", "--format=%an <%ae>|%cn <%ce>"]),
            "Zone <zone@localhost>|Zone <zone@localhost>"
        );
    }

    /// Recovery knows a finished run's worktree only by its path. The path
    /// is bound to the base clone it was added to, and to nothing else: not
    /// to another base under the same root, not from a directory below the
    /// worktree's top, and not when the worktree belongs to a clone outside
    /// the root.
    #[test]
    fn recovery_binds_a_worktree_path_to_the_base_clone_it_was_added_to() {
        let (_holder, root) = private_root();
        let base = base_clone(&root);
        let remote = root.join("remote");
        let other = Checkout::base(&root, "https://github.com/fixture/other.git").unwrap();
        git(
            &root,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        let run = run_worktree(&root, &base, "zone/bound");

        let bound = Checkout::bound(&root, run.path())
            .unwrap()
            .expect("a run's worktree belongs to the base it was added to");
        assert_eq!(bound.top(), run.path());
        assert_eq!(
            bound.repository().canonicalize().unwrap(),
            base.canonicalize().unwrap()
        );

        let nested = run.path().join("src");
        std::fs::create_dir(&nested).unwrap();
        assert!(
            Checkout::bound(&root, &nested).unwrap().is_none(),
            "a directory below a worktree's top is no checkout of its own"
        );

        let elsewhere = tempfile::tempdir().unwrap();
        let foreign = elsewhere.path().join("foreign");
        git(
            &root,
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                foreign.to_str().unwrap(),
            ],
        );
        let stray = root.join(format!("{}.{}", Uuid::new_v4(), Uuid::new_v4()));
        git(
            &foreign,
            &["worktree", "add", "-q", "--detach", stray.to_str().unwrap()],
        );
        assert!(
            Checkout::bound(&root, &stray).unwrap().is_none(),
            "a worktree of a clone outside the root belongs to no base here"
        );
    }

    /// An origin with a second address fetches from the first and pushes to
    /// every one, so a base whose origin names another address beside the
    /// task's repository, in either order, is not rewritten as the task's.
    #[tokio::test]
    async fn a_base_whose_origin_names_two_addresses_is_refused() {
        const ELSEWHERE: &str = "https://example.test/elsewhere.git";
        for addresses in [[ELSEWHERE, FIXTURE_ORIGIN], [FIXTURE_ORIGIN, ELSEWHERE]] {
            let (_holder, root) = private_root();
            let base = base_clone(&root);
            git(&base, &["config", "remote.origin.url", addresses[0]]);
            git(
                &base,
                &["config", "--add", "remote.origin.url", addresses[1]],
            );

            let refused = sanitize(&base, &url(FIXTURE_ORIGIN))
                .await
                .expect_err("an origin with two addresses is not the task's repository");

            assert_eq!(refused, MISMATCHED_ORIGIN, "{addresses:?}");
        }
    }

    /// A base whose configuration cannot be read is refused with what kept it
    /// from being read, so the operator can tell it from a changed origin.
    #[tokio::test]
    async fn a_base_whose_configuration_cannot_be_read_says_why() {
        let (_holder, root) = private_root();
        let base = base_clone(&root);
        let configuration = base.join(".git").join("config");
        std::fs::remove_file(&configuration).unwrap();
        std::fs::create_dir(&configuration).unwrap();

        let refused = sanitize(&base, &url(FIXTURE_ORIGIN))
            .await
            .expect_err("a configuration that is a directory is refused");

        assert!(refused.contains("not a regular file"), "{refused}");
    }

    /// The base a worktree names is the only one asked, and what it refuses
    /// the worktree with is the error recovery logs, not a quiet "belongs to
    /// no base": here a run left a hooks path in the shared configuration,
    /// which the base admits again once it is sanitized.
    #[tokio::test]
    async fn a_base_that_refuses_its_own_worktree_says_why() {
        let (_holder, root) = private_root();
        let base = base_clone(&root);
        let run = run_worktree(&root, &base, "zone/refused");
        git(&base, &["config", "core.hooksPath", "/tmp"]);

        let refused = Checkout::bound(&root, run.path())
            .expect_err("a base configured beyond a clone refuses its worktree");
        assert!(refused.to_string().contains("core.hookspath"), "{refused}");

        sanitize(&base, &url(FIXTURE_ORIGIN)).await.unwrap();
        assert!(
            Checkout::bound(&root, run.path()).unwrap().is_some(),
            "a sanitized base admits its worktree again"
        );
    }

    /// Runs of one repository share its base clone's configuration, which a
    /// run's tools write: husky's `npm install` sets `core.hooksPath`, a
    /// submodule adds `submodule.*`. Finishing or dropping one run's checkout
    /// leaves that configuration and the run's worktree to recovery, which
    /// waits until no run works in the base: restoring it here would undo
    /// what a sibling still running wrote and relies on.
    #[tokio::test]
    async fn ending_a_run_leaves_a_sibling_s_configuration_and_its_worktree_to_recovery() {
        let (_holder, root) = private_root();
        let base = base_clone(&root);
        let sibling = run_worktree(&root, &base, "zone/sibling");
        let finished = run_worktree(&root, &base, "zone/finished");
        let finished_path = finished.path().to_path_buf();
        let cancelled = run_worktree(&root, &base, "zone/cancelled");
        let cancelled_path = cancelled.path().to_path_buf();
        let sentinel = root.join("hook-ran");
        write_shared_configuration(sibling.path(), &sentinel);

        finished.finish().await;
        drop(cancelled);

        let configured = std::fs::read_to_string(base.join(".git").join("config")).unwrap();
        assert!(
            configured.contains("hooksPath"),
            "ending one run rewrote the configuration a sibling still works with: {configured}"
        );
        assert!(configured.contains("submodule"), "{configured}");
        assert!(
            finished_path.exists(),
            "a finished run's worktree waits for recovery"
        );
        assert!(
            cancelled_path.exists(),
            "a dropped run's worktree waits for recovery"
        );
        assert!(
            !sentinel.exists(),
            "a hook the sibling installed ran on the host"
        );
        drop(sibling);
    }

    /// A run with no repository has a plain directory, removed as before.
    #[test]
    fn a_directory_that_is_no_worktree_is_removed_as_it_always_was() {
        let (_holder, root) = private_root();
        let checkout = Checkout::create(&root, Uuid::new_v4(), Uuid::new_v4()).unwrap();
        let path = checkout.path().to_path_buf();
        std::fs::write(path.join("scratch"), "x").unwrap();
        drop(checkout);
        assert!(!path.exists());
    }

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

    /// The guard's fallback in `Drop` runs on whatever thread drops the
    /// checkout, often one of a runtime's workers. It must never wait for
    /// the base's lock, which async tasks hold across a fetch or a
    /// publication: on a runtime with one worker, waiting would stop the
    /// very task that releases it. The worktree is left to recovery.
    #[test]
    fn dropping_a_checkout_never_waits_for_its_base_lock() {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let kept = runtime.block_on(async {
                let (_holder, root) = private_root();
                let base = base_clone(&root);
                let run = run_worktree(&root, &base, "zone/busy");
                let path = run.path().to_path_buf();
                let lock = Checkout::lock_base(&base).await.unwrap();
                let holder = tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    drop(lock);
                });
                drop(run);
                holder.await.unwrap();
                path.exists()
            });
            let _ = sender.send(kept);
        });
        let kept = receiver
            .recv_timeout(std::time::Duration::from_secs(60))
            .expect("dropping a checkout whose base is locked stalled the runtime");
        assert!(
            kept,
            "the worktree is left to recovery while its base is busy"
        );
    }

    /// While a run still works in a base clone, recovery leaves that base's
    /// configuration alone, whatever finished worktree of it waits to be
    /// reclaimed: the live run's own tools may have written it on purpose.
    /// Once no run works there, the next sweep restores it and reclaims both.
    #[tokio::test]
    async fn recovery_leaves_a_base_alone_while_another_run_works_in_it() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .unwrap();
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query("INSERT INTO organizations(id,name,slug) VALUES($1,'Live base',$1::text)")
            .bind(organization)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'Live base',$1::text)").bind(workspace).bind(organization).execute(&pool).await.unwrap();
        let task = crate::db::tasks::create_task(
            &pool,
            workspace,
            &[],
            "Live base",
            "Regression",
            None,
            None,
            true,
            None,
        )
        .await
        .unwrap();
        let (_holder, root) = private_root();
        let base = base_clone(&root);
        let mut paths = Vec::new();
        let mut runs = Vec::new();
        for live in [false, true] {
            let run = crate::db::tasks::create_task_run(&pool, task.id)
                .await
                .unwrap();
            let owner = Uuid::new_v4();
            assert!(
                crate::db::tasks::claim_task_run(&pool, run.id, owner)
                    .await
                    .unwrap()
            );
            let mut checkout = Checkout::create(&root, run.id, owner).unwrap();
            checkout.worktree = Some(worktree::add(&base, checkout.path(), "origin/HEAD").unwrap());
            let start = git(checkout.path(), &["rev-parse", "HEAD"]);
            assert!(
                crate::db::tasks::record_run_checkout(&pool, run.id, owner, &start)
                    .await
                    .unwrap()
            );
            paths.push(checkout.path().to_path_buf());
            std::mem::forget(checkout);
            if !live {
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
            }
            runs.push((run.id, owner));
        }
        git(&paths[1], &["config", "core.hooksPath", "/tmp"]);

        let deferred = Checkout::recover_root(&pool, &root).await.unwrap();
        let during = std::fs::read_to_string(base.join(".git").join("config")).unwrap();
        let waiting = paths[0].exists();
        let (live, owner) = runs[1];
        crate::db::tasks::complete_owned_task_run(
            &pool,
            live,
            Some(owner),
            "completed",
            None,
            None,
        )
        .await
        .unwrap();
        let reclaimed = Checkout::recover_root(&pool, &root).await.unwrap();
        let after = std::fs::read_to_string(base.join(".git").join("config")).unwrap();
        sqlx::query("DELETE FROM organizations WHERE id=$1")
            .bind(organization)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(deferred, 0, "nothing of a base a run works in is reclaimed");
        assert!(
            during.contains("hooksPath"),
            "recovery rewrote the configuration of a base a run still works in: {during}"
        );
        assert!(waiting, "the finished worktree waits for a later sweep");
        assert_eq!(
            reclaimed, 2,
            "once no run works in the base, both are reclaimed"
        );
        assert!(!after.contains("hooksPath"), "{after}");
    }

    /// Recovery reclaims a finished run's worktree only once nothing in it is
    /// lost: while it holds a commit the service never pushed, the sweep
    /// leaves it and says so; once the worktree is back on the commit the
    /// service recorded it started from, the next sweep removes it. A run
    /// whose start was never recorded has nothing anyone can vouch for, and
    /// its worktree is left even when it is clean. The sweep runs over a
    /// root of this test's own, as the recovery tests beside it do, so
    /// nothing here depends on what earlier runs left on the disk.
    #[tokio::test]
    async fn recovery_leaves_a_finished_run_s_worktree_while_it_holds_work() {
        let pool =
            PgPool::connect(&std::env::var("TEST_DATABASE_URL").expect("isolated test database"))
                .await
                .unwrap();
        let organization = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO organizations(id,name,slug) VALUES($1,'GitCheckout recovery',$1::text)",
        )
        .bind(organization)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,'GitCheckout recovery',$1::text)").bind(workspace).bind(organization).execute(&pool).await.unwrap();
        let task = crate::db::tasks::create_task(
            &pool,
            workspace,
            &[],
            "Recover worktree",
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
        let (_holder, root) = private_root();
        let base = base_clone(&root);
        let mut checkout = Checkout::create(&root, run.id, owner).unwrap();
        checkout.worktree = Some(worktree::add(&base, checkout.path(), "origin/HEAD").unwrap());
        git(checkout.path(), &["checkout", "-q", "-b", "zone/recovered"]);
        let start = git(checkout.path(), &["rev-parse", "HEAD"]);
        assert!(
            crate::db::tasks::record_run_checkout(&pool, run.id, owner, &start)
                .await
                .unwrap(),
            "the start is recorded under the run's lease"
        );
        let path = checkout.path().to_path_buf();
        std::fs::write(path.join("work.txt"), "committed, never pushed\n").unwrap();
        git(&path, &["add", "work.txt"]);
        git(&path, &["commit", "-q", "-m", "work"]);
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

        // A second run of the task whose start nobody recorded: clean, and
        // still not reclaimable, since nothing vouches for its commit.
        let unrecorded = crate::db::tasks::create_task_run(&pool, task.id)
            .await
            .unwrap();
        let second_owner = Uuid::new_v4();
        assert!(
            crate::db::tasks::claim_task_run(&pool, unrecorded.id, second_owner)
                .await
                .unwrap()
        );
        let mut second = Checkout::create(&root, unrecorded.id, second_owner).unwrap();
        second.worktree = Some(worktree::add(&base, second.path(), "origin/HEAD").unwrap());
        git(second.path(), &["checkout", "-q", "-b", "zone/unrecorded"]);
        let unrecorded_path = second.path().to_path_buf();
        std::mem::forget(second);
        crate::db::tasks::complete_owned_task_run(
            &pool,
            unrecorded.id,
            Some(second_owner),
            "failed",
            Some("orphaned"),
            None,
        )
        .await
        .unwrap();

        assert_eq!(Checkout::recover_root(&pool, &root).await.unwrap(), 0);
        assert!(
            path.exists(),
            "a finished run's worktree holding a commit the service never pushed is left"
        );
        assert!(git(&base, &["branch", "--list", "zone/recovered"]).contains("zone/recovered"));
        assert!(
            unrecorded_path.exists(),
            "a worktree whose start was never recorded is left"
        );

        // Back on the recorded start, nothing of its own is left: the sweep
        // may now take it, branch and all. The unrecorded one stays.
        git(&path, &["reset", "-q", "--hard", &start]);
        write_shared_configuration(&path, &root.join("hook-ran"));
        assert_eq!(Checkout::recover_root(&pool, &root).await.unwrap(), 1);
        assert!(
            !root.join("hook-ran").exists(),
            "recovery ran a hook the run installed"
        );
        assert!(
            !path.exists(),
            "a worktree of a finished run back on its recorded start is reclaimed"
        );
        assert_eq!(git(&base, &["branch", "--list", "zone/recovered"]), "");
        assert!(unrecorded_path.exists());
        sqlx::query("DELETE FROM organizations WHERE id=$1")
            .bind(organization)
            .execute(&pool)
            .await
            .unwrap();
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
        let key = MasterKey::generate().unwrap();
        let checkout = Checkout::prepare(
            &pool,
            &key,
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
            Some(url("https://github.com/owner/requested.git")),
            vec![Repository {
                url: url("https://github.com/owner/other.git"),
                token: Some(SecretValue::new("secret")),
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
                        url: url("https://github.com/owner/first.git"),
                        token: None
                    },
                    Repository {
                        url: url("https://github.com/owner/second.git"),
                        token: None
                    }
                ]
            )
            .is_err()
        );
    }
}
