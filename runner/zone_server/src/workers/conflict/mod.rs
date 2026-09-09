//! Item 10: repairing a pull request whose branch no longer merges.
//!
//! Opening a pull request and walking away leaves a branch that conflicts with
//! its base stuck until a person notices. This reproduces the conflict in a
//! throwaway checkout, hands a model the conflicted files and nothing else, then
//! judges what came back before any of it reaches the remote.
//!
//! Two outcomes would be worse than leaving the conflict alone, and each has its
//! own bound:
//!
//! - **A repair that discards one side.** Every conflicted file is read before the
//!   agent touches it and judged against that original afterwards
//!   ([`zone_vcs::resolution::judge`]). A resolution carrying no distinctive line
//!   from one of the two branches is rejected and nothing is pushed.
//! - **A repair applied to the wrong tree.** The checkout is built from scratch
//!   from the two commits the caller named, both are checked against what the
//!   caller expected, the merge has to actually fail before an agent is started,
//!   and the checkout's `HEAD` and fetched refs are checked again after the repair.
//!   The push itself is never forced, so a branch that moved meanwhile rejects it.

pub mod agent;
pub mod environment;
pub mod prompt;
pub mod scope;

use std::sync::Arc;

use zone_vcs::conflict::{
    BranchName, CommitSha, Conflict, ConflictError, ConflictRequest, ConflictService,
    ConflictedPath, validate,
};
use zone_vcs::resolution::{ResolutionVerdict, judge};

use agent::{RepairAgent, RepairTask};
use prompt::RepairBrief;
use scope::RepairScope;

/// What to repair, and what the caller believes it is repairing.
#[derive(Debug, Clone)]
pub struct RepairRequest {
    pub remote: String,
    pub token: Option<String>,
    pub head: BranchName,
    pub base: BranchName,
    pub expected_head: Option<CommitSha>,
    pub expected_base: Option<CommitSha>,
    pub pull_request: Option<String>,
}

/// How a repair ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairOutcome {
    /// The conflict was resolved and the merge pushed to the head branch.
    Repaired {
        files: Vec<ConflictedPath>,
        commit: CommitSha,
    },
    /// The branch merges cleanly; there was nothing to do.
    NotConflicted,
    /// A resolution was produced but is not one this will publish.
    Rejected {
        path: ConflictedPath,
        verdict: ResolutionVerdict,
    },
    /// The repair changed files it was not given.
    Strayed(Vec<String>),
    /// The repair could not be completed.
    Failed(String),
}

impl RepairOutcome {
    pub fn repaired(&self) -> bool {
        matches!(self, RepairOutcome::Repaired { .. })
    }
}

/// The message the repaired merge is committed with.
fn message(request: &RepairRequest) -> String {
    match &request.pull_request {
        Some(url) => format!(
            "(fix): resolve merge conflicts with {}\n\nPull request: {url}\n",
            request.base
        ),
        None => format!("(fix): resolve merge conflicts with {}\n", request.base),
    }
}

/// Reproduce a pull request's conflict, repair it, and publish only if the
/// resolution kept both branches' work.
pub async fn repair(
    service: &ConflictService,
    repairer: &dyn RepairAgent,
    request: &RepairRequest,
) -> RepairOutcome {
    let conflict = match service
        .reproduce(&ConflictRequest {
            remote: request.remote.clone(),
            token: request.token.clone(),
            head: request.head.clone(),
            base: request.base.clone(),
            expected_head: request.expected_head.clone(),
            expected_base: request.expected_base.clone(),
        })
        .await
    {
        Ok(conflict) => conflict,
        Err(ConflictError::NoConflict) => return RepairOutcome::NotConflicted,
        Err(error) => return RepairOutcome::Failed(error.to_string()),
    };

    if let Err(error) = validate(conflict.path(), conflict.files()) {
        return RepairOutcome::Failed(error.to_string());
    }

    let originals = match read_all(&conflict) {
        Ok(originals) => originals,
        Err(error) => return RepairOutcome::Failed(error),
    };

    let scope = Arc::new(RepairScope::new(&conflict));
    let task = RepairTask {
        conflict: &conflict,
        scope,
        context: agent::context(&conflict, environment::from_process(conflict.isolation())),
        system: prompt::system(),
        prompt: prompt::build(&RepairBrief {
            pull_request: request.pull_request.as_deref(),
            head: conflict.head(),
            base: conflict.base(),
            files: conflict.files(),
        }),
    };

    match tokio::time::timeout(agent::REPAIR_TIMEOUT, repairer.repair(&task)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return RepairOutcome::Failed(error),
        Err(_) => return RepairOutcome::Failed("the repair ran out of time".to_string()),
    }

    if let Err(error) = conflict.verify(service).await {
        return RepairOutcome::Failed(error.to_string());
    }

    let resolved = match read_all(&conflict) {
        Ok(resolved) => resolved,
        Err(error) => return RepairOutcome::Failed(error),
    };

    for ((path, original), after) in conflict.files().iter().zip(&originals).zip(&resolved) {
        let verdict = judge(original, after);
        if !verdict.accepted() {
            return RepairOutcome::Rejected {
                path: path.clone(),
                verdict,
            };
        }
    }

    match service.strays(&conflict).await {
        Ok(strays) if !strays.is_empty() => return RepairOutcome::Strayed(strays),
        Ok(_) => {}
        Err(error) => return RepairOutcome::Failed(error.to_string()),
    }

    let commit = match service.apply(&conflict, &message(request)).await {
        Ok(commit) => commit,
        Err(error) => return RepairOutcome::Failed(error.to_string()),
    };

    if let Err(error) = service
        .publish(
            &conflict,
            &request.remote,
            request.token.as_deref(),
            &request.head,
        )
        .await
    {
        return RepairOutcome::Failed(error.to_string());
    }

    RepairOutcome::Repaired {
        files: conflict.files().to_vec(),
        commit,
    }
}

