//! Pull request creation service
//!
//! Creates pull requests on GitHub when a task completes with code changes, and
//! reads back how each one was received: when it merged, how many rounds of
//! review it took, who approved it, and what the reviewers actually said.

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

/// GitHub's own REST origin, which answers for repositories on `github.com`.
const GITHUB_API_URL: &str = "https://api.github.com";

/// Rows GitHub returns per page; its maximum for these collections.
const PAGE_SIZE: usize = 100;

/// What a pull request says, for a reviewer who was not in the chat.
///
/// The problem comes first because it is what the reviewer judges the change
/// against, then the run's own report of what it did about it, then the files
/// it touched. A run that reported nothing leaves its section out rather than
/// heading an empty one.
pub struct Description<'a> {
    pub problem: &'a str,
    pub report: Option<&'a str>,
    pub changes: Option<&'a str>,
    pub task: Uuid,
    pub url: Option<&'a str>,
}

impl Description<'_> {
    pub fn render(&self) -> String {
        let mut body = format!("## Problem\n\n{}\n\n", self.problem.trim());

        if let Some(report) = self.report.map(str::trim).filter(|it| !it.is_empty()) {
            body.push_str(&format!("## What changed\n\n{report}\n\n"));
        }

        if let Some(changes) = self.changes.map(str::trim).filter(|it| !it.is_empty()) {
            body.push_str(&format!("## Files\n\n{changes}\n\n"));
        }

        body.push_str("---\n");
        match self.url {
            Some(url) => body.push_str(&format!("Opened by Zone from [this task]({url}).\n")),
            None => body.push_str(&format!("Opened by Zone from task `{}`.\n", self.task)),
        }
        body
    }
}

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

    #[error("The pull request cannot be merged: {0}")]
    NotMergeable(String),

    #[error("Branch protection refused the merge: {0}")]
    Protected(String),

    #[error("The pull request head moved since it was read")]
    HeadMoved,

    #[error("A repository named {0} already exists")]
    RepositoryExists(String),
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

    /// GitHub computes mergeability in the background after a push; until it
    /// has, a merge is refused with the same 405 a protection rule gives.
    pub fn unknown(self) -> bool {
        self == Mergeability::Unknown
    }
}

/// PR service for creating pull requests
#[derive(Debug, Clone)]
pub struct PrService {
    client: Client,
    origin: Origin,
}

/// The API origin this service addresses, and the repository host it answers for.
///
/// The two are decided together because every request is
/// `{origin}/repos/{owner}/{repository}/...`: the owner and repository come from
/// a repository URL, and the origin they are interpolated into has to be the one
/// that answers for that URL's host. Deciding them apart is what let a
/// `github.com` repository be accepted while its request -- and the token sent
/// with it -- went to a configured Enterprise origin.
#[derive(Debug, Clone)]
struct Origin {
    url: String,
    host: String,
}

impl Origin {
    /// The origin an operator configured. It answers for its own host, less the
    /// `api.` label carried by GitHub's own API host and by a subdomain-isolated
    /// Enterprise one; an Enterprise install without subdomain isolation answers
    /// for itself under a `/api/v3` path.
    fn configured(url: String) -> Self {
        let authority = url
            .split_once("://")
            .map_or(url.as_str(), |(_, rest)| rest)
            .split('/')
            .next()
            .unwrap_or_default();
        let host = host_of(authority).to_ascii_lowercase();
        let host = host.strip_prefix("api.").unwrap_or(&host).to_string();
        Self { url, host }
    }

    fn standing_in_for(host: &str, url: String) -> Self {
        Self {
            url,
            host: host.to_ascii_lowercase(),
        }
    }

    /// Whether a repository on `host` is one this origin can be asked about.
    fn answers_for(&self, host: &str) -> bool {
        !self.host.is_empty() && host.eq_ignore_ascii_case(&self.host)
    }
}

impl Default for PrService {
    fn default() -> Self {
        Self::new()
    }
}

/// A single path segment that is safe to interpolate into a request URL.
/// The host of an authority, without userinfo or port.
fn host_of(authority: &str) -> &str {
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    match host.strip_prefix('[') {
        Some(tail) => tail.split_once(']').map_or(host, |(inside, _)| inside),
        None => host.split_once(':').map_or(host, |(host, _)| host),
    }
}

/// Schemes a repository address may carry.
///
/// The scp-like `git@host:owner/repo` has none and is read on its own terms.
const SCHEMES: [&str; 2] = ["https", "ssh"];

