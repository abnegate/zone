//! Git operations service
//!
//! Provides git operations for PR creation workflow:
//! - Branch creation with task-based naming
//! - Staging and committing changes
//! - Pushing to remote

use base64::Engine;

/// Longest diff kept before truncation, in bytes.
const MAXIMUM_DIFF_BYTES: usize = 50_000;

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;
use uuid::Uuid;

/// Git service errors
#[derive(Debug, Error)]
pub enum GitError {
    #[error("Git command failed: {0}")]
    CommandFailed(String),

    #[error("Repository not found at {0}")]
    RepoNotFound(String),

    #[error("Remote not configured")]
    NoRemote,

    #[error("No changes to commit")]
    NoChanges,

    #[error("Branch already exists: {0}")]
    BranchExists(String),

    #[error("Authentication failed")]
    AuthFailed,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type GitResult<T> = Result<T, GitError>;

/// Result of a git diff operation
#[derive(Debug, Clone)]
pub struct DiffSummary {
    pub files_changed: Vec<String>,
    pub insertions: u32,
    pub deletions: u32,
    pub diff_text: String,
}

/// A remote's default branch and the commit at its tip, as the remote itself
/// reports them: what a run starts from is asked of the repository, not read
/// from a ref every run of it shares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteHead {
    pub branch: String,
    pub commit: String,
}

/// Git service for repository operations
#[derive(Debug, Clone)]
pub struct GitService {
    /// Maximum branch name length
    max_branch_length: usize,
}

impl Default for GitService {
    fn default() -> Self {
        Self::new()
    }
}

/// The group remains separate from the server so cancellation cannot signal
/// another task. Drop covers timeout/future cancellation, not server SIGKILL;
/// abrupt process death requires the hosting supervisor to tear down its group.
#[cfg(unix)]
struct Group(nix::unistd::Pid);

#[cfg(unix)]
impl Drop for Group {
    fn drop(&mut self) {
        if let Err(error) = nix::sys::signal::killpg(self.0, nix::sys::signal::Signal::SIGKILL)
            && error != nix::errno::Errno::ESRCH
        {
            tracing::warn!(%error, group = self.0.as_raw(), "Could not terminate Git process group");
        }
    }
}

impl GitService {
    /// Create a new git service
    pub fn new() -> Self {
        Self {
            max_branch_length: 100,
        }
    }

