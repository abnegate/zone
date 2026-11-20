//! A mounted folder whose chat agents keep worktrees under `.worktrees/`, and the rule that
//! decides which of them can go.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use abnegate_vcs::worktree::is_worktree;
use tokio::io::AsyncWriteExt;

use super::entry::Entry;
use super::git;
use super::merge::merged;
use super::tally::Tally;
use super::verdict::Verdict;

/// The directory, inside a folder, that the chat prompt tells agents to add worktrees under.
pub const AREA: &str = ".worktrees";

/// Where the default branch is looked for, in order, when `origin/HEAD` names none.
const DEFAULT_BRANCHES: [&str; 2] = ["refs/remotes/origin/main", "refs/remotes/origin/master"];

#[derive(Debug, Clone)]
pub struct Repository {
    folder: PathBuf,
    area: PathBuf,
}

impl Repository {
    /// The folder, when it holds a `.worktrees/` directory of its own, not a link to one
    /// elsewhere, and lies in a git repository.
    pub async fn open(folder: &Path) -> Option<Self> {
        let area = folder.join(AREA);
        if !tokio::fs::symlink_metadata(&area).await.ok()?.is_dir() {
            return None;
        }
        let area = tokio::fs::canonicalize(area).await.ok()?;
        let inside = git::read(folder, &["rev-parse", "--is-inside-work-tree"]).await;
        (inside.ok()? == "true").then(|| Self {
            folder: folder.to_path_buf(),
            area,
        })
    }

