//! Git operations service
//!
//! Provides git operations for PR creation workflow:
//! - Branch creation with task-based naming
//! - Staging and committing changes
//! - Pushing to remote

use base64::Engine;
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
        let mut command = Command::new("git");
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
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
        // Limit diff text size
        let diff_text = if diff_text.len() > 50_000 {
            format!("{}...[truncated]", &diff_text[..50_000])
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
        let mut command = Self::network_command(Some(token));
        command
            .args(["push", "--", &remote, branch_name])
            .current_dir(path);
        Self::finish(&mut command).await
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

    async fn marker(directory: &Path, name: &str) -> u32 {
        tokio::time::timeout(Duration::from_secs(5), async {
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
        let stopped = tokio::time::timeout(Duration::from_secs(2), async {
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
        tokio::time::timeout(Duration::from_secs(2), async {
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
