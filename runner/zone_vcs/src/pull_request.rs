//! Pull request creation service
//!
//! Creates pull requests on GitHub when a task completes with code changes, and
//! reads back how each one was received: when it merged, how many rounds of
//! review it took, who approved it, and what the reviewers actually said.

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Rows GitHub returns per page; its maximum for these collections.
const PAGE_SIZE: usize = 100;

/// Pages a paged read will follow before it stops. A pull request with more
/// review activity than this has long since stopped teaching anything new, and
/// an unbounded walk would let one pathological change stall the sync.
const MAXIMUM_PAGES: usize = 10;

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

/// Where a pull request lives, recovered from the URL a run recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestReference {
    pub owner: String,
    pub repository: String,
    pub number: i64,
}

impl PullRequestReference {
    /// Parse `https://github.com/owner/repo/pull/123`, with or without a trailing
    /// segment such as `/files` that a person's copied link often carries.
    pub fn parse(url: &str) -> PrResult<Self> {
        let trimmed = url.trim();
        let path = trimmed
            .strip_prefix("https://github.com/")
            .or_else(|| trimmed.strip_prefix("http://github.com/"))
            .ok_or_else(|| PrError::InvalidRepoUrl(url.to_string()))?;

        let mut segments = path.split('/');
        let owner = segments.next().unwrap_or_default();
        let repository = segments.next().unwrap_or_default();
        let marker = segments.next().unwrap_or_default();
        let number = segments.next().unwrap_or_default();

        if owner.is_empty() || repository.is_empty() || marker != "pull" {
            return Err(PrError::InvalidRepoUrl(url.to_string()));
        }

        let number: i64 = number
            .parse()
            .map_err(|_| PrError::InvalidRepoUrl(url.to_string()))?;
        if number < 1 {
            return Err(PrError::InvalidRepoUrl(url.to_string()));
        }

        Ok(Self {
            owner: owner.to_string(),
            repository: repository.trim_end_matches(".git").to_string(),
            number,
        })
    }
}

/// The verdict a reviewer submitted with a review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
    Unknown,
}

impl ReviewState {
    pub fn parse(text: &str) -> Self {
        match text.trim().to_ascii_uppercase().as_str() {
            "APPROVED" => ReviewState::Approved,
            "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
            "COMMENTED" => ReviewState::Commented,
            "DISMISSED" => ReviewState::Dismissed,
            "PENDING" => ReviewState::Pending,
            _ => ReviewState::Unknown,
        }
    }

    /// Whether this verdict replaces the reviewer's previous one.
    fn settles(self) -> bool {
        matches!(
            self,
            ReviewState::Approved | ReviewState::ChangesRequested | ReviewState::Dismissed
        )
    }
}

/// One submitted review, reduced to the two things the tally needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmittedReview {
    pub state: ReviewState,
    pub reviewer: Option<String>,
}

/// What the submitted reviews add up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReviewTally {
    pub cycles: u32,
    pub approvals: u32,
}

/// A round of review is a reviewer sending the change back, so every
/// `CHANGES_REQUESTED` costs a cycle. Approvals count distinct people whose
/// latest verdict was an approval: approving twice is still one approval, and a
/// reviewer who later requests changes has withdrawn theirs.
pub fn tally(reviews: &[SubmittedReview]) -> ReviewTally {
    let mut cycles = 0u32;
    let mut latest: Vec<(String, ReviewState)> = Vec::new();
    let mut unattributed = 0u32;

    for review in reviews {
        if review.state == ReviewState::ChangesRequested {
            cycles = cycles.saturating_add(1);
        }

        let reviewer = review
            .reviewer
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());

        match reviewer {
            Some(name) if review.state.settles() => {
                match latest.iter_mut().find(|(known, _)| known == name) {
                    Some(entry) => entry.1 = review.state,
                    None => latest.push((name.to_string(), review.state)),
                }
            }
            None if review.state == ReviewState::Approved => {
                unattributed = unattributed.saturating_add(1);
            }
            _ => {}
        }
    }

    let named = latest
        .iter()
        .filter(|(_, state)| *state == ReviewState::Approved)
        .count();

    ReviewTally {
        cycles,
        approvals: u32::try_from(named)
            .unwrap_or(u32::MAX)
            .saturating_add(unattributed),
    }
}

