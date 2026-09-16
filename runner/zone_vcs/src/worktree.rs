//! Worktrees for task runs: one base clone per repository, one detached
//! worktree per run, and a removal rule that never throws work away.
//!
//! CC 349-410 and 1487-1640, and CX 19: a run works in a worktree of its own
//! under a directory the host chose, the worktree is cleaned up when it is
//! unchanged, and one that still holds work nobody has is refused rather than
//! removed. "Work nobody has" is read from git itself — changes no commit
//! holds, and commits no remote-tracking ref holds — so the rule does not
//! depend on the run remembering what it did.
//!
//! Everything here is local to the disk and synchronous, so it can run inside
//! a `Drop` as well as under `spawn_blocking`; the one network step, fetching
//! the base clone, stays on [`crate::git::GitService`] with its timeout.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What removing a worktree would lose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unfinished {
    /// Changes in the working tree or the index that no commit holds.
    pub uncommitted: bool,
    /// Commits reachable from HEAD that no remote-tracking ref holds: work
    /// that was committed and never published.
    pub unpublished: bool,
}

impl Unfinished {
    pub fn any(self) -> bool {
        self.uncommitted || self.unpublished
    }
}

/// A git invocation that reads nothing from the host's configuration and
/// never talks to the network: the hardening `GitService` applies, for the
/// local operations that need no timeout.
fn local(repository: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_GRAFT_FILE", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["-c", "core.hooksPath=/dev/null"])
        .current_dir(repository)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn run(command: &mut Command, what: &str) -> std::io::Result<Vec<u8>> {
    let output = command.output()?;
    if !output.status.success() {
        // Git's stderr can quote paths and refs a caller supplied; the
        // operation is named instead, and the caller knows the path.
        return Err(std::io::Error::other(format!("git could not {what}")));
    }
    Ok(output.stdout)
}

/// Add a detached worktree of `repository` at `path`, checked out at `start`.
/// Detached, because the run's own branch is made afterwards by the same
/// step that makes it in a clone, and a worktree that started on a named
/// branch would pin that branch to itself.
pub fn add(repository: &Path, path: &Path, start: &str) -> std::io::Result<()> {
    run(
        local(repository)
            .args(["worktree", "add", "--detach", "--"])
            .arg(path)
            .arg(start),
        "add a worktree",
    )
    .map(drop)
}

/// Whether `path` is a worktree rather than a clone of its own: git marks
/// one with a `.git` file pointing at the repository, where a clone has a
/// `.git` directory.
pub fn is_worktree(path: &Path) -> bool {
    path.join(".git").is_file()
}

/// What this worktree holds that nothing else does.
pub fn unfinished(path: &Path) -> std::io::Result<Unfinished> {
    let status = run(
        local(path).args(["status", "--porcelain"]),
        "read the worktree's status",
    )?;
    let ahead = run(
        local(path).args(["rev-list", "--count", "HEAD", "--not", "--remotes"]),
        "read the worktree's history",
    )?;
    let ahead: u64 = String::from_utf8_lossy(&ahead)
        .trim()
        .parse()
        .map_err(|_| std::io::Error::other("git could not count the worktree's commits"))?;
    Ok(Unfinished {
        uncommitted: !status.is_empty(),
        unpublished: ahead > 0,
    })
}

/// The branch the worktree is on, or `None` when it is detached.
pub fn branch(path: &Path) -> std::io::Result<Option<String>> {
    let name = run(
        local(path).args(["symbolic-ref", "--quiet", "--short", "HEAD"]),
        "read the worktree's branch",
    )
    .ok()
    .map(|out| String::from_utf8_lossy(&out).trim().to_string())
    .filter(|name| !name.is_empty());
    Ok(name)
}

/// Remove a worktree whether or not it is clean — the caller has decided,
/// on [`unfinished`], that nothing in it is lost — and prune the repository's
/// record of it. A branch the worktree was on is deleted with it: its commits
/// are on the remote, that is what clean means, and a local ref left behind
/// would refuse the next run of the same task its own branch.
pub fn remove(repository: &Path, path: &Path) -> std::io::Result<()> {
    let on = branch(path).unwrap_or(None);
    run(
        local(repository)
            .args(["worktree", "remove", "--force", "--"])
            .arg(path),
        "remove the worktree",
    )?;
    let _ = run(
        local(repository).args(["worktree", "prune"]),
        "prune worktrees",
    );
    if let Some(name) = on
        && let Err(error) = run(
            local(repository).args(["branch", "-D", "--", &name]),
            "delete the branch",
        )
    {
        // The worktree is gone and nothing holds the branch now; the next run
        // that asks for the name finds it unheld and takes it over.
        tracing::warn!(branch = %name, %error, "Removed a worktree but could not delete its branch");
    }
    Ok(())
}

/// The repository a worktree belongs to: the directory holding the `.git`
/// its `.git` file points into.
pub fn repository_of(path: &Path) -> std::io::Result<PathBuf> {
    let common = run(
        local(path).args(["rev-parse", "--path-format=absolute", "--git-common-dir"]),
        "find the worktree's repository",
    )?;
    let common = PathBuf::from(String::from_utf8_lossy(&common).trim());
    common
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| std::io::Error::other("git named a repository with no parent"))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::path::Path;
    use std::process::Command;

    /// Run git in a fixture with a fixed identity, panicking on failure.
    pub fn git(path: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
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
            "git {arguments:?} in {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// A repository with one commit on `main`, the shape a remote has.
    pub fn remote(path: &Path) {
        git(path, &["init", "-q", "-b", "main"]);
        std::fs::write(path.join("README"), "fixture\n").unwrap();
        git(path, &["add", "README"]);
        git(path, &["commit", "-q", "-m", "fixture"]);
    }

    /// A clone of `remote` at `path`, as the base clone of a repository is.
    pub fn clone(remote: &Path, path: &Path) {
        git(
            remote.parent().unwrap(),
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                path.to_str().unwrap(),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{clone, git, remote};
    use super::*;

    struct Repositories {
        _root: tempfile::TempDir,
        remote: PathBuf,
        base: PathBuf,
        worktrees: PathBuf,
    }

    fn repositories() -> Repositories {
        let root = tempfile::tempdir().unwrap();
        let remote_path = root.path().join("remote");
        std::fs::create_dir(&remote_path).unwrap();
        remote(&remote_path);
        let base = root.path().join("base");
        clone(&remote_path, &base);
        let worktrees = root.path().join("worktrees");
        std::fs::create_dir(&worktrees).unwrap();
        Repositories {
            _root: root,
            remote: remote_path,
            base,
            worktrees,
        }
    }

    #[test]
    fn a_worktree_starts_detached_at_the_remote_head_and_clean() {
        let repositories = repositories();
        let path = repositories.worktrees.join("run");
        add(&repositories.base, &path, "origin/HEAD").unwrap();
        assert!(is_worktree(&path), "a worktree is marked by a .git file");
        assert!(!is_worktree(&repositories.base), "the base is a clone");
        assert_eq!(
            std::fs::read_to_string(path.join("README")).unwrap(),
            "fixture\n"
        );
        assert_eq!(
            branch(&path).unwrap(),
            None,
            "detached, so no branch is pinned"
        );
        assert_eq!(unfinished(&path).unwrap(), Unfinished::default());
        // Git names the repository by its real path, which on macOS is not
        // the path the temporary directory was handed out under.
        assert_eq!(
            repository_of(&path).unwrap(),
            repositories.base.canonicalize().unwrap()
        );
    }

    #[test]
    fn work_nobody_else_has_is_seen_at_each_stage_and_cleared_by_publication() {
        let repositories = repositories();
        let path = repositories.worktrees.join("run");
        add(&repositories.base, &path, "origin/HEAD").unwrap();
        git(&path, &["checkout", "-q", "-b", "task/one"]);
        std::fs::write(path.join("work.txt"), "in progress\n").unwrap();
        assert_eq!(
            unfinished(&path).unwrap(),
            Unfinished {
                uncommitted: true,
                unpublished: false
            }
        );
        git(&path, &["add", "work.txt"]);
        git(&path, &["commit", "-q", "-m", "work"]);
        assert_eq!(
            unfinished(&path).unwrap(),
            Unfinished {
                uncommitted: false,
                unpublished: true
            },
            "a commit the remote does not have is unpublished"
        );
        // Publication as the service performs it: a push to the remote, and the
        // remote-tracking ref moved to what was pushed.
        git(&path, &["push", "-q", "origin", "HEAD:refs/heads/task/one"]);
        git(
            &path,
            &["update-ref", "refs/remotes/origin/task/one", "HEAD"],
        );
        assert_eq!(unfinished(&path).unwrap(), Unfinished::default());
        assert_eq!(branch(&path).unwrap().as_deref(), Some("task/one"));
        assert_eq!(
            git(&repositories.remote, &["rev-parse", "task/one"]),
            git(&path, &["rev-parse", "HEAD"])
        );
    }

    #[test]
    fn removing_a_worktree_takes_its_branch_and_leaves_the_base_usable() {
        let repositories = repositories();
        let first = repositories.worktrees.join("first");
        add(&repositories.base, &first, "origin/HEAD").unwrap();
        git(&first, &["checkout", "-q", "-b", "task/one"]);
        std::fs::write(first.join("work.txt"), "x\n").unwrap();
        remove(&repositories.base, &first).unwrap();
        assert!(
            !first.exists(),
            "removed even with an uncommitted file: the caller decided"
        );
        let listed = git(&repositories.base, &["worktree", "list", "--porcelain"]);
        assert!(!listed.contains("first"), "{listed}");
        let branches = git(&repositories.base, &["branch", "--list", "task/one"]);
        assert_eq!(branches, "", "the branch went with the worktree");
        // The next run of the same task can take the branch name again.
        let second = repositories.worktrees.join("second");
        add(&repositories.base, &second, "origin/HEAD").unwrap();
        git(&second, &["checkout", "-q", "-b", "task/one"]);
        assert_eq!(branch(&second).unwrap().as_deref(), Some("task/one"));
    }

    #[test]
    fn a_missing_worktree_is_an_error_and_not_a_panic() {
        let repositories = repositories();
        let missing = repositories.worktrees.join("missing");
        assert!(unfinished(&missing).is_err());
        assert!(remove(&repositories.base, &missing).is_err());
        assert!(repository_of(&missing).is_err());
    }
}