fn named(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

impl PrService {
    /// Address GitHub's own API.
    pub fn new() -> Self {
        Self::configured(GITHUB_API_URL.to_string())
    }

    /// Address the origin an operator configured, for the repositories it
    /// answers for.
    pub fn configured(url: String) -> Self {
        Self {
            client: Client::new(),
            origin: Origin::configured(url),
        }
    }

    /// Address `url` as a stand-in for repositories on `host`.
    ///
    /// No operator setting produces one: a configured origin has to answer for
    /// a host it can be reached at. Publication tests use this to drive the
    /// real request path against a mock server.
    pub fn standing_in_for(host: &str, url: String) -> Self {
        Self {
            client: Client::new(),
            origin: Origin::standing_in_for(host, url),
        }
    }

    /// Parse owner and repo from a repository URL.
    ///
    /// The host has to be the one this service's origin answers for, so that a
    /// GitHub Enterprise install parses its own repositories and nothing else.
    /// Matching `https://github.com/` literally was what made `GITHUB_API_URL`
    /// insufficient on its own to reach Enterprise; matching nothing at all
    /// would accept a repository this service cannot open a request against;
    /// and admitting `github.com` alongside the configured origin sent a
    /// github.com repository's token to whichever install was configured.
    ///
    /// Supports, for the host it answers for:
    ///
    /// - `https://host/owner/repo`, with or without `.git`
    /// - `git@host:owner/repo.git`
    /// - `ssh://git@host/owner/repo.git`
    ///
    /// A scheme outside [`SCHEMES`] is refused rather than discarded: reading
    /// the owner and repo out of an `ftp://` or `http://` address treats it as
    /// a repository this service publishes to, which is not what it is.
    pub fn parse_github_url(&self, url: &str) -> PrResult<(String, String)> {
        let invalid = || PrError::InvalidRepoUrl(url.to_string());
        let url = url.trim();

        // `git@host:owner/repo` is not a URL, so it is split on the colon
        // rather than parsed. The scp-like form has no scheme to strip.
        let (authority, path) = if let Some((scheme, rest)) = url.split_once("://") {
            if !SCHEMES
                .iter()
                .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
            {
                return Err(invalid());
            }
            rest.split_once('/').ok_or_else(invalid)?
        } else if let Some((authority, path)) = url.split_once(':') {
            (authority, path)
        } else {
            return Err(invalid());
        };

        if !self.origin.answers_for(host_of(authority)) {
            return Err(invalid());
        }

        let path = path.trim_matches('/').trim_end_matches(".git");
        let mut segments = path.split('/').filter(|segment| !segment.is_empty());
        let owner = segments.next().ok_or_else(invalid)?;
        let repository = segments.next().ok_or_else(invalid)?;
        // The pair is interpolated into `{base}/repos/{owner}/{repo}/...`, so a
        // third segment or a traversal component would reach a different
        // endpoint than the caller asked for.
        if segments.next().is_some() || !named(owner) || !named(repository) {
            return Err(invalid());
        }

        Ok((owner.to_string(), repository.to_string()))
    }

    /// Recover where a pull request lives from the URL a run recorded.
    ///
    /// Reads `https://host/owner/repo/pull/123`, with or without a trailing
    /// segment such as `/files` that a person's copied link often carries. The
    /// host is held to the same origin as a repository URL, because the pull
    /// request is read back from `{origin}/repos/{owner}/{repo}/pulls/{number}`
    /// -- a recorded github.com link would otherwise be read from, and
    /// authenticated against, whichever install happened to be configured.
    pub fn pull_request(&self, url: &str) -> PrResult<PullRequestReference> {
        let invalid = || PrError::InvalidRepoUrl(url.to_string());
        let (scheme, rest) = url.trim().split_once("://").ok_or_else(invalid)?;
        let (authority, path) = rest.split_once('/').ok_or_else(invalid)?;

        if !matches!(scheme, "http" | "https") || !self.origin.answers_for(host_of(authority)) {
            return Err(invalid());
        }

        let mut segments = path.split('/');
        let owner = segments.next().unwrap_or_default();
        let repository = segments.next().unwrap_or_default().trim_end_matches(".git");
        let marker = segments.next().unwrap_or_default();
        let number: i64 = segments
            .next()
            .unwrap_or_default()
            .parse()
            .map_err(|_| invalid())?;

        if marker != "pull" || number < 1 || !named(owner) || !named(repository) {
            return Err(invalid());
        }

        Ok(PullRequestReference {
            owner: owner.to_string(),
            repository: repository.to_string(),
            number,
        })
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
        let url = format!("{}/repos/{}/{}/pulls", self.origin.url, owner, repo);

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

    /// Get the default branch for a repository
    pub async fn get_default_branch(
        &self,
        owner: &str,
        repo: &str,
        token: &str,
    ) -> PrResult<String> {
        let url = format!("{}/repos/{}/{}", self.origin.url, owner, repo);

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
            self.origin.url, owner, repo, owner, head_branch
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
                self.origin.url, path, PAGE_SIZE, page
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
                    self.origin.url, reference.owner, reference.repository, reference.number
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
                &format!("{}/{}/pulls/{}", self.origin.url, scope, reference.number),
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

    /// Matching `https://github.com/` meant an Enterprise repository was
    /// refused as invalid, so `GITHUB_API_URL` alone could not reach one.
    #[test]
    fn an_enterprise_repository_parses_like_a_github_one() {
        let service = PrService::configured("https://github.example.com/api/v3".to_string());
        for url in [
            "https://github.example.com/acme/project",
            "https://github.example.com/acme/project.git",
            "ssh://git@github.example.com/acme/project.git",
            "git@github.example.com:acme/project.git",
            "  https://github.example.com/acme/project/  ",
        ] {
            assert_eq!(
                service.parse_github_url(url).expect(url),
                ("acme".to_string(), "project".to_string()),
                "{url}"
            );
        }
    }

    /// A subdomain-isolated Enterprise install serves its API from `api.` on
    /// the host its repositories live on, exactly as api.github.com does for
    /// github.com.
    #[test]
    fn an_api_subdomain_answers_for_the_host_beneath_it() {
        for origin in [
            "https://api.github.example.com",
            "https://github.example.com/api/v3",
        ] {
            assert_eq!(
                PrService::configured(origin.to_string())
                    .parse_github_url("https://github.example.com/acme/project")
                    .expect(origin),
                ("acme".to_string(), "project".to_string()),
                "{origin}"
            );
        }
    }

    /// The repository's host picks the origin its owner and repository are
    /// interpolated into, and only the configured origin's own host has one. A
    /// github.com repository accepted here would have had its request -- and
    /// its access token -- sent to the Enterprise install instead.
    #[test]
    fn a_github_repository_has_no_origin_while_enterprise_is_configured() {
        let enterprise = PrService::configured("https://github.example.com/api/v3".to_string());
        for url in [
            "https://github.com/acme/project",
            "https://github.com/acme/project.git",
            "git@github.com:acme/project.git",
            "https://github.com/acme/project/pull/7",
        ] {
            assert!(
                enterprise.parse_github_url(url).is_err(),
                "{url} must not be addressed at an origin that does not answer for github.com"
            );
        }
        assert!(
            enterprise
                .pull_request("https://github.com/acme/project/pull/7")
                .is_err(),
            "a recorded github.com pull request must not be read from the Enterprise origin"
        );
    }

    /// The public path is what almost every deployment runs, so it has to stay
    /// exactly as it was: GitHub's own origin answers for github.com.
    #[test]
    fn githubs_own_origin_answers_for_github_repositories() {
        for service in [
            PrService::new(),
            PrService::configured(GITHUB_API_URL.to_string()),
        ] {
            assert_eq!(
                service
                    .parse_github_url("https://github.com/acme/project")
                    .expect("github.com is what api.github.com answers for"),
                ("acme".to_string(), "project".to_string())
            );
            assert_eq!(
                service
                    .pull_request("https://github.com/acme/project/pull/7")
                    .expect("a github.com pull request is read from api.github.com")
                    .number,
                7
            );
        }
    }

    /// The endpoint the pair is interpolated into belongs to one host, so a
    /// repository somewhere else is not this service's to address -- whatever
    /// its path happens to look like.
    #[test]
    fn a_repository_on_another_host_is_refused() {
        let service = PrService::new();
        for url in [
            "https://gitlab.com/acme/project",
            "https://bitbucket.org/acme/project.git",
            "git@gitlab.com:acme/project.git",
            "https://github.com.attacker.test/acme/project",
            "https://notgithub.com/acme/project",
            "https://github.example.com/acme/project",
        ] {
            assert!(
                service.parse_github_url(url).is_err(),
                "{url} was accepted for api.github.com"
            );
        }

        let enterprise = PrService::configured("https://github.example.com/api/v3".to_string());
        for url in [
            "https://gitlab.com/acme/project",
            "https://github.example.com.attacker.test/acme/project",
        ] {
            assert!(
                enterprise.parse_github_url(url).is_err(),
                "a configured enterprise origin does not admit {url}"
            );
        }
    }

    /// The pair is interpolated into `{base}/repos/{owner}/{repo}/pulls`, so
    /// anything that could reach a different endpoint has to be refused. The
    /// old parser split into two and kept every remaining slash in `repo`.
    #[test]
    fn a_path_that_could_reach_another_endpoint_is_refused() {
        let service = PrService::new();
        for url in [
            "https://github.com/acme/project/extra",
            "https://github.com/acme/../admin",
            "https://github.com/../acme",
            "https://github.com/acme",
            "https://github.com/",
            "https://github.com/acme/pro ject",
            "not-a-url",
            "",
        ] {
            assert!(
                service.parse_github_url(url).is_err(),
                "{url:?} must be refused"
            );
        }
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

    fn task() -> Uuid {
        Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap()
    }

    /// The reviewer was not in the chat, so what the change was for comes
    /// before what it did about it, and the run's own report is the account of
    /// the second.
    #[test]
    fn a_description_leads_with_the_problem_and_then_the_run_s_own_report() {
        let body = Description {
            problem: "The login form accepts an invalid email.",
            report: Some("Validated the address before submit. Added a regression test."),
            changes: Some("- `auth.rs`"),
            task: task(),
            url: Some("https://zone.example.com/tasks/123"),
        }
        .render();

        let problem = body.find("## Problem").expect("{body}");
        let changed = body.find("## What changed").expect("{body}");
        let files = body.find("## Files").expect("{body}");

        assert!(problem < changed && changed < files, "{body}");
        assert!(body.contains("accepts an invalid email"), "{body}");
        assert!(body.contains("Added a regression test."), "{body}");
        assert!(body.contains("`auth.rs`"), "{body}");
        assert!(
            body.contains("[this task](https://zone.example.com/tasks/123)"),
            "{body}"
        );
    }

    /// A heading over nothing reads as a section the reviewer has missed.
    #[test]
    fn a_run_that_reported_nothing_heads_no_empty_section() {
        for report in [None, Some(""), Some("   \n ")] {
            let body = Description {
                problem: "Something was wrong.",
                report,
                changes: None,
                task: task(),
                url: None,
            }
            .render();

            assert!(!body.contains("## What changed"), "{body}");
            assert!(!body.contains("## Files"), "{body}");
            assert!(body.contains("## Problem"), "{body}");
        }
    }

    /// Without a console to link to, the id is what takes a reviewer back to
    /// the run that opened this.
    #[test]
    fn a_description_without_a_console_link_names_the_task_it_came_from() {
        let body = Description {
            problem: "Something was wrong.",
            report: None,
            changes: None,
            task: task(),
            url: None,
        }
        .render();

        assert!(body.contains(&task().to_string()), "{body}");
        assert!(!body.contains("this task]("), "{body}");
    }

    fn review(state: &str, reviewer: Option<&str>) -> SubmittedReview {
        SubmittedReview {
            state: ReviewState::parse(state),
            reviewer: reviewer.map(str::to_string),
        }
    }

    #[test]
    fn a_pull_request_url_yields_its_owner_repository_and_number() {
        let reference = PrService::new()
            .pull_request("https://github.com/acme/project/pull/42")
            .expect("a plain pull request URL must parse");
        assert_eq!(reference.owner, "acme");
        assert_eq!(reference.repository, "project");
        assert_eq!(reference.number, 42);
    }

    #[test]
    fn a_pull_request_url_parses_past_a_trailing_tab_segment() {
        let reference = PrService::new()
            .pull_request("https://github.com/acme/project/pull/42/files")
            .unwrap();
        assert_eq!(reference.number, 42);
    }

    #[test]
    fn a_url_that_is_not_a_pull_request_is_refused() {
        let service = PrService::new();
        for url in [
            "https://github.com/acme/project",
            "https://github.com/acme/project/issues/42",
            "https://github.com/acme/project/pull/zero",
            "https://github.com/acme/project/pull/0",
            "https://github.com/../project/pull/42",
            "https://example.test/acme/project/pull/42",
            "git@github.com:acme/project/pull/42",
            "",
        ] {
            assert!(
                service.pull_request(url).is_err(),
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

        let service = PrService::standing_in_for("github.com", server.uri());
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

        let reception = PrService::standing_in_for("github.com", server.uri())
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

            let mergeability = PrService::standing_in_for("github.com", server.uri())
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

        let failure = PrService::standing_in_for("github.com", server.uri())
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

// ---------------------------------------------------------------------------
// What an auto project needs beyond opening a pull request: reading it back,
// reading its checks and its review threads, answering them, and merging.
// ---------------------------------------------------------------------------

/// A repository GitHub just made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedRepository {
    pub html_url: String,
    pub clone_url: String,
    pub default_branch: String,
}

/// A pull request as GitHub describes it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestDetail {
    pub node_id: String,
    pub number: i64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub draft: bool,
    pub merged: bool,
    pub merge_commit_sha: Option<String>,
    pub head_sha: String,
    pub head_ref: String,
    pub base_ref: String,
    pub mergeable: Mergeability,
    pub mergeable_state: Option<String>,
    pub changed_files: u32,
    pub additions: u32,
    pub deletions: u32,
    pub commits: u32,
    pub html_url: String,
}

/// What the checks on a commit add up to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChecksOutcome {
    /// Every check and status finished without failing.
    Success,
    /// At least one failed; the names say which.
    Failure(Vec<String>),
    /// At least one is still running and none has failed.
    Pending,
    /// Nothing reports on this commit at all.
    Absent,
}

impl ChecksOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failure(_) => "failure",
            Self::Pending => "pending",
            Self::Absent => "absent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub filename: String,
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
}

/// A comment on the pull request's conversation, where bots leave summaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueComment {
    pub id: u64,
    pub author: String,
    pub body: String,
    pub url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadComment {
    pub database_id: Option<u64>,
    pub author: String,
    pub body: String,
    pub url: String,
    pub created_at: String,
}

/// One review thread on the diff and every comment in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThreadRecord {
    pub id: String,
    pub resolved: bool,
    pub outdated: bool,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub comments: Vec<ThreadComment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}

impl ReviewEvent {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Comment => "COMMENT",
            Self::Approve => "APPROVE",
            Self::RequestChanges => "REQUEST_CHANGES",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Squash,
    Merge,
    Rebase,
}

impl MergeMethod {
    const fn rest(self) -> &'static str {
        match self {
            Self::Squash => "squash",
            Self::Merge => "merge",
            Self::Rebase => "rebase",
        }
    }

    const fn graphql(self) -> &'static str {
        match self {
            Self::Squash => "SQUASH",
            Self::Merge => "MERGE",
            Self::Rebase => "REBASE",
        }
    }
}

/// A merge that happened, and whether it took administrator privileges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedPr {
    pub sha: String,
    pub admin: bool,
}

