//! Git operations service
//!
//! Provides git operations for PR creation workflow:
//! - Branch creation with task-based naming
//! - Staging and committing changes
//! - Pushing to remote

use std::path::Path;
use std::process::Stdio;
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

impl GitService {
    /// Create a new git service
    pub fn new() -> Self {
        Self {
            max_branch_length: 100,
        }
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
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--is-inside-work-tree")
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        Ok(output.status.success())
    }

    /// Get the current branch name
    pub async fn current_branch(&self, path: &Path) -> GitResult<String> {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Check if there are uncommitted changes
    pub async fn has_changes(&self, path: &Path) -> GitResult<bool> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
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
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
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
        let diff_stat_output = Command::new("git")
            .args(["diff", "--shortstat", "HEAD"])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
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
                } else if part.contains("deletion") {
                    if let Some(num) = part.split_whitespace().next() {
                        deletions = num.parse().unwrap_or(0);
                    }
                }
            }
        }

        // Get actual diff text (limited)
        let diff_output = Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
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
        let check_output = Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{}", branch_name),
            ])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if check_output.status.success() {
            return Err(GitError::BranchExists(branch_name.to_string()));
        }

        // Create and checkout the branch
        let output = Command::new("git")
            .args(["checkout", "-b", branch_name])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Stage all changes
    pub async fn stage_all(&self, path: &Path) -> GitResult<()> {
        let output = Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Commit staged changes
    pub async fn commit(&self, path: &Path, message: &str) -> GitResult<String> {
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("nothing to commit") {
                return Err(GitError::NoChanges);
            }
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        // Get the commit SHA
        let sha_output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        Ok(String::from_utf8_lossy(&sha_output.stdout)
            .trim()
            .to_string())
    }

    /// Push branch to remote
    pub async fn push(&self, path: &Path, branch_name: &str, remote: &str) -> GitResult<()> {
        let output = Command::new("git")
            .args(["push", "-u", remote, branch_name])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
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
        // Parse the remote URL and inject token
        let authenticated_url = inject_token_into_url(remote_url, token)?;

        // Add/update remote with authenticated URL
        // First remove if exists, then add
        let _ = Command::new("git")
            .args(["remote", "remove", "zone-push"])
            .current_dir(path)
            .output()
            .await;

        let add_output = Command::new("git")
            .args(["remote", "add", "zone-push", &authenticated_url])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !add_output.status.success() {
            let stderr = String::from_utf8_lossy(&add_output.stderr);
            // Ignore "already exists" error
            if !stderr.contains("already exists") {
                return Err(GitError::CommandFailed(stderr.to_string()));
            }
        }

        // Push to the authenticated remote
        let push_output = Command::new("git")
            .args(["push", "-u", "zone-push", branch_name])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        // Clean up the remote with token
        let _ = Command::new("git")
            .args(["remote", "remove", "zone-push"])
            .current_dir(path)
            .output()
            .await;

        if !push_output.status.success() {
            let stderr = String::from_utf8_lossy(&push_output.stderr);
            if stderr.contains("Authentication failed")
                || stderr.contains("could not read Username")
            {
                return Err(GitError::AuthFailed);
            }
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }

    /// Get the default remote URL
    pub async fn get_remote_url(&self, path: &Path, remote: &str) -> GitResult<String> {
        let output = Command::new("git")
            .args(["remote", "get-url", remote])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            return Err(GitError::NoRemote);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Checkout existing branch
    pub async fn checkout(&self, path: &Path, branch_name: &str) -> GitResult<()> {
        let output = Command::new("git")
            .args(["checkout", branch_name])
            .current_dir(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::CommandFailed(stderr.to_string()));
        }

        Ok(())
    }
}

/// Inject authentication token into a git URL
fn inject_token_into_url(url: &str, token: &str) -> GitResult<String> {
    // Handle HTTPS URLs: https://github.com/owner/repo.git
    if let Some(without_scheme) = url.strip_prefix("https://") {
        // Insert x-access-token:token@ after the scheme
        return Ok(format!(
            "https://x-access-token:{}@{}",
            token, without_scheme
        ));
    }

    // Handle SSH URLs - convert to HTTPS with token
    // git@github.com:owner/repo.git -> https://x-access-token:token@github.com/owner/repo.git
    if let Some(stripped) = url.strip_prefix("git@") {
        let parts: Vec<&str> = stripped.splitn(2, ':').collect();
        if parts.len() == 2 {
            let host = parts[0];
            let path = parts[1];
            return Ok(format!(
                "https://x-access-token:{}@{}/{}",
                token, host, path
            ));
        }
    }

    Err(GitError::CommandFailed(format!(
        "Unsupported URL format: {}",
        url
    )))
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

    #[test]
    fn test_inject_token_https() {
        let url = "https://github.com/owner/repo.git";
        let token = "ghp_test123";
        let result = inject_token_into_url(url, token).unwrap();
        assert_eq!(
            result,
            "https://x-access-token:ghp_test123@github.com/owner/repo.git"
        );
    }

    #[test]
    fn test_inject_token_ssh() {
        let url = "git@github.com:owner/repo.git";
        let token = "ghp_test123";
        let result = inject_token_into_url(url, token).unwrap();
        assert_eq!(
            result,
            "https://x-access-token:ghp_test123@github.com/owner/repo.git"
        );
    }

    #[test]
    fn test_inject_token_invalid_url() {
        let url = "invalid://url";
        let token = "test";
        let result = inject_token_into_url(url, token);
        assert!(result.is_err());
    }
}