/// Read every conflicted file, re-validating each path first.
///
/// Validating again after the agent has run is the point: a repair that replaced
/// a conflicted file with a symlink would otherwise be read straight through it.
fn read_all(conflict: &Conflict) -> Result<Vec<String>, String> {
    let resolved =
        validate(conflict.path(), conflict.files()).map_err(|error| error.to_string())?;

    resolved
        .iter()
        .map(|path| std::fs::read_to_string(path).map_err(|error| error.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;
    use zone_vcs::resolution::ConflictSide;

    /// The checkout is detached at the head branch and merges the base into it,
    /// so the head branch is git's "ours" and the base branch is git's "theirs".
    const HEAD_SIDE: &str = "fn value() -> u32 {\n    let feature = 2;\n    feature\n}\n";
    const BASE_SIDE: &str = "fn value() -> u32 {\n    let mainline = 1;\n    mainline\n}\n";
    const MERGED: &str = "fn value() -> u32 {\n    let feature = 2;\n    let mainline = 1;\n    feature + mainline\n}\n";

    /// Writes the same resolution into every conflicted file.
    struct Writer(&'static str);

    #[async_trait]
    impl RepairAgent for Writer {
        async fn repair(&self, task: &RepairTask<'_>) -> Result<(), String> {
            for path in task.conflict.files() {
                let target = task
                    .conflict
                    .confine(path.as_str())
                    .map_err(|error| error.to_string())?;
                std::fs::write(target, self.0).map_err(|error| error.to_string())?;
            }
            Ok(())
        }
    }

    /// Resolves properly, then rewrites a file it was never given.
    struct Wanderer;

    #[async_trait]
    impl RepairAgent for Wanderer {
        async fn repair(&self, task: &RepairTask<'_>) -> Result<(), String> {
            Writer(MERGED).repair(task).await?;
            std::fs::write(task.conflict.path().join("README.md"), "rewritten")
                .map_err(|error| error.to_string())
        }
    }

    /// Leaves the conflict exactly as it found it.
    struct Idle;

    #[async_trait]
    impl RepairAgent for Idle {
        async fn repair(&self, _task: &RepairTask<'_>) -> Result<(), String> {
            Ok(())
        }
    }

    fn git(repository: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(repository)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Zone")
            .env("GIT_AUTHOR_EMAIL", "zone@example.test")
            .env("GIT_COMMITTER_NAME", "Zone")
            .env("GIT_COMMITTER_EMAIL", "zone@example.test")
            .args(arguments)
            .output()
            .expect("git must be available to run these tests");

        assert!(
            output.status.success(),
            "git {arguments:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn commit(repository: &Path, path: &str, content: &str, message: &str) {
        let target = repository.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, content).unwrap();
        git(repository, &["add", "--all"]);
        git(repository, &["commit", "--quiet", "-m", message]);
    }

    /// An origin whose `feature` branch conflicts with `main` in `src/value.rs`.
    fn origin(conflicting: bool) -> TempDir {
        let origin = TempDir::new().unwrap();
        let path = origin.path();

        git(path, &["init", "--quiet", "--initial-branch", "main"]);
        std::fs::write(path.join("README.md"), "# project\n").unwrap();
        commit(
            path,
            "src/value.rs",
            "fn value() -> u32 {\n    0\n}\n",
            "initial",
        );

        git(path, &["checkout", "--quiet", "-b", "feature"]);
        if conflicting {
            commit(path, "src/value.rs", HEAD_SIDE, "feature changes the value");
        } else {
            commit(path, "NOTES.md", "# notes\n", "feature adds notes");
        }

        git(path, &["checkout", "--quiet", "main"]);
        commit(
            path,
            "src/value.rs",
            BASE_SIDE,
            "main changes the value too",
        );

        origin
    }

    fn request(remote: &Path) -> RepairRequest {
        RepairRequest {
            remote: remote.to_string_lossy().to_string(),
            token: None,
            head: BranchName::parse("feature").unwrap(),
            base: BranchName::parse("main").unwrap(),
            expected_head: None,
            expected_base: None,
            pull_request: Some("https://github.com/acme/project/pull/7".to_string()),
        }
    }

    fn feature_head(origin: &Path) -> String {
        git(origin, &["rev-parse", "feature"])
    }

    fn feature_value(origin: &Path) -> String {
        git(origin, &["show", "feature:src/value.rs"])
    }

    #[tokio::test]
    async fn a_branch_that_merges_cleanly_is_a_no_op() {
        let origin = origin(false);
        let before = feature_head(origin.path());

        let outcome = repair(
            &ConflictService::new(),
            &Writer(MERGED),
            &request(origin.path()),
        )
        .await;

        assert_eq!(outcome, RepairOutcome::NotConflicted);
        assert_eq!(
            feature_head(origin.path()),
            before,
            "a branch with nothing to repair must come out untouched"
        );
    }

    #[tokio::test]
    async fn a_resolution_keeping_both_sides_is_published() {
        let origin = origin(true);
        let before = feature_head(origin.path());

        let outcome = repair(
            &ConflictService::new(),
            &Writer(MERGED),
            &request(origin.path()),
        )
        .await;

        match &outcome {
            RepairOutcome::Repaired { files, commit } => {
                assert_eq!(files, &[ConflictedPath::parse("src/value.rs").unwrap()]);
                assert_eq!(commit.as_str(), feature_head(origin.path()));
            }
            other => panic!("a resolvable conflict must be repaired, got {other:?}"),
        }

        assert_ne!(feature_head(origin.path()), before);
        let published = feature_value(origin.path());
        assert!(published.contains("let feature = 2;"));
        assert!(published.contains("let mainline = 1;"));
    }

    #[tokio::test]
    async fn a_resolution_that_discards_one_side_is_rejected_and_never_pushed() {
        for (resolution, discarded) in [
            (BASE_SIDE, ConflictSide::Ours),
            (HEAD_SIDE, ConflictSide::Theirs),
        ] {
            let origin = origin(true);
            let before = feature_head(origin.path());

            let outcome = repair(
                &ConflictService::new(),
                &Writer(resolution),
                &request(origin.path()),
            )
            .await;

            match &outcome {
                RepairOutcome::Rejected { path, verdict } => {
                    assert_eq!(path.as_str(), "src/value.rs");
                    assert_eq!(*verdict, ResolutionVerdict::Discarded(discarded));
                }
                other => panic!("throwing a branch's work away is not a repair, got {other:?}"),
            }

            assert_eq!(
                feature_head(origin.path()),
                before,
                "a rejected resolution must leave the remote exactly as it was"
            );
            assert_eq!(feature_value(origin.path()), HEAD_SIDE.trim_end());
        }
    }

    #[tokio::test]
    async fn a_resolution_that_leaves_conflict_markers_is_rejected() {
        let origin = origin(true);
        let before = feature_head(origin.path());

        let outcome = repair(&ConflictService::new(), &Idle, &request(origin.path())).await;

        match &outcome {
            RepairOutcome::Rejected { verdict, .. } => {
                assert_eq!(*verdict, ResolutionVerdict::MarkersRemain);
            }
            other => panic!("an untouched conflict is not a repair, got {other:?}"),
        }
        assert_eq!(feature_head(origin.path()), before);
    }

    #[tokio::test]
    async fn a_repair_that_touches_a_file_it_was_not_given_is_refused() {
        let origin = origin(true);
        let before = feature_head(origin.path());

        let outcome = repair(&ConflictService::new(), &Wanderer, &request(origin.path())).await;

        assert_eq!(
            outcome,
            RepairOutcome::Strayed(vec!["README.md".to_string()]),
            "a resolution is only publishable if it stayed inside the conflict"
        );
        assert_eq!(feature_head(origin.path()), before);
    }

    #[tokio::test]
    async fn a_head_that_moved_since_the_caller_looked_is_not_repaired() {
        let origin = origin(true);
        let mut request = request(origin.path());
        request.expected_head =
            Some(CommitSha::parse("0123456789abcdef0123456789abcdef01234567").unwrap());

        let outcome = repair(&ConflictService::new(), &Writer(MERGED), &request).await;

        match outcome {
            RepairOutcome::Failed(reason) => assert!(
                reason.contains("feature is at"),
                "the refusal must name what moved, got {reason}"
            ),
            other => panic!("a moved head must not be repaired, got {other:?}"),
        }
    }

    #[test]
    fn the_commit_message_names_the_base_and_the_pull_request() {
        let message = message(&request(Path::new("/tmp/origin")));
        assert!(message.starts_with("(fix): resolve merge conflicts with main"));
        assert!(message.contains("https://github.com/acme/project/pull/7"));
    }

    #[test]
    fn an_outcome_is_only_a_repair_when_something_was_published() {
        assert!(!RepairOutcome::NotConflicted.repaired());
        assert!(!RepairOutcome::Strayed(vec!["README.md".to_string()]).repaired());
        assert!(
            !RepairOutcome::Rejected {
                path: ConflictedPath::parse("src/value.rs").unwrap(),
                verdict: ResolutionVerdict::MarkersRemain,
            }
            .repaired()
        );
        assert!(
            RepairOutcome::Repaired {
                files: Vec::new(),
                commit: CommitSha::parse("0123456789abcdef0123456789abcdef01234567").unwrap(),
            }
            .repaired()
        );
    }
}