/// The marker a truncated body ends with.
pub const TRUNCATED: &str = "\n[truncated]";

/// Conclusions that fail a commit, as GitHub names them.
const FAILING_CONCLUSIONS: [&str; 6] = [
    "failure",
    "error",
    "cancelled",
    "timed_out",
    "action_required",
    "startup_failure",
];

fn truncated(text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut cut = max_bytes;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut kept = text[..cut].to_string();
    kept.push_str(TRUNCATED);
    kept
}

/// A repository path safe to interpolate into a request URL: every segment
/// named, so no `..` and no empty segment can reach a different endpoint.
fn repository_path(path: &str) -> PrResult<String> {
    let trimmed = path.trim().trim_matches('/');
    if trimmed.is_empty() || !trimmed.split('/').all(named) {
        return Err(PrError::GitHubApi(format!(
            "Invalid repository path: {path}"
        )));
    }
    Ok(trimmed.to_string())
}

fn login(user: &Value) -> String {
    user["login"].as_str().unwrap_or_default().to_string()
}

impl PrService {
    fn repos(&self, owner: &str, repo: &str) -> String {
        format!("{}/repos/{}/{}", self.origin.url, owner, repo)
    }

    fn pulls(&self, reference: &PullRequestReference) -> String {
        format!(
            "{}/pulls/{}",
            self.repos(&reference.owner, &reference.repository),
            reference.number
        )
    }

