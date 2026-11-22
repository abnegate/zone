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
    projects, sources,
    tasks::{self, TaskRow},
    users,
};
use crate::services::github_app::{Credential, Error as GitHubError, Provider};

const ROOT: &str = "zone-checkouts-v1";

const PROJECT_TOKEN_UNOPENABLE: &str =
    "The project's GitHub token is sealed under a key this deployment does not hold";

const SOURCE_TOKEN_UNOPENABLE: &str =
    "The source's GitHub token is sealed under a key this deployment does not hold";

pub(crate) const AUTHOR_NAME: &str = "Zone";

pub(crate) const AUTHOR_EMAIL: &str = "zone@users.noreply.github.com";

const BRANCH_PREFIX: &str = "zone/task";

const LOST_LEASE: &str = "Task execution lost its lease";

pub(crate) const MISMATCHED_ORIGIN: &str =
    "Checkout repository no longer matches the task repository";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    pub name: String,
    pub email: String,
}

impl Author {
    pub async fn of(pool: &PgPool, user: Option<Uuid>) -> Option<Self> {
        let row = users::get_user_by_id(pool, user?).await.ok()??;
        let name = row
            .display_name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| local_part(&row.email).to_string());
        Some(Self {
            name,
            email: row.email,
        })
    }
}

fn local_part(email: &str) -> &str {
    email.split('@').next().unwrap_or(email)
}

pub fn git() -> GitService {
    git_as(None)
}

pub fn git_as(author: Option<&Author>) -> GitService {
    let (name, email) = author
        .map(|author| (author.name.as_str(), author.email.as_str()))
        .unwrap_or((AUTHOR_NAME, AUTHOR_EMAIL));
    GitService::new()
        .with_author(name, email)
        .with_branch_prefix(
            BranchName::parse(BRANCH_PREFIX).expect("the branch prefix is a branch name"),
        )
        .with_branch_format(BranchFormat::Joined)
}

pub async fn sanitize(base: &Path, origin: &RepositoryUrl) -> Result<(), String> {
    let _lock = Checkout::lock_base(base).await?;
    restore(base, origin).await
}

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
    pub token: Option<Credential>,
}