    /// Accept GitHub HTTPS repositories without URL credentials or transport options.
    pub fn repository_url(source: &str) -> GitResult<String> {
        let invalid =
            || GitError::CommandFailed("Expected an HTTPS GitHub owner/repository URL".to_string());
        let url = reqwest::Url::parse(source).map_err(|_| invalid())?;
        if url.scheme() != "https"
            || url.host_str() != Some("github.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        let path = url.path().trim_matches('/');
        let parts: Vec<_> = path.split('/').collect();
        if parts.len() != 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || *part == "."
                    || *part == ".."
                    || !part.chars().all(|character| {
                        character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
                    })
            })
        {
            return Err(invalid());
        }
        let name = parts[1].strip_suffix(".git").unwrap_or(parts[1]);
        if name.is_empty() {
            return Err(invalid());
        }
        Ok(format!("https://github.com/{}/{}.git", parts[0], name))
    }

    /// Clone into an empty, caller-owned directory. Credentials live only in the
    /// child environment, never the origin URL, process arguments, or git config.
    pub async fn clone_repository(
        &self,
        source: &str,
        destination: &Path,
        token: Option<&str>,
    ) -> GitResult<()> {
        let source = Self::repository_url(source)?;
        self.clone_source(&source, destination, token, false).await
    }

    fn network_command(token: Option<&str>) -> Command {
        // Task-local replacement refs and legacy grafts must not reinterpret
        // the stored objects used by history checks, diffs, commits or pushes.
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_GRAFT_FILE", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "/usr/bin/false")
            .env("GIT_ALLOW_PROTOCOL", "https")
            .args([
                "-c",
                "credential.helper=",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "http.followRedirects=false",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(token) = token {
            authenticate(&mut command, token);
        }
        command
    }

    async fn output(command: &mut Command) -> GitResult<std::process::Output> {
        #[cfg(unix)]
        command.process_group(0);
        command.kill_on_drop(true);
        let child = command.spawn()?;
        #[cfg(unix)]
        let _group = Group(nix::unistd::Pid::from_raw(
            child
                .id()
                .ok_or_else(|| std::io::Error::other("Git process has no ID"))? as i32,
        ));
        match tokio::time::timeout(Duration::from_secs(300), child.wait_with_output()).await {
            Ok(output) => Ok(output?),
            Err(_) => Err(GitError::CommandFailed(
                "Git operation timed out".to_string(),
            )),
        }
    }

    async fn finish(command: &mut Command) -> GitResult<()> {
        let output = Self::output(command).await?;
        if !output.status.success() {
            return Err(GitError::CommandFailed(
                "Git operation failed; verify repository access".to_string(),
            ));
        }
        Ok(())
    }

    async fn clone_source(
        &self,
        source: &str,
        destination: &Path,
        token: Option<&str>,
        local: bool,
    ) -> GitResult<()> {
        let mut command = Self::network_command(token);
        // Local transport is reachable only from this module's controlled fixture tests.
        if local {
            #[cfg(test)]
            command.env("GIT_ALLOW_PROTOCOL", "file");
            #[cfg(not(test))]
            return Err(GitError::CommandFailed(
                "Local repositories are disabled".to_string(),
            ));
        }
        command
            .args(["clone", "--no-hardlinks", "--template=", "--", source])
            .arg(destination);
        Self::finish(&mut command).await
    }

    /// Resume the task branch from a fresh clone without rewriting its history.
    pub async fn prepare_branch(&self, path: &Path, branch: &str, required: bool) -> GitResult<()> {
        let reference = format!("refs/heads/{branch}");
        let valid =
            Self::output(Self::network_command(None).args(["check-ref-format", &reference]))
                .await?;
        if !valid.status.success() || branch.starts_with('-') {
            return Err(GitError::CommandFailed("Invalid task branch".into()));
        }
        let remote = format!("refs/remotes/origin/{branch}");
        let exists = Self::output(
            Self::network_command(None)
                .args(["show-ref", "--verify", "--quiet", &remote])
                .current_dir(path),
        )
        .await?;
        if !exists.status.success() && (required || exists.status.code() != Some(1)) {
            return Err(GitError::CommandFailed(
                "Task branch is missing from the repository".into(),
            ));
        }
        if self.current_branch(path).await? == branch {
            return Ok(());
        }
        // A run works in a worktree of a base clone the repository's runs
        // share, so a branch that already exists here is either one an earlier
        // run's worktree still holds — kept because it has work no remote has,
        // and the run is refused with that reason rather than git's wording —
        // or one a removed worktree left behind when its deletion failed, which
        // nothing holds and this run may take over.
        let held = Self::output(
            Self::network_command(None)
                .args(["show-ref", "--verify", "--quiet", &reference])
                .current_dir(path),
        )
        .await?;
        if held.status.success() {
            // A worktree whose directory was deleted by hand no longer holds
            // anything; prune it so it cannot stand in for one that does.
            let mut prune = Self::network_command(None);
            prune.args(["worktree", "prune"]).current_dir(path);
            let _ = Self::finish(&mut prune).await;
            let listed = Self::output(
                Self::network_command(None)
                    .args(["worktree", "list", "--porcelain"])
                    .current_dir(path)
                    .stdout(Stdio::piped()),
            )
            .await?;
            let holder = format!("branch {reference}");
            if String::from_utf8_lossy(&listed.stdout)
                .lines()
                .any(|line| line == holder)
            {
                return Err(GitError::CommandFailed(
                    "An earlier run's worktree still holds this task's branch with work that was \
                     never published; publish or remove it before running the task again"
                        .into(),
                ));
            }
            let mut delete = Self::network_command(None);
            delete
                .args(["branch", "-D", "--", branch])
                .current_dir(path);
            Self::finish(&mut delete).await?;
        }
        let mut command = Self::network_command(None);
        command.args(["checkout", "-b", branch]);
        if exists.status.success() {
            command.arg(&remote);
        }
        command.arg("--").current_dir(path);
        Self::finish(&mut command).await
    }

    /// Resolve a commit without reading a caller-controlled symbolic baseline later.
    pub async fn revision(&self, path: &Path, reference: &str) -> GitResult<String> {
        let output = Self::output(
            Self::network_command(None)
                .args([
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{reference}^{{commit}}"),
                ])
                .current_dir(path)
                .stdout(Stdio::piped()),
        )
        .await?;
        if !output.status.success() {
            return Err(GitError::CommandFailed("Checkout commit is missing".into()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub async fn is_ancestor(&self, path: &Path, before: &str, after: &str) -> GitResult<bool> {
        let output = Self::output(
            Self::network_command(None)
                .args(["merge-base", "--is-ancestor", before, after])
                .current_dir(path),
        )
        .await?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(GitError::CommandFailed(
                "Cannot verify checkout history".into(),
            )),
        }
    }

    /// Include changes already committed by task tools in the PR description.
    pub async fn changed_files(&self, path: &Path, before: &str) -> GitResult<Vec<String>> {
        let output = Self::output(
            Self::network_command(None)
                .args(["diff", "--name-only", "-z", before, "HEAD", "--"])
                .current_dir(path)
                .stdout(Stdio::piped()),
        )
        .await?;
        if !output.status.success() {
            return Err(GitError::CommandFailed(
                "Cannot inspect task changes".into(),
            ));
        }
        Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect())
    }

    /// Generate a branch name for a task
    ///
    /// Format: zone/task-{short_id}-{slug}
    /// Where slug is a sanitized version of the first few words of the title
    pub fn generate_branch_name(&self, task_id: Uuid, title: &str) -> String {
        let short_id = &task_id.to_string()[..8];

        // Sanitize title: lowercase, replace spaces/special chars with hyphens
        let slug: String = title
            .chars()
            .take(50) // Limit title length
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();

        // Remove consecutive hyphens and trim
        let slug = slug
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        let branch = format!("zone/task-{}-{}", short_id, slug);

        // Truncate if too long
        if branch.len() > self.max_branch_length {
            branch[..self.max_branch_length].to_string()
        } else {
            branch
        }
    }

    /// Check if a path is a git repository
    pub async fn is_git_repo(&self, path: &Path) -> GitResult<bool> {
        let output = Self::output(
            Self::network_command(None)
                .arg("rev-parse")
                .arg("--is-inside-work-tree")
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        Ok(output.status.success())
    }

    /// Get the current branch name
    pub async fn current_branch(&self, path: &Path) -> GitResult<String> {
        let output = Self::output(
            Self::network_command(None)
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Bring a base clone up to date with the repository at `url`, credentials
    /// in the child environment only. The URL is the one Zone knows, given on
    /// the command line, so nothing a run wrote into the clone's configuration
    /// — a rewritten `remote.origin.url`, an `insteadOf` rule — decides where
    /// the fetch goes; every `refs/remotes/origin/*` ref is forced to what the
    /// remote holds and the ones it no longer has are pruned. A refusal is
    /// retried once, since git's ref locks refuse the loser of a race with a
    /// run's own git commands.
    pub async fn fetch(&self, path: &Path, url: &str, token: Option<&str>) -> GitResult<()> {
        let source = Self::repository_url(url)?;
        self.fetch_source(path, &source, token, false).await
    }

    async fn fetch_source(
        &self,
        path: &Path,
        source: &str,
        token: Option<&str>,
        local: bool,
    ) -> GitResult<()> {
        let mut attempts = 0;
        loop {
            let mut command = Self::network_command(token);
            if local {
                #[cfg(test)]
                command.env("GIT_ALLOW_PROTOCOL", "file");
                #[cfg(not(test))]
                return Err(GitError::CommandFailed(
                    "Local repositories are disabled".to_string(),
                ));
            }
            command
                .args([
                    "fetch",
                    "--prune",
                    "--",
                    source,
                    "+refs/heads/*:refs/remotes/origin/*",
                ])
                .current_dir(path);
            match Self::finish(&mut command).await {
                Ok(()) => return Ok(()),
                Err(error) if attempts == 0 => {
                    attempts += 1;
                    tracing::debug!(%error, "Retrying a fetch another git command may have locked");
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// The remote's default branch and the commit at its tip, asked of the
    /// remote itself. `refs/remotes/origin/HEAD` is not consulted: a run's git
    /// commands share it with every other run of the repository, and a fetch
    /// never restores it.
    pub async fn remote_head(
        &self,
        path: &Path,
        url: &str,
        token: Option<&str>,
    ) -> GitResult<RemoteHead> {
        let source = Self::repository_url(url)?;
        self.remote_head_source(path, &source, token, false).await
    }

    async fn remote_head_source(
        &self,
        path: &Path,
        source: &str,
        token: Option<&str>,
        local: bool,
    ) -> GitResult<RemoteHead> {
        let mut command = Self::network_command(token);
        if local {
            #[cfg(test)]
            command.env("GIT_ALLOW_PROTOCOL", "file");
            #[cfg(not(test))]
            return Err(GitError::CommandFailed(
                "Local repositories are disabled".to_string(),
            ));
        }
        command
            .args(["ls-remote", "--symref", "--", source, "HEAD"])
            .current_dir(path)
            .stdout(Stdio::piped());
        let output = Self::output(&mut command).await?;
        if !output.status.success() {
            return Err(GitError::CommandFailed(
                "Git operation failed; verify repository access".to_string(),
            ));
        }
        let listing = String::from_utf8_lossy(&output.stdout);
        let (mut branch, mut commit) = (None, None);
        for line in listing.lines() {
            let Some((left, right)) = line.split_once('\t') else {
                continue;
            };
            if right != "HEAD" {
                continue;
            }
            if let Some(reference) = left.strip_prefix("ref: ") {
                branch = reference.strip_prefix("refs/heads/").map(str::to_string);
            } else if !left.is_empty() && left.chars().all(|c| c.is_ascii_hexdigit()) {
                commit = Some(left.to_string());
            }
        }
        match (branch, commit) {
            (Some(branch), Some(commit)) => Ok(RemoteHead { branch, commit }),
            _ => Err(GitError::CommandFailed(
                "The repository reports no default branch".to_string(),
            )),
        }
    }

    /// Rewrite a base clone's configuration from what Zone knows about it.
    /// The clone's `.git/config` is read by every git command that runs in
    /// it or in a worktree of it, and a run's git commands can write to it:
    /// a `core.fsmonitor` or a filter driver would run as this process on the
    /// next fetch or checkout, an `insteadOf` rule would redirect it. So the
    /// file is replaced before the base is used, carrying over only the
    /// settings git chose for the file system, and the remote's URL is
    /// written through `git config`, which escapes it.
    pub async fn reset_config(&self, path: &Path, url: &str) -> GitResult<()> {
        const CARRIED: [&str; 4] = ["filemode", "ignorecase", "precomposeunicode", "symlinks"];
        if url.chars().any(char::is_control) {
            return Err(GitError::CommandFailed("Invalid repository URL".into()));
        }
        let file = path.join(".git").join("config");
        let listed = Self::output(
            Self::network_command(None)
                .args(["config", "--file"])
                .arg(&file)
                .arg("--list")
                .stdout(Stdio::piped()),
        )
        .await?;
        let mut carried = String::new();
        if listed.status.success() {
            for line in String::from_utf8_lossy(&listed.stdout).lines() {
                if let Some((key, value)) = line.split_once('=')
                    && let Some(name) = key.strip_prefix("core.")
                    && CARRIED.contains(&name)
                    && matches!(value, "true" | "false")
                {
                    carried.push_str(&format!("\t{name} = {value}\n"));
                }
            }
        }
        tokio::fs::write(
            &file,
            format!(
                "[core]\n\trepositoryformatversion = 0\n\tbare = false\n\tlogallrefupdates = true\n{carried}"
            ),
        )
        .await?;
        for (key, value) in [
            ("remote.origin.url", url),
            ("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"),
        ] {
            Self::finish(
                Self::network_command(None)
                    .args(["config", "--file"])
                    .arg(&file)
                    .args([key, value]),
            )
            .await?;
        }
        Ok(())
    }

    /// Whether the repository holds `commit`, given as a hex object name.
    pub async fn has_commit(&self, path: &Path, commit: &str) -> GitResult<bool> {
        if commit.is_empty() || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(GitError::CommandFailed("Invalid commit".into()));
        }
        let output = Self::output(
            Self::network_command(None)
                .args(["cat-file", "-e", &format!("{commit}^{{commit}}")])
                .current_dir(path),
        )
        .await?;
        Ok(output.status.success())
    }

    /// Point `refs/remotes/origin/HEAD` at the remote's default branch, for
    /// whoever reads the ref; a run is started from the commit
    /// [`Self::remote_head`] reported, never from this ref.
    pub async fn set_remote_head(&self, path: &Path, branch: &str) -> GitResult<()> {
        let reference = format!("refs/remotes/origin/{branch}");
        let valid =
            Self::output(Self::network_command(None).args(["check-ref-format", &reference]))
                .await?;
        if !valid.status.success() || branch.starts_with('-') {
            return Err(GitError::CommandFailed("Invalid default branch".into()));
        }
        Self::finish(
            Self::network_command(None)
                .args(["symbolic-ref", "refs/remotes/origin/HEAD", &reference])
                .current_dir(path),
        )
        .await
    }

    /// Check if there are uncommitted changes
    pub async fn has_changes(&self, path: &Path) -> GitResult<bool> {
        let output = Self::output(
            Self::network_command(None)
                .args(["status", "--porcelain"])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(!output.stdout.is_empty())
    }

    /// Get a summary of uncommitted changes
    pub async fn diff_summary(&self, path: &Path) -> GitResult<DiffSummary> {
        // Get list of changed files
        let status_output = Self::output(
            Self::network_command(None)
                .args(["status", "--porcelain"])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if !status_output.status.success() {
            let stderr = String::from_utf8_lossy(&status_output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        let status_text = String::from_utf8_lossy(&status_output.stdout);
        let files_changed: Vec<String> = status_text
            .lines()
            .filter_map(|line| {
                if line.len() > 3 {
                    Some(line[3..].to_string())
                } else {
                    None
                }
            })
            .collect();

        // Get diff stats
        let diff_stat_output = Self::output(
            Self::network_command(None)
                .args(["diff", "--shortstat", "HEAD"])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        let mut insertions = 0;
        let mut deletions = 0;

        if diff_stat_output.status.success() {
            let stat_text = String::from_utf8_lossy(&diff_stat_output.stdout);
            // Parse "1 file changed, 10 insertions(+), 5 deletions(-)"
            for part in stat_text.split(',') {
                let part = part.trim();
                if part.contains("insertion") {
                    if let Some(num) = part.split_whitespace().next() {
                        insertions = num.parse().unwrap_or(0);
                    }
                } else if part.contains("deletion")
                    && let Some(num) = part.split_whitespace().next()
                {
                    deletions = num.parse().unwrap_or(0);
                }
            }
        }

        // Get actual diff text (limited)
        let diff_output = Self::output(
            Self::network_command(None)
                .args(["diff", "HEAD"])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        let diff_text = String::from_utf8_lossy(&diff_output.stdout);
        // len() counts bytes, so cutting at a fixed offset panics whenever the
        // boundary lands inside a multi-byte character -- an emoji or any
        // accented character in a diff over the cap is enough.
        let diff_text = if diff_text.len() > MAXIMUM_DIFF_BYTES {
            let mut end = MAXIMUM_DIFF_BYTES;
            while end > 0 && !diff_text.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...[truncated]", &diff_text[..end])
        } else {
            diff_text.to_string()
        };

        Ok(DiffSummary {
            files_changed,
            insertions,
            deletions,
            diff_text,
        })
    }

    /// Create and checkout a new branch
    pub async fn create_branch(&self, path: &Path, branch_name: &str) -> GitResult<()> {
        // Check if branch already exists
        let check_output = Self::output(
            Self::network_command(None)
                .args([
                    "show-ref",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{}", branch_name),
                ])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if check_output.status.success() {
            return Err(GitError::BranchExists(branch_name.to_string()));
        }

        // Create and checkout the branch
        let output = Self::output(
            Self::network_command(None)
                .args(["checkout", "-b", branch_name])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Stage all changes
    pub async fn stage_all(&self, path: &Path) -> GitResult<()> {
        let output = Self::output(
            Self::network_command(None)
                .args(["add", "-A"])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Commit staged changes
    pub async fn commit(&self, path: &Path, message: &str) -> GitResult<String> {
        let output = Self::output(
            Self::network_command(None)
                .args([
                    "-c",
                    "user.name=Zone",
                    "-c",
                    "user.email=zone@localhost",
                    "commit",
                    "-m",
                    message,
                ])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("nothing to commit") {
                return Err(GitError::NoChanges);
            }
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        // Get the commit SHA
        let sha_output = Self::output(
            Self::network_command(None)
                .args(["rev-parse", "HEAD"])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        Ok(String::from_utf8_lossy(&sha_output.stdout)
            .trim()
            .to_string())
    }

    /// Push branch to remote
    pub async fn push(&self, path: &Path, branch_name: &str, remote: &str) -> GitResult<()> {
        let output = Self::output(
            Self::network_command(None)
                .args(["push", "-u", remote, branch_name])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("Authentication failed")
                || stderr.contains("could not read Username")
            {
                return Err(GitError::AuthFailed);
            }
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Push branch with access token authentication
    pub async fn push_with_token(
        &self,
        path: &Path,
        branch_name: &str,
        remote_url: &str,
        token: &str,
    ) -> GitResult<()> {
        let remote = Self::repository_url(remote_url)?;
        self.push_source(path, branch_name, &remote, token, false)
            .await
    }

    async fn push_source(
        &self,
        path: &Path,
        branch_name: &str,
        remote: &str,
        token: &str,
        local: bool,
    ) -> GitResult<()> {
        let mut command = Self::network_command(Some(token));
        if local {
            #[cfg(test)]
            command.env("GIT_ALLOW_PROTOCOL", "file");
            #[cfg(not(test))]
            return Err(GitError::CommandFailed(
                "Local repositories are disabled".into(),
            ));
        }
        command
            .args([
                "push",
                "--porcelain",
                "--",
                remote,
                &format!("HEAD:refs/heads/{branch_name}"),
            ])
            .current_dir(path)
            .stdout(Stdio::piped());
        let output = Self::output(&mut command).await?;
        if output.status.success() {
            // What was pushed is what origin has. Said in the repository's own
            // terms so a worktree's "unpublished" reading — commits no
            // remote-tracking ref holds — turns false the moment it is true.
            let mut tracking = Self::network_command(None);
            tracking
                .args([
                    "update-ref",
                    &format!("refs/remotes/origin/{branch_name}"),
                    "HEAD",
                ])
                .current_dir(path);
            if let Err(error) = Self::finish(&mut tracking).await {
                tracing::warn!(%error, "Pushed, but could not record the remote-tracking ref");
            }
            return Ok(());
        }
        // Classify only Git's machine-readable status; never expose remote output
        // or URLs that could contain credentials or untrusted server messages.
        let rejected = output
            .stdout
            .split(|byte| *byte == b'\n')
            .any(|line| line.starts_with(b"!\t"));
        Err(GitError::CommandFailed(if rejected {
            "Git push rejected; remote history or policy changed. Retry from a fresh checkout"
                .into()
        } else {
            "Git push failed; verify repository access".into()
        }))
    }

    /// Get the default remote URL
    pub async fn get_remote_url(&self, path: &Path, remote: &str) -> GitResult<String> {
        let output = Self::output(
            Self::network_command(None)
                .args(["remote", "get-url", remote])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if !output.status.success() {
            return Err(GitError::NoRemote);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Checkout existing branch
    pub async fn checkout(&self, path: &Path, branch_name: &str) -> GitResult<()> {
        let output = Self::output(
            Self::network_command(None)
                .args(["checkout", branch_name])
                .current_dir(path)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped()),
        )
        .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }
}

/// Authenticate a git network command without putting the token in the URL.
///
/// A credential in the remote URL reaches `.git/config`, the process table and
/// any error text that echoes the remote. The header is scoped to github.com so
/// a redirect elsewhere cannot carry it.
pub(crate) fn authenticate(command: &mut Command, token: &str) {
    let authorization =
        base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
    command
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraHeader")
        .env(
            "GIT_CONFIG_VALUE_0",
            format!("Authorization: Basic {authorization}"),
        );
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_branch_name() {
        let service = GitService::new();
        let task_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let branch = service.generate_branch_name(task_id, "Fix the login bug");
        assert!(branch.starts_with("zone/task-12345678-"));
        assert!(branch.contains("fix-the-login-bug"));
    }

    #[test]
    fn test_generate_branch_name_special_chars() {
        let service = GitService::new();
        let task_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let branch = service.generate_branch_name(task_id, "Add user@email validation!!!");
        assert!(!branch.contains('@'));
        assert!(!branch.contains('!'));
    }

    #[test]
    fn test_generate_branch_name_truncation() {
        let service = GitService::new();
        let task_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let long_title = "A".repeat(200);
        let branch = service.generate_branch_name(task_id, &long_title);
        assert!(branch.len() <= 100);
    }
}

#[cfg(test)]
mod checkout_tests {
    use super::*;

    #[test]
    fn rejects_unsafe_repository_inputs() {
        for source in [
            "--upload-pack=evil",
            "/tmp/repository",
            "file:///tmp/repository",
            "ext::command",
            "git@github.com:owner/repository",
            "https://token@github.com/owner/repository",
            "https://github.com/owner/repository?token=secret",
            "https://elsewhere.test/owner/repository",
            "https://github.com/owner/repository/extra",
        ] {
            assert!(GitService::repository_url(source).is_err(), "{source}");
        }
        assert_eq!(
            GitService::repository_url("https://github.com/owner/repository").unwrap(),
            "https://github.com/owner/repository.git"
        );
    }

    #[test]
    fn authentication_is_not_in_arguments_or_repository_config() {
        let command = GitService::network_command(Some("sensitive-token"));
        let arguments = format!("{:?}", command.as_std().get_args().collect::<Vec<_>>());
        assert!(!arguments.contains("sensitive-token"));
        assert!(arguments.contains("credential.helper="));
        assert!(arguments.contains("http.followRedirects=false"));
    }

    #[tokio::test]
    async fn failed_clone_prevents_execution() {
        let fixture = tempfile::tempdir().unwrap();
        let mut executed = false;
        let result = async {
            GitService::new()
                .clone_source(
                    fixture.path().join("missing").to_str().unwrap(),
                    &fixture.path().join("checkout"),
                    Some("sensitive-token"),
                    true,
                )
                .await?;
            executed = true;
            Ok::<(), GitError>(())
        }
        .await;
        assert!(result.is_err());
        assert!(!executed);
        assert!(!result.unwrap_err().to_string().contains("sensitive-token"));
    }

    #[tokio::test]
    async fn clone_contains_committed_sentinel() {
        let fixture = tempfile::tempdir().unwrap();
        for arguments in [
            vec!["init"],
            vec!["config", "user.name", "Fixture"],
            vec!["config", "user.email", "fixture@example.test"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(arguments)
                    .current_dir(fixture.path())
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        std::fs::write(fixture.path().join("sentinel"), "committed").unwrap();
        for arguments in [vec!["add", "sentinel"], vec!["commit", "-m", "fixture"]] {
            assert!(
                std::process::Command::new("git")
                    .args(arguments)
                    .current_dir(fixture.path())
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        let destination = tempfile::tempdir().unwrap();
        let path = destination.path().join("checkout");
        GitService::new()
            .clone_source(fixture.path().to_str().unwrap(), &path, None, true)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(path.join("sentinel")).unwrap(),
            "committed"
        );
        assert!(path.join(".git").is_dir());
        let second = destination.path().join("second");
        GitService::new()
            .clone_source(
                fixture.path().to_str().unwrap(),
                &second,
                Some("sensitive-token"),
                true,
            )
            .await
            .unwrap();
        std::fs::write(path.join("sentinel"), "modified").unwrap();
        assert_eq!(
            std::fs::read_to_string(second.join("sentinel")).unwrap(),
            "committed"
        );
        let config = std::fs::read_to_string(second.join(".git/config")).unwrap();
        assert!(!config.contains("sensitive-token"));
        assert!(!config.contains("Authorization"));
        let service = GitService::new();
        service.stage_all(&path).await.unwrap();
        assert!(
            !service
                .commit(&path, "task change")
                .await
                .unwrap()
                .is_empty()
        );
        let author = std::process::Command::new("git")
            .args(["show", "-s", "--format=%an <%ae>"])
            .current_dir(&path)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8(author.stdout).unwrap().trim(),
            "Zone <zone@localhost>"
        );
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::os::unix::fs::PermissionsExt;

    /// These fixtures share the machine with every other test binary, and under
    /// llvm-cov the instrumentation slows all of it down. The budgets only bound
    /// how long a genuine regression takes to surface, so they are generous.
    const SPAWN_BUDGET: Duration = Duration::from_secs(30);
    const TEARDOWN_BUDGET: Duration = Duration::from_secs(10);

    async fn marker(directory: &Path, name: &str) -> u32 {
        tokio::time::timeout(SPAWN_BUDGET, async {
            loop {
                if let Ok(value) = tokio::fs::read_to_string(directory.join(name)).await
                    && let Ok(pid) = value.trim().parse()
                {
                    return pid;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture process did not start")
    }

    fn alive(pid: u32) -> bool {
        kill(Pid::from_raw(pid as i32), None).is_ok()
    }

    async fn cancellation(timeout: bool) {
        let fixture = tempfile::tempdir().unwrap();
        std::fs::write(
            fixture.path().join("git"),
            "#!/bin/sh\necho $$ > \"$FIXTURE/parent\"\n/bin/sh \"$FIXTURE/helper\" &\nwait\n",
        )
        .unwrap();
        std::fs::write(fixture.path().join("helper"), "#!/bin/sh\necho $$ > \"$FIXTURE/helper-pid\"\nprintf '%s' \"$GIT_CONFIG_VALUE_0\" > \"$FIXTURE/credential\"\n/bin/sh \"$FIXTURE/grandchild\" &\nwait\n").unwrap();
        std::fs::write(
            fixture.path().join("grandchild"),
            "#!/bin/sh\necho $$ > \"$FIXTURE/grandchild-pid\"\nexec /bin/sleep 60\n",
        )
        .unwrap();
        std::fs::set_permissions(
            fixture.path().join("git"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let mut unrelated = Command::new("/bin/sleep")
            .arg("60")
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut command = GitService::network_command(Some("fixture-credential"));
        command
            .env("PATH", fixture.path())
            .env("FIXTURE", fixture.path());
        let operation = tokio::spawn(async move { GitService::finish(&mut command).await });
        let parent = marker(fixture.path(), "parent").await;
        let helper = marker(fixture.path(), "helper-pid").await;
        let grandchild = marker(fixture.path(), "grandchild-pid").await;
        let credential = tokio::fs::read_to_string(fixture.path().join("credential"))
            .await
            .unwrap();
        assert!(credential.starts_with("Authorization: Basic "));
        if timeout {
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(301)).await;
            let result = operation.await.unwrap();
            tokio::time::resume();
            assert!(
                matches!(result, Err(GitError::CommandFailed(ref error)) if error.contains("timed out"))
            );
        } else {
            operation.abort();
            assert!(operation.await.unwrap_err().is_cancelled());
        }
        let stopped = tokio::time::timeout(TEARDOWN_BUDGET, async {
            while [parent, helper, grandchild].iter().any(|pid| alive(*pid)) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();
        let survivor = unrelated.try_wait().unwrap().is_none();
        // Clean up only this fixture's known processes, including on the seen-red path.
        for pid in [grandchild, helper, parent] {
            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
        }
        unrelated.kill().await.unwrap();
        tokio::time::timeout(TEARDOWN_BUDGET, async {
            while [parent, helper, grandchild].iter().any(|pid| alive(*pid)) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture cleanup left a Git helper running");
        assert!(
            survivor,
            "cancellation killed an unrelated process outside the Git group"
        );
        assert!(
            stopped,
            "credential-bearing helper or grandchild survived Git cancellation"
        );
    }

    #[tokio::test]
    async fn dropped_git_future_kills_helpers_and_preserves_unrelated_processes() {
        cancellation(false).await;
    }

    #[tokio::test]
    async fn timed_out_git_kills_helpers_and_preserves_unrelated_processes() {
        cancellation(true).await;
    }
}

#[cfg(test)]
mod publication_tests {
    use super::*;

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

    async fn forged_ancestry(legacy: bool) {
        let fixture = tempfile::tempdir().unwrap();
        git(fixture.path(), &["init", "--initial-branch=main"]);
        let service = GitService::new();
        std::fs::write(fixture.path().join("file"), "baseline").unwrap();
        service.stage_all(fixture.path()).await.unwrap();
        let baseline = service.commit(fixture.path(), "baseline").await.unwrap();
        git(fixture.path(), &["checkout", "--orphan", "task"]);
        std::fs::write(fixture.path().join("file"), "unrelated").unwrap();
        service.stage_all(fixture.path()).await.unwrap();
        let orphan = service.commit(fixture.path(), "orphan").await.unwrap();
        if legacy {
            std::fs::write(
                fixture.path().join(".git/info/grafts"),
                format!("{orphan} {baseline}\n"),
            )
            .unwrap();
        } else {
            git(fixture.path(), &["replace", "--graft", "HEAD", &baseline]);
        }
        assert!(
            !service
                .is_ancestor(fixture.path(), &baseline, "HEAD")
                .await
                .unwrap(),
            "task metadata forged baseline ancestry"
        );
        assert_eq!(
            service.revision(fixture.path(), "HEAD").await.unwrap(),
            orphan
        );
        assert_eq!(
            service
                .changed_files(fixture.path(), &baseline)
                .await
                .unwrap(),
            vec!["file"]
        );
    }

    #[tokio::test]
    async fn replacement_refs_do_not_supply_trusted_ancestry() {
        forged_ancestry(false).await;
    }

    #[tokio::test]
    async fn legacy_grafts_do_not_supply_trusted_ancestry() {
        forged_ancestry(true).await;
    }

    #[tokio::test]
    async fn replacement_objects_cannot_hide_staged_changes() {
        let fixture = tempfile::tempdir().unwrap();
        git(fixture.path(), &["init", "--initial-branch=main"]);
        let service = GitService::new();
        std::fs::write(fixture.path().join("file"), "baseline").unwrap();
        service.stage_all(fixture.path()).await.unwrap();
        let baseline = service.commit(fixture.path(), "baseline").await.unwrap();
        git(fixture.path(), &["checkout", "-b", "replacement"]);
        std::fs::write(fixture.path().join("file"), "changed").unwrap();
        service.stage_all(fixture.path()).await.unwrap();
        let replacement = service
            .commit(fixture.path(), "replacement tree")
            .await
            .unwrap();
        git(fixture.path(), &["checkout", "main"]);
        git(fixture.path(), &["replace", &baseline, &replacement]);
        std::fs::write(fixture.path().join("file"), "changed").unwrap();
        git(fixture.path(), &["--no-replace-objects", "add", "-A"]);
        assert!(
            service.has_changes(fixture.path()).await.unwrap(),
            "replacement tree hid staged task changes"
        );
        let committed = service
            .commit(fixture.path(), "real task changes")
            .await
            .unwrap();
        assert_ne!(committed, baseline);
        assert_eq!(
            service
                .changed_files(fixture.path(), &baseline)
                .await
                .unwrap(),
            vec!["file"]
        );
    }

    #[tokio::test]
    async fn branch_resumption_preserves_commits_and_normal_push_rejects_divergence() {
        let fixture = tempfile::tempdir().unwrap();
        let source = fixture.path().join("source");
        std::fs::create_dir(&source).unwrap();
        git(&source, &["init", "--initial-branch=main"]);
        std::fs::write(source.join("initial"), "base").unwrap();
        let service = GitService::new();
        service.stage_all(&source).await.unwrap();
        service.commit(&source, "initial").await.unwrap();
        let remote = fixture.path().join("remote.git");
        git(&source, &["clone", "--bare", ".", remote.to_str().unwrap()]);
        let first = fixture.path().join("first");
        service
            .clone_source(remote.to_str().unwrap(), &first, None, true)
            .await
            .unwrap();
        service
            .prepare_branch(&first, "zone/task-test", false)
            .await
            .unwrap();
        std::fs::write(first.join("first"), "first attempt").unwrap();
        service.stage_all(&first).await.unwrap();
        let first_commit = service.commit(&first, "first attempt").await.unwrap();
        service
            .push_source(
                &first,
                "zone/task-test",
                remote.to_str().unwrap(),
                "sensitive-token",
                true,
            )
            .await
            .unwrap();
        let second = fixture.path().join("second");
        service
            .clone_source(remote.to_str().unwrap(), &second, None, true)
            .await
            .unwrap();
        service
            .prepare_branch(&second, "zone/task-test", true)
            .await
            .unwrap();
        assert_eq!(
            service.revision(&second, "HEAD").await.unwrap(),
            first_commit
        );
        assert_eq!(
            std::fs::read_to_string(second.join("first")).unwrap(),
            "first attempt"
        );
        std::fs::write(second.join("second"), "second attempt").unwrap();
        service.stage_all(&second).await.unwrap();
        let second_commit = service.commit(&second, "second attempt").await.unwrap();
        service
            .push_source(
                &second,
                "zone/task-test",
                remote.to_str().unwrap(),
                "sensitive-token",
                true,
            )
            .await
            .unwrap();
        assert_eq!(
            git(&remote, &["rev-parse", "zone/task-test"]),
            second_commit
        );
        std::fs::write(first.join("concurrent"), "stale checkout").unwrap();
        service.stage_all(&first).await.unwrap();
        service.commit(&first, "concurrent attempt").await.unwrap();
        let failure = service
            .push_source(
                &first,
                "zone/task-test",
                remote.to_str().unwrap(),
                "sensitive-token",
                true,
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(failure.contains("Git push rejected"), "{failure}");
        assert!(
            !failure.contains("sensitive-token") && !failure.contains(remote.to_str().unwrap())
        );
        assert_eq!(
            git(&remote, &["rev-parse", "zone/task-test"]),
            second_commit
        );
        assert!(
            !std::fs::read_to_string(first.join(".git/config"))
                .unwrap()
                .contains("sensitive-token")
        );
        assert!(
            service
                .is_ancestor(&second, &first_commit, "HEAD")
                .await
                .unwrap()
        );
    }
    /// A branch an earlier run's kept worktree still holds is refused with
    /// the reason, not with git's "already exists".
    #[tokio::test]
    async fn a_branch_a_kept_worktree_holds_is_refused_with_the_reason() {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        crate::worktree::fixtures::remote(&remote);
        let base = root.path().join("base");
        crate::worktree::fixtures::clone(&remote, &base);
        let first = root.path().join("first");
        crate::worktree::add(&base, &first, "origin/HEAD").unwrap();
        let service = GitService::new();
        service
            .prepare_branch(&first, "zone/task", false)
            .await
            .unwrap();
        assert_eq!(service.current_branch(&first).await.unwrap(), "zone/task");
        assert!(
            service
                .prepare_branch(&first, "zone/task", false)
                .await
                .is_ok(),
            "the worktree already on the branch is prepared again without complaint"
        );
        let second = root.path().join("second");
        crate::worktree::add(&base, &second, "origin/HEAD").unwrap();
        let refused = service
            .prepare_branch(&second, "zone/task", false)
            .await
            .unwrap_err()
            .to_string();
        assert!(
            refused.contains("earlier run's worktree still holds"),
            "{refused}"
        );
    }

    /// Two remotes that look alike, one of which a run pointed the clone at.
    /// A fetch is bound to the URL Zone gives it, and prunes what that
    /// remote no longer has.
    #[tokio::test]
    async fn a_fetch_goes_to_the_url_it_is_given_and_not_where_the_clone_was_pointed() {
        let root = tempfile::tempdir().unwrap();
        let genuine = root.path().join("genuine");
        std::fs::create_dir(&genuine).unwrap();
        crate::worktree::fixtures::remote(&genuine);
        let rogue = root.path().join("rogue");
        std::fs::create_dir(&rogue).unwrap();
        crate::worktree::fixtures::remote(&rogue);
        std::fs::write(rogue.join("ROGUE"), "planted\n").unwrap();
        crate::worktree::fixtures::git(&rogue, &["add", "ROGUE"]);
        crate::worktree::fixtures::git(&rogue, &["commit", "-q", "-m", "rogue"]);
        let base = root.path().join("base");
        crate::worktree::fixtures::clone(&genuine, &base);
        std::fs::write(genuine.join("NEW"), "moved on\n").unwrap();
        crate::worktree::fixtures::git(&genuine, &["add", "NEW"]);
        crate::worktree::fixtures::git(&genuine, &["commit", "-q", "-m", "moved on"]);
        crate::worktree::fixtures::git(&genuine, &["branch", "gone"]);
        // A run redirected the clone's remote.
        crate::worktree::fixtures::git(
            &base,
            &["config", "remote.origin.url", rogue.to_str().unwrap()],
        );
        let service = GitService::new();
        service
            .fetch_source(&base, genuine.to_str().unwrap(), None, true)
            .await
            .unwrap();
        assert_eq!(
            crate::worktree::fixtures::git(&base, &["rev-parse", "refs/remotes/origin/main"]),
            crate::worktree::fixtures::git(&genuine, &["rev-parse", "main"]),
            "the genuine remote's tip, not the rogue's"
        );
        assert_eq!(
            crate::worktree::fixtures::git(&base, &["rev-parse", "refs/remotes/origin/gone"]),
            crate::worktree::fixtures::git(&genuine, &["rev-parse", "gone"])
        );
        crate::worktree::fixtures::git(&genuine, &["branch", "-D", "gone"]);
        service
            .fetch_source(&base, genuine.to_str().unwrap(), None, true)
            .await
            .unwrap();
        let refs = crate::worktree::fixtures::git(&base, &["for-each-ref", "refs/remotes/origin/"]);
        assert!(!refs.contains("origin/gone"), "{refs}");
    }

    /// The default branch and its tip come from the remote; a redirected
    /// `origin/HEAD` in the clone changes nothing about the answer, and the
    /// ref is set back to the branch the remote named.
    #[tokio::test]
    async fn the_remote_head_is_asked_of_the_remote_and_not_read_from_a_shared_ref() {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        crate::worktree::fixtures::remote(&remote);
        let base = root.path().join("base");
        crate::worktree::fixtures::clone(&remote, &base);
        crate::worktree::fixtures::git(
            &base,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/planted",
            ],
        );
        let service = GitService::new();
        let head = service
            .remote_head_source(&base, remote.to_str().unwrap(), None, true)
            .await
            .unwrap();
        assert_eq!(head.branch, "main");
        assert_eq!(
            head.commit,
            crate::worktree::fixtures::git(&remote, &["rev-parse", "main"])
        );
        assert!(service.has_commit(&base, &head.commit).await.unwrap());
        assert!(!service.has_commit(&base, &"0".repeat(40)).await.unwrap());
        assert!(
            service.has_commit(&base, "HEAD").await.is_err(),
            "a name is not a commit"
        );
        service.set_remote_head(&base, &head.branch).await.unwrap();
        assert_eq!(
            crate::worktree::fixtures::git(&base, &["symbolic-ref", "refs/remotes/origin/HEAD"]),
            "refs/remotes/origin/main"
        );
        assert!(service.set_remote_head(&base, "-x").await.is_err());
    }

    /// What a run planted in the clone's configuration is gone once it is
    /// reset, what git chose for the file system stays, and the remote is
    /// the one Zone knows.
    #[tokio::test]
    async fn resetting_the_configuration_drops_what_a_run_planted_and_keeps_the_remote() {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        crate::worktree::fixtures::remote(&remote);
        let base = root.path().join("base");
        crate::worktree::fixtures::clone(&remote, &base);
        crate::worktree::fixtures::git(&base, &["config", "core.fsmonitor", "/bin/false"]);
        crate::worktree::fixtures::git(
            &base,
            &[
                "config",
                "url.https://evil.example/.insteadOf",
                "https://github.com/",
            ],
        );
        crate::worktree::fixtures::git(
            &base,
            &["config", "remote.origin.url", "https://evil.example/x.git"],
        );
        crate::worktree::fixtures::git(&base, &["config", "filter.planted.smudge", "/bin/false"]);
        let service = GitService::new();
        service
            .reset_config(&base, "https://github.com/fixture/repository.git")
            .await
            .unwrap();
        let listed =
            crate::worktree::fixtures::git(&base, &["config", "--file", ".git/config", "--list"]);
        assert!(!listed.contains("fsmonitor"), "{listed}");
        assert!(!listed.contains("insteadof"), "{listed}");
        assert!(!listed.contains("filter."), "{listed}");
        assert!(!listed.contains("evil"), "{listed}");
        assert!(
            listed.contains("remote.origin.url=https://github.com/fixture/repository.git"),
            "{listed}"
        );
        assert!(
            listed.contains("remote.origin.fetch=+refs/heads/*:refs/remotes/origin/*"),
            "{listed}"
        );
        assert!(listed.contains("core.filemode="), "{listed}");
        assert_eq!(
            crate::worktree::fixtures::git(&base, &["status", "--porcelain"]),
            "",
            "the clone still works"
        );
        assert!(
            service
                .reset_config(&base, "https://github.com/x/y.git\n[core]")
                .await
                .is_err()
        );
    }

    /// A branch that no worktree holds — left behind by a removal whose
    /// deletion failed, or by a worktree deleted by hand — is taken over by
    /// the next run that asks for it.
    #[tokio::test]
    async fn a_branch_no_worktree_holds_is_taken_over_by_the_next_run() {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        crate::worktree::fixtures::remote(&remote);
        let base = root.path().join("base");
        crate::worktree::fixtures::clone(&remote, &base);
        let service = GitService::new();

        let first = root.path().join("first");
        crate::worktree::add(&base, &first, "origin/HEAD").unwrap();
        service
            .prepare_branch(&first, "zone/task", false)
            .await
            .unwrap();
        crate::worktree::fixtures::git(
            &base,
            &["worktree", "remove", "--force", first.to_str().unwrap()],
        );
        assert!(
            crate::worktree::fixtures::git(&base, &["branch", "--list", "zone/task"])
                .contains("zone/task"),
            "the branch outlived its worktree"
        );
        let second = root.path().join("second");
        crate::worktree::add(&base, &second, "origin/HEAD").unwrap();
        service
            .prepare_branch(&second, "zone/task", false)
            .await
            .unwrap();
        assert_eq!(service.current_branch(&second).await.unwrap(), "zone/task");

        let third = root.path().join("third");
        crate::worktree::add(&base, &third, "origin/HEAD").unwrap();
        service
            .prepare_branch(&third, "zone/other", false)
            .await
            .unwrap();
        std::fs::remove_dir_all(&third).unwrap();
        let fourth = root.path().join("fourth");
        crate::worktree::add(&base, &fourth, "origin/HEAD").unwrap();
        service
            .prepare_branch(&fourth, "zone/other", false)
            .await
            .unwrap();
        assert_eq!(service.current_branch(&fourth).await.unwrap(), "zone/other");
    }
}
