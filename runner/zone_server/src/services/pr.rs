//! Pull request creation service
//!
//! Creates pull requests on GitHub when a task completes with code changes.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// PR service errors
#[derive(Debug, Error)]
pub enum PrError {
    #[error("GitHub API error: {0}")]
    GitHubApi(String),

    #[error("Repository not configured")]
    NoRepository,

    #[error("Authentication failed")]
    AuthFailed,

    #[error("Branch not found: {0}")]
    BranchNotFound(String),

    #[error("PR already exists for branch: {0}")]
    PrAlreadyExists(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Invalid repository URL: {0}")]
    InvalidRepoUrl(String),
}

pub type PrResult<T> = Result<T, PrError>;

/// GitHub pull request response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubPullRequest {
    pub id: i64,
    pub number: i64,
    pub html_url: String,
    pub state: String,
    pub title: String,
    pub body: Option<String>,
    pub head: GitHubBranch,
    pub base: GitHubBranch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubBranch {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub sha: String,
}

/// Request to create a pull request
#[derive(Debug, Clone, Serialize)]
struct CreatePrRequest {
    title: String,
    body: String,
    head: String,
    base: String,
    draft: bool,
}

/// Created PR result
#[derive(Debug, Clone)]
pub struct CreatedPr {
    pub url: String,
    pub number: i64,
    pub state: String,
}

/// PR service for creating pull requests
#[derive(Debug, Clone)]
pub struct PrService {
    client: Client,
    base_url: String,
}

impl Default for PrService {
    fn default() -> Self {
        Self::new()
    }
}

impl PrService {
    /// Create a new PR service
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: "https://api.github.com".to_string(),
        }
    }

    /// Create with custom base URL (for testing)
    #[cfg(test)]
    pub fn with_base_url(base_url: String) -> Self {
        Self {
            client: Client::new(),
            base_url,
        }
    }

    /// Parse owner and repo from a GitHub URL
    ///
    /// Supports formats:
    /// - https://github.com/owner/repo
    /// - https://github.com/owner/repo.git
    /// - git@github.com:owner/repo.git
    pub fn parse_github_url(&self, url: &str) -> PrResult<(String, String)> {
        // Handle HTTPS URLs
        if let Some(path) = url.strip_prefix("https://github.com/") {
            let path = path.trim_end_matches(".git");
            let parts: Vec<&str> = path.splitn(2, '/').collect();
            if parts.len() == 2 {
                return Ok((parts[0].to_string(), parts[1].to_string()));
            }
        }

        // Handle SSH URLs
        if let Some(path) = url.strip_prefix("git@github.com:") {
            let path = path.trim_end_matches(".git");
            let parts: Vec<&str> = path.splitn(2, '/').collect();
            if parts.len() == 2 {
                return Ok((parts[0].to_string(), parts[1].to_string()));
            }
        }

        Err(PrError::InvalidRepoUrl(url.to_string()))
    }

    /// Create a pull request on GitHub
    pub async fn create_pr(
        &self,
        owner: &str,
        repo: &str,
        token: &str,
        head_branch: &str,
        base_branch: &str,
        title: &str,
        body: &str,
        draft: bool,
    ) -> PrResult<CreatedPr> {
        let url = format!("{}/repos/{}/{}/pulls", self.base_url, owner, repo);

        let request = CreatePrRequest {
            title: title.to_string(),
            body: body.to_string(),
            head: head_branch.to_string(),
            base: base_branch.to_string(),
            draft,
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "zone-pr-service")
            .json(&request)
            .send()
            .await?;

        let status = response.status();

        if status.is_success() {
            let pr: GitHubPullRequest = response.json().await?;
            return Ok(CreatedPr {
                url: pr.html_url,
                number: pr.number,
                state: pr.state,
            });
        }

        // Handle error responses
        let error_text = response.text().await.unwrap_or_default();

        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(PrError::AuthFailed);
        }

        if status == reqwest::StatusCode::NOT_FOUND {
            return Err(PrError::BranchNotFound(head_branch.to_string()));
        }

        if status == reqwest::StatusCode::UNPROCESSABLE_ENTITY
            && error_text.contains("A pull request already exists")
        {
            return Err(PrError::PrAlreadyExists(head_branch.to_string()));
        }

        Err(PrError::GitHubApi(format!(
            "GitHub API returned {}: {}",
            status, error_text
        )))
    }

    /// Generate a PR title from a task
    pub fn generate_pr_title(&self, task_title: &str, task_id: Uuid) -> String {
        let short_id = &task_id.to_string()[..8];
        format!("[Zone] {} ({})", task_title, short_id)
    }

    /// Generate a PR body from task details
    pub fn generate_pr_body(
        &self,
        task_title: &str,
        task_description: &str,
        task_id: Uuid,
        diff_summary: Option<&str>,
        zone_task_url: Option<&str>,
    ) -> String {
        let mut body = String::new();

        // Summary section
        body.push_str("## Summary\n\n");
        body.push_str(&format!("**Task:** {}\n\n", task_title));
        body.push_str(&format!("{}\n\n", task_description));

        // Zone link
        if let Some(url) = zone_task_url {
            body.push_str(&format!("**Zone Task:** [View in Zone]({})\n\n", url));
        } else {
            body.push_str(&format!("**Zone Task ID:** `{}`\n\n", task_id));
        }

        // Changes section
        if let Some(diff) = diff_summary {
            body.push_str("## Changes\n\n");
            body.push_str(diff);
            body.push_str("\n\n");
        }

        // Footer
        body.push_str("---\n");
        body.push_str("*This PR was automatically created by Zone after task completion.*\n");

        body
    }

    /// Get the default branch for a repository
    pub async fn get_default_branch(
        &self,
        owner: &str,
        repo: &str,
        token: &str,
    ) -> PrResult<String> {
        let url = format!("{}/repos/{}/{}", self.base_url, owner, repo);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "zone-pr-service")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(PrError::GitHubApi(format!(
                "Failed to get repo info: {} {}",
                status, error_text
            )));
        }

        #[derive(Deserialize)]
        struct RepoInfo {
            default_branch: String,
        }

        let repo_info: RepoInfo = response.json().await?;
        Ok(repo_info.default_branch)
    }

    /// Check if a PR already exists for a branch
    pub async fn pr_exists_for_branch(
        &self,
        owner: &str,
        repo: &str,
        token: &str,
        head_branch: &str,
    ) -> PrResult<Option<String>> {
        let url = format!(
            "{}/repos/{}/{}/pulls?head={}:{}&state=open",
            self.base_url, owner, repo, owner, head_branch
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "zone-pr-service")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(PrError::GitHubApi(format!(
                "Failed to check PRs: {} {}",
                status, error_text
            )));
        }

        let prs: Vec<GitHubPullRequest> = response.json().await?;

        if let Some(pr) = prs.first() {
            Ok(Some(pr.html_url.clone()))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_https_url() {
        let service = PrService::new();
        let (owner, repo) = service
            .parse_github_url("https://github.com/acme/project")
            .unwrap();
        assert_eq!(owner, "acme");
        assert_eq!(repo, "project");
    }

    #[test]
    fn test_parse_github_https_url_with_git() {
        let service = PrService::new();
        let (owner, repo) = service
            .parse_github_url("https://github.com/acme/project.git")
            .unwrap();
        assert_eq!(owner, "acme");
        assert_eq!(repo, "project");
    }

    #[test]
    fn test_parse_github_ssh_url() {
        let service = PrService::new();
        let (owner, repo) = service
            .parse_github_url("git@github.com:acme/project.git")
            .unwrap();
        assert_eq!(owner, "acme");
        assert_eq!(repo, "project");
    }

    #[test]
    fn test_parse_invalid_url() {
        let service = PrService::new();
        let result = service.parse_github_url("not-a-github-url");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_pr_title() {
        let service = PrService::new();
        let task_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();
        let title = service.generate_pr_title("Fix login bug", task_id);
        assert!(title.contains("[Zone]"));
        assert!(title.contains("Fix login bug"));
        assert!(title.contains("12345678"));
    }

    #[test]
    fn test_generate_pr_body() {
        let service = PrService::new();
        let task_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let body = service.generate_pr_body(
            "Fix login bug",
            "Fixed the authentication issue",
            task_id,
            Some("- 3 files changed"),
            Some("https://zone.example.com/tasks/123"),
        );

        assert!(body.contains("## Summary"));
        assert!(body.contains("Fix login bug"));
        assert!(body.contains("View in Zone"));
        assert!(body.contains("## Changes"));
    }

    #[test]
    fn test_generate_pr_body_no_zone_url() {
        let service = PrService::new();
        let task_id = Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap();

        let body =
            service.generate_pr_body("Fix login bug", "Fixed the issue", task_id, None, None);

        assert!(body.contains("Zone Task ID"));
        assert!(body.contains(&task_id.to_string()));
    }
}
