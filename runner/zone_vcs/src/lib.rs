//! Local git operations and GitHub pull requests.
//!
//! [`git::GitService`] shells out to `git` for the branch, commit, and push a
//! change needs; [`pull_request::PrService`] opens the pull request over the
//! GitHub API. Neither holds application state, so a task runner can drive them
//! directly.
//!
//! ```no_run
//! # async fn example(repo: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
//! use uuid::Uuid;
//! use zone_vcs::{GitService, PrService};
//!
//! let git = GitService::new();
//! let task = Uuid::new_v4();
//! let branch = git.generate_branch_name(task, "add rate limiting");
//! # let _ = (git.is_git_repo(repo).await?, branch);
//!
//! let pulls = PrService::new();
//! let (owner, name) = pulls.parse_github_url("https://github.com/owner/repo")?;
//! # let _ = (owner, name);
//! # Ok(())
//! # }
//! ```

pub mod conflict;
pub mod git;
pub mod pull_request;
pub mod resolution;
pub mod subject;

pub use conflict::{
    BranchName, CommitSha, Conflict, ConflictError, ConflictRequest, ConflictService,
    ConflictedPath,
};
pub use git::{DiffSummary, GitError, GitService};
pub use pull_request::{
    CreatedPr, Description, GitHubBranch, GitHubPullRequest, PrError, PrService,
    PullRequestReception, PullRequestReference,
};
pub use resolution::{ConflictSide, ResolutionVerdict};
pub use subject::{Kind, Subject};