    /// Where GraphQL lives for this origin: beside REST on github.com, under
    /// `/api/graphql` on an Enterprise install whose REST is `/api/v3`.
    fn graphql_url(&self) -> String {
        match self.origin.url.strip_suffix("/api/v3") {
            Some(host) => format!("{host}/api/graphql"),
            None => format!("{}/graphql", self.origin.url),
        }
    }

    async fn send(
        &self,
        method: reqwest::Method,
        url: &str,
        token: &str,
        accept: &str,
        body: Option<&Value>,
    ) -> PrResult<reqwest::Response> {
        let mut request = self
            .client
            .request(method, url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", accept)
            .header("User-Agent", "zone-pr-service");
        if let Some(body) = body {
            request = request.json(body);
        }
        Ok(request.send().await?)
    }

    /// Send, and turn any status outside 2xx into the error it stands for.
    async fn send_checked(
        &self,
        method: reqwest::Method,
        url: &str,
        token: &str,
        accept: &str,
        body: Option<&Value>,
    ) -> PrResult<reqwest::Response> {
        let response = self.send(method, url, token, accept, body).await?;
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
        Ok(response)
    }

    async fn json(
        &self,
        method: reqwest::Method,
        url: &str,
        token: &str,
        body: Option<&Value>,
    ) -> PrResult<Value> {
        Ok(self
            .send_checked(method, url, token, "application/vnd.github+json", body)
            .await?
            .json()
            .await?)
    }

    /// One GraphQL call; an `errors` array is a refusal, named by its first message.
    async fn graphql(&self, token: &str, query: &str, variables: Value) -> PrResult<Value> {
        let response: Value = self
            .send_checked(
                reqwest::Method::POST,
                &self.graphql_url(),
                token,
                "application/vnd.github+json",
                Some(&json!({"query": query, "variables": variables})),
            )
            .await?
            .json()
            .await?;
        if let Some(errors) = response.get("errors").and_then(Value::as_array)
            && !errors.is_empty()
        {
            let message = errors
                .iter()
                .filter_map(|error| error["message"].as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(PrError::GitHubApi(format!("GraphQL: {message}")));
        }
        Ok(response["data"].clone())
    }

    /// Make a repository, with an initial commit so the first task has a
    /// default branch to start from.
    ///
    /// `owner` names an organisation; `None`, or the token's own login, makes
    /// the repository under the token's user.
    pub async fn create_repository(
        &self,
        token: &str,
        owner: Option<&str>,
        name: &str,
        description: &str,
        private: bool,
    ) -> PrResult<CreatedRepository> {
        if !named(name) {
            return Err(PrError::GitHubApi(format!(
                "Invalid repository name: {name}"
            )));
        }
        let me = self
            .json(
                reqwest::Method::GET,
                &format!("{}/user", self.origin.url),
                token,
                None,
            )
            .await?;
        let me = login(&me);
        let url = match owner.map(str::trim).filter(|owner| !owner.is_empty()) {
            Some(organisation) if !organisation.eq_ignore_ascii_case(&me) => {
                if !named(organisation) {
                    return Err(PrError::GitHubApi(format!(
                        "Invalid repository owner: {organisation}"
                    )));
                }
                format!("{}/orgs/{}/repos", self.origin.url, organisation)
            }
            _ => format!("{}/user/repos", self.origin.url),
        };
        let response = self
            .send(
                reqwest::Method::POST,
                &url,
                token,
                "application/vnd.github+json",
                Some(&json!({
                    "name": name,
                    "description": description,
                    "private": private,
                    "auto_init": true,
                })),
            )
            .await?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(PrError::AuthFailed);
        }
        let text = response.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::UNPROCESSABLE_ENTITY && text.contains("already exists") {
            return Err(PrError::RepositoryExists(name.to_string()));
        }
        if !status.is_success() {
            return Err(PrError::GitHubApi(format!(
                "GitHub API returned {}: {}",
                status, text
            )));
        }
        let created: Value = serde_json::from_str(&text)
            .map_err(|error| PrError::GitHubApi(format!("Unreadable repository: {error}")))?;
        Ok(CreatedRepository {
            html_url: created["html_url"].as_str().unwrap_or_default().to_string(),
            clone_url: created["clone_url"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            default_branch: created["default_branch"]
                .as_str()
                .unwrap_or("main")
                .to_string(),
        })
    }

    /// Read a pull request back.
    pub async fn fetch_pull(
        &self,
        reference: &PullRequestReference,
        token: &str,
    ) -> PrResult<PullRequestDetail> {
        let pull = self
            .json(reqwest::Method::GET, &self.pulls(reference), token, None)
            .await?;
        let count = |key: &str| u32::try_from(pull[key].as_u64().unwrap_or(0)).unwrap_or(u32::MAX);
        Ok(PullRequestDetail {
            node_id: pull["node_id"].as_str().unwrap_or_default().to_string(),
            number: pull["number"].as_i64().unwrap_or(reference.number),
            title: pull["title"].as_str().unwrap_or_default().to_string(),
            body: pull["body"].as_str().map(str::to_string),
            state: pull["state"].as_str().unwrap_or_default().to_string(),
            draft: pull["draft"].as_bool().unwrap_or(false),
            merged: pull["merged"].as_bool().unwrap_or(false),
            merge_commit_sha: pull["merge_commit_sha"].as_str().map(str::to_string),
            head_sha: pull["head"]["sha"].as_str().unwrap_or_default().to_string(),
            head_ref: pull["head"]["ref"].as_str().unwrap_or_default().to_string(),
            base_ref: pull["base"]["ref"].as_str().unwrap_or_default().to_string(),
            mergeable: Mergeability::from_flag(pull["mergeable"].as_bool()),
            mergeable_state: pull["mergeable_state"].as_str().map(str::to_string),
            changed_files: count("changed_files"),
            additions: count("additions"),
            deletions: count("deletions"),
            commits: count("commits"),
            html_url: pull["html_url"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// Every element of a paged object endpoint's array field, page after page
    /// until a short page, capped at `MAXIMUM_PAGES`.
    async fn paged(&self, url: &str, field: &str, token: &str) -> PrResult<Vec<Value>> {
        let separator = if url.contains('?') { '&' } else { '?' };
        let mut collected: Vec<Value> = Vec::new();
        for page in 1..=MAXIMUM_PAGES {
            let body = self
                .json(
                    reqwest::Method::GET,
                    &format!("{url}{separator}per_page={PAGE_SIZE}&page={page}"),
                    token,
                    None,
                )
                .await?;
            let batch: Vec<Value> = body[field].as_array().cloned().unwrap_or_default();
            let complete = batch.len() < PAGE_SIZE;
            collected.extend(batch);
            if complete {
                break;
            }
        }
        Ok(collected)
    }

    /// What the check runs and commit statuses on a commit add up to.
    ///
    /// A run still going is pending unless something else already failed; a
    /// neutral or skipped conclusion is neither. Nothing reporting at all is
    /// `Absent`, which a caller decides the meaning of: a new repository has no
    /// checks yet, and that is not a pass.
    pub async fn fetch_checks(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
        token: &str,
    ) -> PrResult<ChecksOutcome> {
        if !sha.chars().all(|character| character.is_ascii_hexdigit()) || sha.is_empty() {
            return Err(PrError::GitHubApi(format!("Invalid commit: {sha}")));
        }
        let scope = self.repos(owner, repo);
        // Both endpoints page: a failing run past the first page must count,
        // or a head with many checks would merge on the ones that fit.
        let runs = self
            .paged(
                &format!("{scope}/commits/{sha}/check-runs?filter=latest"),
                "check_runs",
                token,
            )
            .await?;
        let statuses = self
            .paged(&format!("{scope}/commits/{sha}/status"), "statuses", token)
            .await?;
        let mut seen = 0usize;
        let mut pending = false;
        let mut failed: Vec<String> = Vec::new();
        for run in &runs {
            seen += 1;
            let name = run["name"].as_str().unwrap_or("check").to_string();
            if run["status"].as_str() != Some("completed") {
                pending = true;
                continue;
            }
            let conclusion = run["conclusion"].as_str().unwrap_or_default();
            if FAILING_CONCLUSIONS.contains(&conclusion) {
                failed.push(name);
            }
        }
        for status in &statuses {
            seen += 1;
            let name = status["context"].as_str().unwrap_or("status").to_string();
            match status["state"].as_str().unwrap_or_default() {
                "pending" => pending = true,
                "failure" | "error" => failed.push(name),
                _ => {}
            }
        }
        Ok(if !failed.is_empty() {
            failed.sort();
            failed.dedup();
            ChecksOutcome::Failure(failed)
        } else if pending {
            ChecksOutcome::Pending
        } else if seen == 0 {
            ChecksOutcome::Absent
        } else {
            ChecksOutcome::Success
        })
    }

    /// The unified diff of a pull request, cut at `max_bytes`.
    pub async fn fetch_diff(
        &self,
        reference: &PullRequestReference,
        token: &str,
        max_bytes: usize,
    ) -> PrResult<String> {
        let text = self
            .send_checked(
                reqwest::Method::GET,
                &self.pulls(reference),
                token,
                "application/vnd.github.diff",
                None,
            )
            .await?
            .text()
            .await?;
        Ok(truncated(text, max_bytes))
    }

    pub async fn fetch_files(
        &self,
        reference: &PullRequestReference,
        token: &str,
    ) -> PrResult<Vec<ChangedFile>> {
        let rows: Vec<Value> = self
            .get_all(
                &format!(
                    "repos/{}/{}/pulls/{}/files",
                    reference.owner, reference.repository, reference.number
                ),
                token,
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| ChangedFile {
                filename: row["filename"].as_str().unwrap_or_default().to_string(),
                status: row["status"].as_str().unwrap_or_default().to_string(),
                additions: u32::try_from(row["additions"].as_u64().unwrap_or(0))
                    .unwrap_or(u32::MAX),
                deletions: u32::try_from(row["deletions"].as_u64().unwrap_or(0))
                    .unwrap_or(u32::MAX),
            })
            .collect())
    }

    /// One file at one ref, as text, cut at `max_bytes`.
    pub async fn fetch_file(
        &self,
        owner: &str,
        repo: &str,
        path: &str,
        git_ref: &str,
        token: &str,
        max_bytes: usize,
    ) -> PrResult<String> {
        let path = repository_path(path)?;
        if git_ref.is_empty()
            || !git_ref
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        {
            return Err(PrError::GitHubApi(format!("Invalid ref: {git_ref}")));
        }
        let bytes = self
            .send_checked(
                reqwest::Method::GET,
                &format!("{}/contents/{path}?ref={git_ref}", self.repos(owner, repo)),
                token,
                "application/vnd.github.raw+json",
                None,
            )
            .await?
            .bytes()
            .await?;
        Ok(truncated(
            String::from_utf8_lossy(&bytes).into_owned(),
            max_bytes,
        ))
    }

    /// Comments on the conversation tab, where review bots post their summaries.
    pub async fn fetch_issue_comments(
        &self,
        reference: &PullRequestReference,
        token: &str,
    ) -> PrResult<Vec<IssueComment>> {
        let rows: Vec<Value> = self
            .get_all(
                &format!(
                    "repos/{}/{}/issues/{}/comments",
                    reference.owner, reference.repository, reference.number
                ),
                token,
            )
            .await?;
        Ok(rows
            .iter()
            .map(|row| IssueComment {
                id: row["id"].as_u64().unwrap_or(0),
                author: login(&row["user"]),
                body: row["body"].as_str().unwrap_or_default().to_string(),
                url: row["html_url"].as_str().unwrap_or_default().to_string(),
                created_at: row["created_at"].as_str().unwrap_or_default().to_string(),
            })
            .collect())
    }

    /// Every review thread on the diff, with the comments in it.
    pub async fn fetch_review_threads(
        &self,
        reference: &PullRequestReference,
        token: &str,
    ) -> PrResult<Vec<ReviewThreadRecord>> {
        const QUERY: &str = "query($owner:String!,$name:String!,$number:Int!,$after:String){\
repository(owner:$owner,name:$name){pullRequest(number:$number){\
reviewThreads(first:100,after:$after){pageInfo{hasNextPage endCursor}\
nodes{id isResolved isOutdated path line comments(first:20){\
nodes{databaseId body url createdAt author{login}}}}}}}}";
        let mut threads = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..MAXIMUM_PAGES {
            let data = self
                .graphql(
                    token,
                    QUERY,
                    json!({
                        "owner": reference.owner,
                        "name": reference.repository,
                        "number": reference.number,
                        "after": after,
                    }),
                )
                .await?;
            let page = &data["repository"]["pullRequest"]["reviewThreads"];
            for node in page["nodes"].as_array().into_iter().flatten() {
                threads.push(ReviewThreadRecord {
                    id: node["id"].as_str().unwrap_or_default().to_string(),
                    resolved: node["isResolved"].as_bool().unwrap_or(false),
                    outdated: node["isOutdated"].as_bool().unwrap_or(false),
                    path: node["path"].as_str().map(str::to_string),
                    line: node["line"]
                        .as_u64()
                        .and_then(|line| u32::try_from(line).ok()),
                    comments: node["comments"]["nodes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|comment| ThreadComment {
                            database_id: comment["databaseId"].as_u64(),
                            author: login(&comment["author"]),
                            body: comment["body"].as_str().unwrap_or_default().to_string(),
                            url: comment["url"].as_str().unwrap_or_default().to_string(),
                            created_at: comment["createdAt"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                        })
                        .collect(),
                });
            }
            if page["pageInfo"]["hasNextPage"].as_bool() != Some(true) {
                break;
            }
            after = page["pageInfo"]["endCursor"].as_str().map(str::to_string);
            if after.is_none() {
                break;
            }
        }
        Ok(threads)
    }

    /// Submit a review. A token that opened the pull request may only comment:
    /// GitHub refuses it an approval of its own change.
    pub async fn submit_review(
        &self,
        reference: &PullRequestReference,
        token: &str,
        commit_id: &str,
        event: ReviewEvent,
        body: &str,
    ) -> PrResult<i64> {
        let review = self
            .json(
                reqwest::Method::POST,
                &format!("{}/reviews", self.pulls(reference)),
                token,
                Some(&json!({
                    "commit_id": commit_id,
                    "body": body,
                    "event": event.as_str(),
                })),
            )
            .await?;
        Ok(review["id"].as_i64().unwrap_or(0))
    }

    /// Answer a review comment in its thread.
    pub async fn reply_to_review_comment(
        &self,
        reference: &PullRequestReference,
        comment_id: u64,
        body: &str,
        token: &str,
    ) -> PrResult<()> {
        self.json(
            reqwest::Method::POST,
            &format!("{}/comments/{comment_id}/replies", self.pulls(reference)),
            token,
            Some(&json!({"body": body})),
        )
        .await?;
        Ok(())
    }

    pub async fn resolve_review_thread(&self, thread_id: &str, token: &str) -> PrResult<()> {
        const MUTATION: &str =
            "mutation($id:ID!){resolveReviewThread(input:{threadId:$id}){thread{isResolved}}}";
        self.graphql(token, MUTATION, json!({"id": thread_id}))
            .await?;
        Ok(())
    }

    /// Comment on the conversation tab; how a review bot is asked to look again.
    pub async fn post_issue_comment(
        &self,
        reference: &PullRequestReference,
        body: &str,
        token: &str,
    ) -> PrResult<u64> {
        let comment = self
            .json(
                reqwest::Method::POST,
                &format!(
                    "{}/issues/{}/comments",
                    self.repos(&reference.owner, &reference.repository),
                    reference.number
                ),
                token,
                Some(&json!({"body": body})),
            )
            .await?;
        Ok(comment["id"].as_u64().unwrap_or(0))
    }

    /// Merge a pull request whose head is still `expected_head`.
    ///
    /// The REST merge honours branch protection; when it answers 405 and the
    /// caller allows it, the merge is asked for again through the mutation an
    /// administrator merges with, which succeeds for a token that may bypass
    /// the protection and refuses everyone else. A refusal there is reported
    /// as `Protected` with GitHub's own reason, and a head that moved as
    /// `HeadMoved`.
    #[allow(clippy::too_many_arguments)]
    pub async fn merge(
        &self,
        reference: &PullRequestReference,
        token: &str,
        node_id: Option<&str>,
        expected_head: &str,
        title: &str,
        message: &str,
        method: MergeMethod,
        admin: bool,
    ) -> PrResult<MergedPr> {
        let response = self
            .send(
                reqwest::Method::PUT,
                &format!("{}/merge", self.pulls(reference)),
                token,
                "application/vnd.github+json",
                Some(&json!({
                    "commit_title": title,
                    "commit_message": message,
                    "sha": expected_head,
                    "merge_method": method.rest(),
                })),
            )
            .await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            let merged: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            return Ok(MergedPr {
                sha: merged["sha"].as_str().unwrap_or_default().to_string(),
                admin: false,
            });
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(PrError::AuthFailed);
        }
        if status == reqwest::StatusCode::CONFLICT {
            return Err(PrError::HeadMoved);
        }
        let reason: String = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|body| body["message"].as_str().map(str::to_string))
            .unwrap_or(text);
        if status != reqwest::StatusCode::METHOD_NOT_ALLOWED {
            return Err(PrError::NotMergeable(format!("{status}: {reason}")));
        }
        // GitHub answers 405 both for a rule the token cannot bypass and for a
        // branch that no longer merges cleanly; only the former has an
        // administrator path, the latter needs the conflict repaired.
        if reason.to_ascii_lowercase().contains("not mergeable") {
            return Err(PrError::NotMergeable(format!("{status}: {reason}")));
        }
        let Some(node_id) = node_id.filter(|_| admin) else {
            return Err(PrError::Protected(reason));
        };
        const MUTATION: &str = "mutation($input:MergePullRequestInput!){mergePullRequest(input:$input){pullRequest{mergeCommit{oid}}}}";
        match self
            .graphql(
                token,
                MUTATION,
                json!({"input": {
                    "pullRequestId": node_id,
                    "mergeMethod": method.graphql(),
                    "commitHeadline": title,
                    "commitBody": message,
                    "expectedHeadOid": expected_head,
                }}),
            )
            .await
        {
            Ok(data) => Ok(MergedPr {
                sha: data["mergePullRequest"]["pullRequest"]["mergeCommit"]["oid"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                admin: true,
            }),
            Err(PrError::GitHubApi(message)) => Err(PrError::Protected(format!(
                "{reason}; administrator merge refused: {message}"
            ))),
            Err(error) => Err(error),
        }
    }

    /// Delete a branch; one already gone is not an error.
    pub async fn delete_branch(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
        token: &str,
    ) -> PrResult<()> {
        // The name lands in a URL path: every segment has to be a plain ref
        // segment, so `#`, `?` or an empty segment cannot redirect the request.
        if branch.is_empty() || branch.contains("..") || !branch.split('/').all(named) {
            return Err(PrError::GitHubApi(format!("Invalid branch: {branch}")));
        }
        let response = self
            .send(
                reqwest::Method::DELETE,
                &format!("{}/git/refs/heads/{branch}", self.repos(owner, repo)),
                token,
                "application/vnd.github+json",
                None,
            )
            .await?;
        let status = response.status();
        if status.is_success()
            || status == reqwest::StatusCode::NOT_FOUND
            || status == reqwest::StatusCode::UNPROCESSABLE_ENTITY
        {
            return Ok(());
        }
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(PrError::AuthFailed);
        }
        let text = response.text().await.unwrap_or_default();
        Err(PrError::GitHubApi(format!(
            "GitHub API returned {}: {}",
            status, text
        )))
    }
}

#[cfg(test)]
mod automation_tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn reference() -> PullRequestReference {
        PullRequestReference {
            owner: "acme".to_string(),
            repository: "project".to_string(),
            number: 7,
        }
    }

    #[tokio::test]
    async fn checks_fold_runs_and_statuses_into_one_outcome() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());
        let sha = "0123456789abcdef0123456789abcdef01234567";

        Mock::given(method("GET"))
            .and(path(format!(
                "/repos/acme/project/commits/{sha}/check-runs"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "check_runs": [
                    {"name": "build", "status": "completed", "conclusion": "success"},
                    {"name": "lint", "status": "completed", "conclusion": "skipped"},
                    {"name": "test", "status": "in_progress", "conclusion": null}
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/repos/acme/project/commits/{sha}/status")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "state": "pending",
                "statuses": [{"context": "ci/deploy", "state": "success"}]
            })))
            .mount(&server)
            .await;

        let outcome = service
            .fetch_checks("acme", "project", sha, "token")
            .await
            .unwrap();
        assert_eq!(
            outcome,
            ChecksOutcome::Pending,
            "a run still going is pending"
        );
    }

