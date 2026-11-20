//! The sweep across every workspace's folders.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::repository::Repository;
use super::tally::Tally;
use crate::db::DbResult;
use crate::db::workspace_host_directories;
use crate::state::AppState;

pub const SWEEP_INTERVAL_SECONDS: u64 = 60 * 60;

/// How long a worktree's HEAD has to stay where it is before the worktree can go. A worktree
/// just added off the default branch is trivially merged into it.
pub const IDLE: Duration = Duration::from_secs(24 * 60 * 60);

/// Clean every folder any workspace mounted, each once, as this process sees it.
pub async fn sweep(state: &AppState) -> DbResult<Tally> {
    let directories = workspace_host_directories::all(state.db()).await?;
    let mounts = &state.config().host_mounts;
    let folders: BTreeSet<PathBuf> = directories
        .iter()
        .filter_map(|directory| mounts.map(directory).ok())
        .collect();
    let now = SystemTime::now();
    let mut tally = Tally::default();
    for folder in folders {
        tally += clean(&folder, IDLE, now).await;
    }
    Ok(tally)
}

/// Remove the removable worktrees under one folder's `.worktrees/`. A folder that is not in a
/// repository, or has no `.worktrees/`, has nothing to clean.
pub async fn clean(folder: &Path, idle: Duration, now: SystemTime) -> Tally {
    match Repository::open(folder).await {
        Some(repository) => repository.sweep(idle, now).await,
        None => Tally::default(),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::services::worktree::verdict::Verdict;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    fn run(directory: &Path, arguments: &[&str]) -> String {
        let mut command = Command::new("git");
        for variable in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
        ] {
            command.env_remove(variable);
        }
        let output = command
            .args(arguments)
            .current_dir(directory)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit(directory: &Path, file: &str, contents: &str) {
        std::fs::write(directory.join(file), contents).unwrap();
        run(directory, &["add", file]);
        run(directory, &["commit", "-q", "-m", file]);
    }

    /// A bare origin, an upstream clone that plays GitHub's merge button, and the user's clone
    /// the agents add worktrees to.
    struct Fixture {
        _root: TempDir,
        upstream: PathBuf,
        checkout: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let origin = root.path().join("origin.git");
            let upstream = root.path().join("upstream");
            let checkout = root.path().join("checkout");
            run(
                root.path(),
                &[
                    "init",
                    "-q",
                    "--bare",
                    "-b",
                    "main",
                    origin.to_str().unwrap(),
                ],
            );
            run(
                root.path(),
                &[
                    "clone",
                    "-q",
                    origin.to_str().unwrap(),
                    upstream.to_str().unwrap(),
                ],
            );
            run(&upstream, &["symbolic-ref", "HEAD", "refs/heads/main"]);
            commit(&upstream, ".gitignore", "target/\n");
            commit(&upstream, "README.md", "zone\n");
            run(&upstream, &["push", "-q", "origin", "main"]);
            run(
                root.path(),
                &[
                    "clone",
                    "-q",
                    origin.to_str().unwrap(),
                    checkout.to_str().unwrap(),
                ],
            );
            Self {
                _root: root,
                upstream,
                checkout,
            }
        }

        fn worktree(&self, branch: &str) -> PathBuf {
            let path = format!(".worktrees/{branch}");
            run(
                &self.checkout,
                &["worktree", "add", "-q", &path, "-b", branch, "origin/main"],
            );
            self.checkout.join(path)
        }

        fn work(&self, branch: &str) -> PathBuf {
            let worktree = self.worktree(branch);
            let file = format!("{}.txt", branch.replace('/', "-"));
            commit(&worktree, &file, "first\n");
            commit(&worktree, &file, "first\nsecond\n");
            run(&worktree, &["push", "-q", "origin", branch]);
            worktree
        }

        fn advance_main(&self, file: &str) {
            commit(&self.upstream, file, "elsewhere\n");
            run(&self.upstream, &["push", "-q", "origin", "main"]);
        }

        fn squash(&self, branch: &str) {
            run(&self.upstream, &["fetch", "-q", "origin"]);
            run(
                &self.upstream,
                &["merge", "-q", "--squash", &format!("origin/{branch}")],
            );
            run(&self.upstream, &["commit", "-q", "-m", "squashed"]);
            run(&self.upstream, &["push", "-q", "origin", "main"]);
            run(&self.checkout, &["fetch", "-q", "origin"]);
        }

        fn fast_forward(&self, branch: &str) {
            run(&self.upstream, &["fetch", "-q", "origin"]);
            run(
                &self.upstream,
                &["merge", "-q", "--ff-only", &format!("origin/{branch}")],
            );
            run(&self.upstream, &["push", "-q", "origin", "main"]);
            run(&self.checkout, &["fetch", "-q", "origin"]);
        }

        fn merged(&self, branch: &str) -> PathBuf {
            let worktree = self.work(branch);
            self.fast_forward(branch);
            worktree
        }

        async fn clean_later(&self) -> Tally {
            clean(&self.checkout, DAY, SystemTime::now() + 2 * DAY).await
        }

        fn listed(&self, branch: &str) -> bool {
            run(&self.checkout, &["worktree", "list", "--porcelain"])
                .contains(&format!(".worktrees/{branch}"))
        }

        fn exclude(&self) -> String {
            std::fs::read_to_string(self.checkout.join(".git/info/exclude")).unwrap_or_default()
        }
    }

    #[tokio::test]
    async fn removes_a_clean_idle_worktree_whose_branch_was_squash_merged() {
        let fixture = Fixture::new();
        let worktree = fixture.work("feat/squashed");
        fixture.advance_main("unrelated.txt");
        fixture.squash("feat/squashed");

        let tally = fixture.clean_later().await;

        assert_eq!(
            tally,
            Tally {
                removed: 1,
                kept: 0,
                failed: 0
            }
        );
        assert!(!worktree.exists(), "the squash-merged worktree is gone");
        assert!(!fixture.listed("feat/squashed"), "git no longer records it");
        assert!(
            !fixture.checkout.join(".worktrees/feat").exists(),
            "the directory the branch name's prefix made goes with it"
        );
        assert_eq!(
            run(
                &fixture.checkout,
                &["rev-parse", "--verify", "refs/heads/feat/squashed"]
            )
            .len(),
            40,
            "the branch outlives its worktree"
        );
    }

    #[tokio::test]
    async fn removes_a_worktree_fast_forwarded_into_main_with_its_ignored_files() {
        let fixture = Fixture::new();
        let worktree = fixture.merged("fix/forwarded");
        std::fs::create_dir_all(worktree.join("target")).unwrap();
        std::fs::write(worktree.join("target/build.o"), "ignored").unwrap();

        let tally = fixture.clean_later().await;

        assert_eq!(tally.removed, 1, "{tally:?}");
        assert!(!worktree.exists(), "ignored build output does not keep it");
    }

    #[tokio::test]
    async fn keeps_a_merged_worktree_with_uncommitted_changes() {
        let fixture = Fixture::new();
        let worktree = fixture.merged("fix/edited");
        std::fs::write(worktree.join("README.md"), "changed\n").unwrap();

        let tally = fixture.clean_later().await;

        assert_eq!(tally.removed, 0, "{tally:?}");
        assert_eq!(tally.kept, 1, "{tally:?}");
        assert!(worktree.join("README.md").exists());
    }

    #[tokio::test]
    async fn keeps_a_merged_worktree_holding_an_untracked_file() {
        let fixture = Fixture::new();
        let worktree = fixture.merged("fix/notes");
        std::fs::write(worktree.join("notes.txt"), "draft\n").unwrap();

        let tally = fixture.clean_later().await;

        assert_eq!(tally.removed, 0, "{tally:?}");
        assert_eq!(tally.kept, 1, "{tally:?}");
        assert!(worktree.join("notes.txt").exists());
    }

    #[tokio::test]
    async fn keeps_a_worktree_whose_commits_are_not_on_main() {
        let fixture = Fixture::new();
        let worktree = fixture.work("feat/unmerged");
        fixture.advance_main("unrelated.txt");
        run(&fixture.checkout, &["fetch", "-q", "origin"]);

        let tally = fixture.clean_later().await;

        assert_eq!(tally.removed, 0, "{tally:?}");
        assert_eq!(tally.kept, 1, "{tally:?}");
        assert!(worktree.exists());
    }

    #[tokio::test]
    async fn keeps_a_merged_worktree_until_it_has_been_idle_long_enough() {
        let fixture = Fixture::new();
        let worktree = fixture.worktree("feat/fresh");

        let now = clean(&fixture.checkout, DAY, SystemTime::now()).await;

        assert_eq!(now.removed, 0, "{now:?}");
        assert!(
            worktree.exists(),
            "a worktree just added off main is trivially merged and must survive"
        );

        let later = fixture.clean_later().await;

        assert_eq!(later.removed, 1, "{later:?}");
        assert!(!worktree.exists(), "idleness was the only thing keeping it");
    }

    #[tokio::test]
    async fn keeps_a_locked_worktree() {
        let fixture = Fixture::new();
        let worktree = fixture.merged("fix/locked");
        run(
            &fixture.checkout,
            &["worktree", "lock", worktree.to_str().unwrap()],
        );

        let tally = fixture.clean_later().await;

        assert_eq!(tally.removed, 0, "{tally:?}");
        assert_eq!(tally.kept, 1, "{tally:?}");
        assert!(worktree.exists());
    }

    #[tokio::test]
    async fn ignores_worktrees_outside_the_agents_directory() {
        let fixture = Fixture::new();
        let ours = fixture.worktree("feat/ours");
        let other = fixture.checkout.join(".claude/worktrees/elsewhere");
        run(
            &fixture.checkout,
            &[
                "worktree",
                "add",
                "-q",
                other.to_str().unwrap(),
                "-b",
                "elsewhere",
                "origin/main",
            ],
        );

        let tally = fixture.clean_later().await;

        assert_eq!(
            tally,
            Tally {
                removed: 1,
                kept: 0,
                failed: 0
            },
            "only the worktree under .worktrees/ is judged at all"
        );
        assert!(!ours.exists());
        assert!(
            other.exists(),
            "another tool's worktree is not ours to remove"
        );
    }

    #[tokio::test]
    async fn adds_the_exclude_line_exactly_once() {
        let fixture = Fixture::new();
        let worktree = fixture.worktree("feat/excluded");

        clean(&fixture.checkout, DAY, SystemTime::now()).await;
        clean(&fixture.checkout, DAY, SystemTime::now()).await;

        let exclude = fixture.exclude();
        assert_eq!(
            exclude
                .lines()
                .filter(|line| *line == "/.worktrees/")
                .count(),
            1,
            "{exclude}"
        );
        assert!(worktree.exists(), "a fresh worktree is not idle yet");
        assert!(
            run(&fixture.checkout, &["status", "--porcelain"]).is_empty(),
            "the host's status lists no agent worktree"
        );
    }

    #[tokio::test]
    async fn keeps_a_worktree_whose_head_moved_after_it_was_judged_merged() {
        let fixture = Fixture::new();
        let worktree = fixture.merged("feat/moved");
        let repository = Repository::open(&fixture.checkout).await.unwrap();
        let entry = repository.worktrees().await.unwrap().remove(0);
        let target = repository.default_branch().await.unwrap();
        let verdict = repository
            .judge(&entry, target.as_deref(), DAY, SystemTime::now() + 2 * DAY)
            .await
            .unwrap();
        assert_eq!(verdict, Verdict::Removable);

        commit(&worktree, "later.txt", "after the verdict\n");

        assert!(repository.remove(&entry).await.is_err());
        assert!(fixture.listed("feat/moved"));
    }

    #[tokio::test]
    async fn leaves_a_folder_without_agent_worktrees_untouched() {
        let fixture = Fixture::new();

        let tally = fixture.clean_later().await;

        assert_eq!(tally, Tally::default());
        assert!(!fixture.exclude().contains(".worktrees"));
    }
}