/// Whole minutes between two RFC 3339 instants, or `None` if either is unreadable.
pub fn minutes_between(opened: &str, merged: &str) -> Option<i64> {
    let opened = DateTime::parse_from_rfc3339(opened).ok()?;
    let merged = DateTime::parse_from_rfc3339(merged).ok()?;
    Some((merged.with_timezone(&Utc) - opened.with_timezone(&Utc)).num_minutes())
}

/// How a change was received once the agent handed it over.
///
/// Timestamps stay RFC 3339 and tallies stay plain counts, because this is read
/// back out of a run's artifacts by the learning loop rather than by a person.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PullRequestReception {
    pub opened_at: Option<String>,
    pub merged_at: Option<String>,
    pub minutes_to_merge: Option<i64>,
    pub review_cycles: u32,
    pub approvals: u32,
    pub state: Option<String>,
    pub comments: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubUser {
    #[serde(default)]
    login: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubReview {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    user: Option<GitHubUser>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubComment {
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubPullRequestDetail {
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    merged_at: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    mergeable: Option<bool>,
}

/// Whether a pull request's branch still merges with its base.
///
/// GitHub computes this in the background and answers `null` until it has,
/// which is a third answer rather than a missing one: a branch nobody has
/// checked yet is not a branch known to conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mergeability {
    Clean,
    Conflicted,
    Unknown,
}

impl Mergeability {
    fn from_flag(flag: Option<bool>) -> Self {
        match flag {
            Some(true) => Mergeability::Clean,
            Some(false) => Mergeability::Conflicted,
            None => Mergeability::Unknown,
        }
    }

    pub fn conflicted(self) -> bool {
        self == Mergeability::Conflicted
    }
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

    /// Create a client for an explicitly supplied API endpoint.
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

    async fn get<T: serde::de::DeserializeOwned>(&self, url: &str, token: &str) -> PrResult<T> {
        let response = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "zone-pr-service")
            .send()
            .await?;

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(PrError::AuthFailed);
        }
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(PrError::GitHubApi(format!(
                "GitHub API returned {}: {}",
                status, error_text
            )));
        }

        Ok(response.json().await?)
    }

    async fn get_all<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        token: &str,
    ) -> PrResult<Vec<T>> {
        let mut collected: Vec<T> = Vec::new();

        for page in 1..=MAXIMUM_PAGES {
            let url = format!(
                "{}/{}?per_page={}&page={}",
                self.base_url, path, PAGE_SIZE, page
            );
            let batch: Vec<T> = self.get(&url, token).await?;
            let complete = batch.len() < PAGE_SIZE;
            collected.extend(batch);
            if complete {
                break;
            }
        }

        Ok(collected)
    }

    /// Ask GitHub whether a pull request's branch still merges with its base.
    ///
    /// One read, so a caller can skip the expensive work of reproducing a merge
    /// for the overwhelming majority of branches that do not conflict.
    pub async fn fetch_mergeability(
        &self,
        reference: &PullRequestReference,
        token: &str,
    ) -> PrResult<Mergeability> {
        let detail: GitHubPullRequestDetail = self
            .get(
                &format!(
                    "{}/repos/{}/{}/pulls/{}",
                    self.base_url, reference.owner, reference.repository, reference.number
                ),
                token,
            )
            .await?;

        Ok(Mergeability::from_flag(detail.mergeable))
    }

    /// Read back how a pull request was received.
    ///
    /// Three reads: the pull request itself for the opening and merge times, its
    /// reviews for the cycle and approval tallies, and its inline review comments.
    /// Review bodies count as comments too, because a reviewer who requests changes
    /// with a paragraph of reasoning is saying the same thing as one who writes it
    /// on a line.
    pub async fn fetch_reception(
        &self,
        reference: &PullRequestReference,
        token: &str,
    ) -> PrResult<PullRequestReception> {
        let scope = format!("repos/{}/{}", reference.owner, reference.repository);

        let detail: GitHubPullRequestDetail = self
            .get(
                &format!("{}/{}/pulls/{}", self.base_url, scope, reference.number),
                token,
            )
            .await?;

        let reviews: Vec<GitHubReview> = self
            .get_all(
                &format!("{}/pulls/{}/reviews", scope, reference.number),
                token,
            )
            .await?;

        let inline: Vec<GitHubComment> = self
            .get_all(
                &format!("{}/pulls/{}/comments", scope, reference.number),
                token,
            )
            .await?;

        let submitted: Vec<SubmittedReview> = reviews
            .iter()
            .map(|review| SubmittedReview {
                state: ReviewState::parse(review.state.as_deref().unwrap_or_default()),
                reviewer: review.user.as_ref().and_then(|user| user.login.clone()),
            })
            .collect();
        let tallied = tally(&submitted);

        let comments: Vec<String> = reviews
            .iter()
            .filter_map(|review| review.body.as_deref())
            .chain(inline.iter().filter_map(|comment| comment.body.as_deref()))
            .map(|body| body.trim().to_string())
            .filter(|body| !body.is_empty())
            .collect();

        let opened_at = detail.created_at.filter(|value| !value.is_empty());
        let merged_at = detail.merged_at.filter(|value| !value.is_empty());
        let minutes_to_merge = match (&opened_at, &merged_at) {
            (Some(opened), Some(merged)) => minutes_between(opened, merged),
            _ => None,
        };

        Ok(PullRequestReception {
            opened_at,
            merged_at,
            minutes_to_merge,
            review_cycles: tallied.cycles,
            approvals: tallied.approvals,
            state: detail.state.filter(|value| !value.is_empty()),
            comments,
        })
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

    fn review(state: &str, reviewer: Option<&str>) -> SubmittedReview {
        SubmittedReview {
            state: ReviewState::parse(state),
            reviewer: reviewer.map(str::to_string),
        }
    }

    #[test]
    fn a_pull_request_url_yields_its_owner_repository_and_number() {
        let reference = PullRequestReference::parse("https://github.com/acme/project/pull/42")
            .expect("a plain pull request URL must parse");
        assert_eq!(reference.owner, "acme");
        assert_eq!(reference.repository, "project");
        assert_eq!(reference.number, 42);
    }

    #[test]
    fn a_pull_request_url_parses_past_a_trailing_tab_segment() {
        let reference =
            PullRequestReference::parse("https://github.com/acme/project/pull/42/files").unwrap();
        assert_eq!(reference.number, 42);
    }

    #[test]
    fn a_url_that_is_not_a_pull_request_is_refused() {
        for url in [
            "https://github.com/acme/project",
            "https://github.com/acme/project/issues/42",
            "https://github.com/acme/project/pull/zero",
            "https://github.com/acme/project/pull/0",
            "https://example.test/acme/project/pull/42",
            "",
        ] {
            assert!(
                PullRequestReference::parse(url).is_err(),
                "{url} is not a pull request and must not parse as one"
            );
        }
    }

    #[test]
    fn every_request_for_changes_costs_a_review_cycle() {
        let tallied = tally(&[
            review("CHANGES_REQUESTED", Some("ada")),
            review("COMMENTED", Some("grace")),
            review("CHANGES_REQUESTED", Some("grace")),
        ]);
        assert_eq!(tallied.cycles, 2);
    }

    #[test]
    fn a_reviewer_who_approves_twice_is_still_one_approval() {
        let tallied = tally(&[
            review("APPROVED", Some("ada")),
            review("APPROVED", Some("ada")),
        ]);
        assert_eq!(tallied.approvals, 1);
        assert_eq!(tallied.cycles, 0);
    }

    #[test]
    fn an_approval_withdrawn_by_a_later_request_for_changes_stops_counting() {
        let tallied = tally(&[
            review("APPROVED", Some("ada")),
            review("CHANGES_REQUESTED", Some("ada")),
        ]);
        assert_eq!(
            tallied.approvals, 0,
            "a reviewer who came back asking for changes has withdrawn their approval"
        );
        assert_eq!(tallied.cycles, 1);
    }

    #[test]
    fn a_comment_only_review_is_neither_a_cycle_nor_an_approval() {
        let tallied = tally(&[review("COMMENTED", Some("ada")), review("PENDING", None)]);
        assert_eq!(tallied, ReviewTally::default());
    }

    #[test]
    fn an_unknown_review_state_is_counted_as_neither() {
        let tallied = tally(&[review("SOMETHING_NEW", Some("ada"))]);
        assert_eq!(tallied, ReviewTally::default());
        assert_eq!(ReviewState::parse("something_new"), ReviewState::Unknown);
    }

    #[test]
    fn merge_minutes_are_whole_and_reject_unreadable_timestamps() {
        assert_eq!(
            minutes_between("2026-09-04T09:00:00Z", "2026-09-04T11:30:00Z"),
            Some(150)
        );
        assert_eq!(
            minutes_between("2026-09-04T09:00:00+02:00", "2026-09-04T09:00:00Z"),
            Some(120),
            "offsets must be normalised before subtracting"
        );
        assert_eq!(minutes_between("soon", "2026-09-04T11:30:00Z"), None);
        assert_eq!(minutes_between("2026-09-04T09:00:00Z", ""), None);
    }

    #[tokio::test]
    async fn reception_reads_the_merge_time_cycles_approvals_and_comments() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/acme/project/pulls/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "created_at": "2026-09-04T09:00:00Z",
                "merged_at": "2026-09-04T10:00:00Z",
                "state": "closed",
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/acme/project/pulls/7/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "state": "CHANGES_REQUESTED", "body": "needs a regression test", "user": { "login": "ada" } },
                { "state": "APPROVED", "body": "", "user": { "login": "ada" } },
                { "state": "APPROVED", "body": null, "user": { "login": "grace" } },
            ])))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/acme/project/pulls/7/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "body": "rename this variable" },
                { "body": "   " },
            ])))
            .mount(&server)
            .await;

        let service = PrService::with_base_url(server.uri());
        let reception = service
            .fetch_reception(
                &PullRequestReference {
                    owner: "acme".to_string(),
                    repository: "project".to_string(),
                    number: 7,
                },
                "token",
            )
            .await
            .expect("a reachable pull request must yield its reception");

        assert_eq!(reception.opened_at.as_deref(), Some("2026-09-04T09:00:00Z"));
        assert_eq!(reception.merged_at.as_deref(), Some("2026-09-04T10:00:00Z"));
        assert_eq!(reception.minutes_to_merge, Some(60));
        assert_eq!(reception.review_cycles, 1);
        assert_eq!(reception.approvals, 2);
        assert_eq!(
            reception.comments,
            vec!["needs a regression test", "rename this variable"],
            "review bodies and inline comments both carry reviewer corrections"
        );
    }

    #[tokio::test]
    async fn an_unmerged_pull_request_reports_no_merge_time() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/acme/project/pulls/7"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "created_at": "2026-09-04T09:00:00Z",
                "merged_at": null,
                "state": "open",
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/acme/project/pulls/7/reviews"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/acme/project/pulls/7/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let reception = PrService::with_base_url(server.uri())
            .fetch_reception(
                &PullRequestReference {
                    owner: "acme".to_string(),
                    repository: "project".to_string(),
                    number: 7,
                },
                "token",
            )
            .await
            .unwrap();

        assert_eq!(reception.merged_at, None);
        assert_eq!(reception.minutes_to_merge, None);
        assert_eq!(reception.state.as_deref(), Some("open"));
        assert!(reception.comments.is_empty());
    }

    #[tokio::test]
    async fn mergeability_distinguishes_conflicted_from_not_yet_computed() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        for (reported, expected) in [
            (serde_json::json!(true), Mergeability::Clean),
            (serde_json::json!(false), Mergeability::Conflicted),
            (serde_json::json!(null), Mergeability::Unknown),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/repos/acme/project/pulls/7"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({ "mergeable": reported })),
                )
                .mount(&server)
                .await;

            let mergeability = PrService::with_base_url(server.uri())
                .fetch_mergeability(
                    &PullRequestReference {
                        owner: "acme".to_string(),
                        repository: "project".to_string(),
                        number: 7,
                    },
                    "token",
                )
                .await
                .unwrap();

            assert_eq!(mergeability, expected, "GitHub reported {reported}");
        }

        assert!(Mergeability::Conflicted.conflicted());
        assert!(
            !Mergeability::Unknown.conflicted(),
            "a branch nobody has checked is not a branch known to conflict"
        );
    }

    #[tokio::test]
    async fn a_rejected_token_is_reported_as_an_authentication_failure() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/acme/project/pulls/7"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let failure = PrService::with_base_url(server.uri())
            .fetch_reception(
                &PullRequestReference {
                    owner: "acme".to_string(),
                    repository: "project".to_string(),
                    number: 7,
                },
                "token",
            )
            .await
            .expect_err("an unauthorised read must not look like an empty pull request");

        assert!(matches!(failure, PrError::AuthFailed));
    }
}