    #[tokio::test]
    async fn checks_follow_every_page_before_judging() {
        use wiremock::matchers::query_param;
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());
        let sha = "3333333333333333333333333333333333333333";
        let full_page: Vec<serde_json::Value> = (0..PAGE_SIZE)
            .map(|index| json!({"name": format!("check-{index}"), "status": "completed", "conclusion": "success"}))
            .collect();

        Mock::given(method("GET"))
            .and(path(format!(
                "/repos/acme/project/commits/{sha}/check-runs"
            )))
            .and(query_param("page", "1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"check_runs": full_page})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/repos/acme/project/commits/{sha}/check-runs"
            )))
            .and(query_param("page", "2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "check_runs": [{"name": "deploy", "status": "completed", "conclusion": "failure"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/repos/acme/project/commits/{sha}/status")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "state": "success",
                "statuses": []
            })))
            .mount(&server)
            .await;

        assert_eq!(
            service
                .fetch_checks("acme", "project", sha, "token")
                .await
                .unwrap(),
            ChecksOutcome::Failure(vec!["deploy".to_string()]),
            "the failure on the second page decides the outcome"
        );
    }

    #[tokio::test]
    async fn a_conflicted_pull_request_is_not_merged_as_an_administrator() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());
        Mock::given(method("PUT"))
            .and(path("/repos/acme/project/pulls/7/merge"))
            .respond_with(
                ResponseTemplate::new(405)
                    .set_body_json(json!({"message": "Pull Request is not mergeable"})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {}})))
            .expect(0)
            .mount(&server)
            .await;

        let error = service
            .merge(
                &reference(),
                "token",
                Some("PR_node"),
                "0123456789abcdef0123456789abcdef01234567",
                "Title",
                "Body",
                MergeMethod::Squash,
                true,
            )
            .await
            .expect_err("a conflict is not merged");
        assert!(
            matches!(error, PrError::NotMergeable(ref reason) if reason.contains("not mergeable")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_branch_that_could_change_the_request_target_is_refused() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());
        for branch in [
            "feature#x",
            "a?b=c",
            "zone//task",
            "trailing/",
            "/leading",
            "a/../b",
        ] {
            let error = service
                .delete_branch("acme", "project", branch, "token")
                .await
                .expect_err(branch);
            assert!(
                matches!(error, PrError::GitHubApi(ref message) if message.contains("Invalid branch")),
                "{branch}: {error:?}"
            );
        }
        assert!(
            server.received_requests().await.unwrap().is_empty(),
            "nothing reached GitHub"
        );
    }

    #[tokio::test]
    async fn a_failed_status_names_itself_and_silence_is_absent() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());
        let failing = "1111111111111111111111111111111111111111";
        let silent = "2222222222222222222222222222222222222222";

        for (sha, runs, statuses) in [
            (
                failing,
                json!({"check_runs": [{"name": "test", "status": "completed", "conclusion": "failure"}]}),
                json!({"state": "failure", "statuses": [{"context": "ci/deploy", "state": "error"}]}),
            ),
            (
                silent,
                json!({"check_runs": []}),
                json!({"state": "pending", "statuses": []}),
            ),
        ] {
            Mock::given(method("GET"))
                .and(path(format!(
                    "/repos/acme/project/commits/{sha}/check-runs"
                )))
                .respond_with(ResponseTemplate::new(200).set_body_json(runs))
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path(format!("/repos/acme/project/commits/{sha}/status")))
                .respond_with(ResponseTemplate::new(200).set_body_json(statuses))
                .mount(&server)
                .await;
        }

        assert_eq!(
            service
                .fetch_checks("acme", "project", failing, "token")
                .await
                .unwrap(),
            ChecksOutcome::Failure(vec!["ci/deploy".to_string(), "test".to_string()])
        );
        assert_eq!(
            service
                .fetch_checks("acme", "project", silent, "token")
                .await
                .unwrap(),
            ChecksOutcome::Absent
        );
    }

    #[tokio::test]
    async fn a_protected_merge_is_retried_through_the_administrator_mutation() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());

        Mock::given(method("PUT"))
            .and(path("/repos/acme/project/pulls/7/merge"))
            .respond_with(ResponseTemplate::new(405).set_body_json(json!({
                "message": "At least 1 approving review is required by reviewers with write access."
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(json!({"variables": {"input": {
                "pullRequestId": "PR_node",
                "mergeMethod": "SQUASH",
                "expectedHeadOid": "abc"
            }}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"mergePullRequest": {"pullRequest": {"mergeCommit": {"oid": "def"}}}}
            })))
            .mount(&server)
            .await;

        let merged = service
            .merge(
                &reference(),
                "token",
                Some("PR_node"),
                "abc",
                "title",
                "body",
                MergeMethod::Squash,
                true,
            )
            .await
            .unwrap();
        assert_eq!(
            merged,
            MergedPr {
                sha: "def".to_string(),
                admin: true
            }
        );

        let refused = service
            .merge(
                &reference(),
                "token",
                Some("PR_node"),
                "abc",
                "title",
                "body",
                MergeMethod::Squash,
                false,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(&refused, PrError::Protected(reason) if reason.contains("approving review")),
            "{refused:?}"
        );
    }

    #[tokio::test]
    async fn a_refused_administrator_merge_carries_both_reasons_and_a_moved_head_is_named() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());

        Mock::given(method("PUT"))
            .and(path("/repos/acme/project/pulls/7/merge"))
            .respond_with(
                ResponseTemplate::new(405).set_body_json(
                    json!({"message": "Required status check \"test\" is expected."}),
                ),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": null,
                "errors": [{"message": "Base branch was modified. Review and try the merge again."}]
            })))
            .mount(&server)
            .await;

        let refused = service
            .merge(
                &reference(),
                "token",
                Some("PR_node"),
                "abc",
                "t",
                "b",
                MergeMethod::Squash,
                true,
            )
            .await
            .unwrap_err();
        match refused {
            PrError::Protected(reason) => {
                assert!(reason.contains("Required status check"), "{reason}");
                assert!(reason.contains("Base branch was modified"), "{reason}");
            }
            other => panic!("{other:?}"),
        }

        let moved = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/repos/acme/project/pulls/7/merge"))
            .respond_with(
                ResponseTemplate::new(409)
                    .set_body_json(json!({"message": "Head branch was modified."})),
            )
            .mount(&moved)
            .await;
        let service = PrService::standing_in_for("github.com", moved.uri());
        assert!(matches!(
            service
                .merge(
                    &reference(),
                    "token",
                    None,
                    "abc",
                    "t",
                    "b",
                    MergeMethod::Squash,
                    true
                )
                .await,
            Err(PrError::HeadMoved)
        ));
    }

    #[tokio::test]
    async fn review_threads_are_read_with_their_comments_and_resolved_by_id() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());

        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(json!({"variables": {"number": 7}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"repository": {"pullRequest": {"reviewThreads": {
                    "pageInfo": {"hasNextPage": false, "endCursor": null},
                    "nodes": [{
                        "id": "PRRT_1", "isResolved": false, "isOutdated": false,
                        "path": "src/cart.ts", "line": 12,
                        "comments": {"nodes": [{
                            "databaseId": 99, "body": "handle the empty cart", "url": "https://github.com/acme/project/pull/7#discussion_r99",
                            "createdAt": "2026-09-20T10:00:00Z", "author": {"login": "coderabbitai"}
                        }]}
                    }]
                }}}}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(json!({"variables": {"id": "PRRT_1"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"resolveReviewThread": {"thread": {"isResolved": true}}}
            })))
            .mount(&server)
            .await;

        let threads = service
            .fetch_review_threads(&reference(), "token")
            .await
            .unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].id, "PRRT_1");
        assert_eq!(threads[0].path.as_deref(), Some("src/cart.ts"));
        assert_eq!(threads[0].line, Some(12));
        assert_eq!(threads[0].comments[0].author, "coderabbitai");
        assert_eq!(threads[0].comments[0].database_id, Some(99));
        service
            .resolve_review_thread("PRRT_1", "token")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_repository_is_created_under_the_organisation_or_the_user() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());

        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"login": "ada"})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/orgs/acme/repos"))
            .and(body_partial_json(
                json!({"name": "shop", "auto_init": true, "private": true}),
            ))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "html_url": "https://github.com/acme/shop",
                "clone_url": "https://github.com/acme/shop.git",
                "default_branch": "main"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/user/repos"))
            .respond_with(ResponseTemplate::new(422).set_body_json(json!({
                "message": "Repository creation failed.",
                "errors": [{"message": "name already exists on this account"}]
            })))
            .mount(&server)
            .await;

        let created = service
            .create_repository("token", Some("acme"), "shop", "A shop", true)
            .await
            .unwrap();
        assert_eq!(created.html_url, "https://github.com/acme/shop");
        assert_eq!(created.default_branch, "main");

        let taken = service
            .create_repository("token", Some("ada"), "shop", "A shop", false)
            .await
            .unwrap_err();
        assert!(matches!(taken, PrError::RepositoryExists(name) if name == "shop"));
        assert!(matches!(
            service
                .create_repository("token", None, "../x", "", false)
                .await,
            Err(PrError::GitHubApi(_))
        ));
    }

    #[tokio::test]
    async fn the_diff_is_cut_at_the_budget_and_a_deleted_branch_is_tolerated() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());

        Mock::given(method("GET"))
            .and(path("/repos/acme/project/pulls/7"))
            .and(header("Accept", "application/vnd.github.diff"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("diff --git a/x b/x\n+".repeat(20)),
            )
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/repos/acme/project/git/refs/heads/zone/task-1"))
            .respond_with(
                ResponseTemplate::new(422)
                    .set_body_json(json!({"message": "Reference does not exist"})),
            )
            .mount(&server)
            .await;

        let diff = service.fetch_diff(&reference(), "token", 50).await.unwrap();
        assert!(diff.ends_with(TRUNCATED), "{diff}");
        assert!(diff.len() <= 50 + TRUNCATED.len());
        service
            .delete_branch("acme", "project", "zone/task-1", "token")
            .await
            .unwrap();
        assert!(repository_path("../etc/passwd").is_err());
        assert_eq!(repository_path("/src/a.rs/").unwrap(), "src/a.rs");
    }

    #[tokio::test]
    async fn a_review_is_submitted_as_a_comment_with_its_commit() {
        let server = MockServer::start().await;
        let service = PrService::standing_in_for("github.com", server.uri());
        Mock::given(method("POST"))
            .and(path("/repos/acme/project/pulls/7/reviews"))
            .and(body_partial_json(
                json!({"event": "COMMENT", "commit_id": "abc"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 5})))
            .mount(&server)
            .await;
        assert_eq!(
            service
                .submit_review(
                    &reference(),
                    "token",
                    "abc",
                    ReviewEvent::Comment,
                    "Zone review"
                )
                .await
                .unwrap(),
            5
        );
    }
}