    /// Remove every worktree under `.worktrees/` that [`Self::judge`] finds removable, after
    /// making sure the host's own `git status` does not list them. A failure is logged and
    /// counted, never returned, so one worktree cannot stop the rest.
    pub async fn sweep(&self, idle: Duration, now: SystemTime) -> Tally {
        let mut tally = Tally::default();
        match self.exclude().await {
            Ok(true) => tracing::info!(
                folder = %self.folder.display(),
                "Excluded agent worktrees from the repository's status"
            ),
            Ok(false) => {}
            Err(error) => tracing::warn!(
                folder = %self.folder.display(),
                %error,
                "Could not exclude agent worktrees from the repository's status"
            ),
        }
        let entries = match self.worktrees().await {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(
                    folder = %self.folder.display(),
                    %error,
                    "Could not list the repository's worktrees"
                );
                tally.failed += 1;
                return tally;
            }
        };
        if entries.is_empty() {
            return tally;
        }
        let target = match self.default_branch().await {
            Ok(target) => target,
            Err(error) => {
                tracing::warn!(
                    folder = %self.folder.display(),
                    %error,
                    "Could not read the repository's default branch"
                );
                tally.failed += 1;
                return tally;
            }
        };
        for entry in entries {
            let verdict = self.judge(&entry, target.as_deref(), idle, now).await;
            match verdict {
                Ok(Verdict::Removable) => match self.remove(&entry).await {
                    Ok(()) => {
                        tracing::info!(
                            worktree = %entry.path.display(),
                            "Removed an agent worktree whose work is merged"
                        );
                        tally.removed += 1;
                    }
                    Err(error) => {
                        tracing::warn!(
                            worktree = %entry.path.display(),
                            %error,
                            "Could not remove an agent worktree whose work is merged"
                        );
                        tally.failed += 1;
                    }
                },
                Ok(kept) => {
                    tracing::debug!(
                        worktree = %entry.path.display(),
                        reason = kept.reason(),
                        "Kept an agent worktree"
                    );
                    tally.kept += 1;
                }
                Err(error) => {
                    tracing::warn!(
                        worktree = %entry.path.display(),
                        %error,
                        "Could not judge whether an agent worktree can be removed"
                    );
                    tally.failed += 1;
                }
            }
        }
        tally
    }

    /// Make sure the repository's `info/exclude` ignores this folder's `.worktrees/`, returning
    /// whether the line had to be added.
    pub async fn exclude(&self) -> io::Result<bool> {
        let common = git::read(
            &self.folder,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .await?;
        let prefix = git::read(&self.folder, &["rev-parse", "--show-prefix"]).await?;
        let pattern = format!("/{prefix}{AREA}/");
        let information = PathBuf::from(common).join("info");
        let path = information.join("exclude");
        let existing = match tokio::fs::read_to_string(&path).await {
            Ok(existing) => existing,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        if existing.lines().any(|line| line.trim() == pattern) {
            return Ok(false);
        }
        let separator = if existing.is_empty() || existing.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        tokio::fs::create_dir_all(&information).await?;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        file.write_all(format!("{separator}{pattern}\n").as_bytes())
            .await?;
        Ok(true)
    }

    /// The remote's default branch as a full ref: what `origin/HEAD` names, else `origin/main`,
    /// else `origin/master`, whichever resolves to a commit first.
    pub async fn default_branch(&self) -> io::Result<Option<String>> {
        let named = git::output(
            &self.folder,
            &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
        )
        .await?;
        let named = named
            .status
            .success()
            .then(|| String::from_utf8_lossy(&named.stdout).trim().to_string());
        let candidates = named
            .into_iter()
            .chain(DEFAULT_BRANCHES.iter().map(|branch| branch.to_string()));
        for candidate in candidates {
            let commit = format!("{candidate}^{{commit}}");
            let resolved =
                git::output(&self.folder, &["rev-parse", "--verify", "--quiet", &commit]).await?;
            if resolved.status.success() {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    /// Every linked worktree that stands under this folder's `.worktrees/`.
    pub async fn worktrees(&self) -> io::Result<Vec<Entry>> {
        let listing = git::output(&self.folder, &["worktree", "list", "--porcelain", "-z"]).await?;
        if !listing.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&listing.stderr).trim().to_string(),
            ));
        }
        let mut inside = Vec::new();
        for mut entry in Entry::parse(&listing.stdout) {
            let Ok(path) = tokio::fs::canonicalize(&entry.path).await else {
                continue;
            };
            if path.starts_with(&self.area) && path != self.area && is_worktree(&path) {
                entry.path = path;
                inside.push(entry);
            }
        }
        Ok(inside)
    }

    /// Whether `entry` can be removed: unlocked, untouched for `idle`, clean, and merged into
    /// `target`. The cheap checks run first, and the first that fails names the verdict.
    pub async fn judge(
        &self,
        entry: &Entry,
        target: Option<&str>,
        idle: Duration,
        now: SystemTime,
    ) -> io::Result<Verdict> {
        if entry.locked {
            return Ok(Verdict::Locked);
        }
        let active = last_active(&entry.path).await?;
        if now.duration_since(active).unwrap_or_default() < idle {
            return Ok(Verdict::Active);
        }
        let Some(target) = target else {
            return Ok(Verdict::NoDefaultBranch);
        };
        let status = git::read(
            &entry.path,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=normal",
                "--ignore-submodules=none",
            ],
        )
        .await?;
        if !status.is_empty() {
            return Ok(Verdict::Dirty);
        }
        if !merged(&self.folder, &entry.head, target).await? {
            return Ok(Verdict::Unmerged);
        }
        Ok(Verdict::Removable)
    }

    /// `git worktree remove` without `--force`, so git refuses a worktree that changed since it
    /// was judged, then every directory a branch name's prefix made that is now empty. The branch
    /// it was on is left where it is. A worktree whose HEAD moved since it was judged is left
    /// too: its new commit was never checked against the default branch.
    pub async fn remove(&self, entry: &Entry) -> io::Result<()> {
        let head = git::read(&entry.path, &["rev-parse", "HEAD"]).await?;
        if head != entry.head {
            return Err(io::Error::other(format!(
                "HEAD moved from {} to {head} since the worktree was judged",
                entry.head
            )));
        }
        let path = entry.path.to_string_lossy();
        git::read(&self.folder, &["worktree", "remove", "--", &path]).await?;
        let mut parent = entry.path.parent();
        while let Some(directory) = parent.filter(|directory| *directory != self.area) {
            if tokio::fs::remove_dir(directory).await.is_err() {
                break;
            }
            parent = directory.parent();
        }
        Ok(())
    }
}

/// When the worktree's HEAD last moved: the newer of its admin `HEAD` and `logs/HEAD`, which
/// every checkout, commit and reset touches.
async fn last_active(worktree: &Path) -> io::Result<SystemTime> {
    let administration = git::read(worktree, &["rev-parse", "--absolute-git-dir"]).await?;
    let administration = PathBuf::from(administration);
    let head = tokio::fs::metadata(administration.join("HEAD"))
        .await?
        .modified()?;
    let logged = match tokio::fs::metadata(administration.join("logs").join("HEAD")).await {
        Ok(metadata) => Some(metadata.modified()?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    Ok(logged.map_or(head, |logged| logged.max(head)))
}
