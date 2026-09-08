//! Reproducing a real merge conflict against a real repository.
//!
//! Every repository here is built from scratch inside a temporary directory, so
//! nothing in these tests can reach the checkout the suite is running from.

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;
use zone_vcs::conflict::{
    BranchName, CommitSha, ConflictError, ConflictRequest, ConflictService, ConflictedPath,
    has_markers,
};

const OURS: &str = "fn value() -> u32 {\n    1\n}\n";
const THEIRS: &str = "fn value() -> u32 {\n    2\n}\n";
const UNRELATED: &str = "# notes\n";

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

fn write(repository: &Path, path: &str, content: &str) {
    let target = repository.join(path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(target, content).unwrap();
}

fn commit(repository: &Path, message: &str) {
    git(repository, &["add", "--all"]);
    git(repository, &["commit", "--quiet", "-m", message]);
}

/// A repository whose `feature` branch conflicts with `main` in `src/value.rs`.
fn conflicting_origin() -> TempDir {
    let origin = TempDir::new().unwrap();
    let path = origin.path();

    git(path, &["init", "--quiet", "--initial-branch", "main"]);
    write(path, "src/value.rs", "fn value() -> u32 {\n    0\n}\n");
    write(path, "README.md", "# project\n");
    commit(path, "initial");

    git(path, &["checkout", "--quiet", "-b", "feature"]);
    write(path, "src/value.rs", THEIRS);
    commit(path, "feature changes the value");

    git(path, &["checkout", "--quiet", "main"]);
    write(path, "src/value.rs", OURS);
    commit(path, "main changes the value too");

    origin
}

/// A repository whose `feature` branch merges cleanly into `main`.
fn clean_origin() -> TempDir {
    let origin = TempDir::new().unwrap();
    let path = origin.path();

    git(path, &["init", "--quiet", "--initial-branch", "main"]);
    write(path, "src/value.rs", "fn value() -> u32 {\n    0\n}\n");
    commit(path, "initial");

    git(path, &["checkout", "--quiet", "-b", "feature"]);
    write(path, "NOTES.md", UNRELATED);
    commit(path, "feature adds notes");

    git(path, &["checkout", "--quiet", "main"]);
    origin
}

fn request(origin: &Path) -> ConflictRequest {
    ConflictRequest {
        remote: origin.to_string_lossy().to_string(),
        token: None,
        head: BranchName::parse("feature").unwrap(),
        base: BranchName::parse("main").unwrap(),
        expected_head: None,
        expected_base: None,
    }
}

#[tokio::test]
async fn a_conflicting_branch_reproduces_in_a_throwaway_checkout() {
    let origin = conflicting_origin();
    let conflict = ConflictService::new()
        .reproduce(&request(origin.path()))
        .await
        .expect("a branch that conflicts must reproduce");

    assert_eq!(
        conflict.files(),
        &[ConflictedPath::parse("src/value.rs").unwrap()],
        "only the file git reported unmerged belongs to the conflict"
    );

    let checkout = conflict.path().to_path_buf();
    assert!(checkout.is_dir());
    assert_ne!(
        checkout.canonicalize().unwrap(),
        origin.path().canonicalize().unwrap(),
        "the repair must never run in the repository it fetched from"
    );

    let conflicted = std::fs::read_to_string(checkout.join("src/value.rs")).unwrap();
    assert!(
        has_markers(&conflicted),
        "the reproduced checkout must actually hold the conflicted text"
    );
    assert!(conflicted.contains("    1") && conflicted.contains("    2"));

    drop(conflict);
    assert!(
        !checkout.exists(),
        "the throwaway checkout must be gone once the conflict is dropped"
    );
}

#[tokio::test]
async fn a_branch_that_merges_cleanly_is_nothing_to_repair() {
    let origin = clean_origin();
    let outcome = ConflictService::new()
        .reproduce(&request(origin.path()))
        .await;

    assert!(
        matches!(outcome, Err(ConflictError::NoConflict)),
        "a clean merge must report nothing to do rather than invent a repair"
    );
}

#[tokio::test]
async fn a_head_that_moved_since_the_caller_looked_refuses_to_reproduce() {
    let origin = conflicting_origin();
    let mut request = request(origin.path());
    request.expected_head =
        Some(CommitSha::parse("0123456789abcdef0123456789abcdef01234567").unwrap());

    let outcome = ConflictService::new().reproduce(&request).await;
    match outcome {
        Err(ConflictError::Moved { branch, .. }) => assert_eq!(branch, "feature"),
        other => panic!("a moved head must refuse the repair, got {other:?}"),
    }
}

#[tokio::test]
async fn the_expected_commits_let_a_caller_pin_the_state_it_reproduced() {
    let origin = conflicting_origin();
    let head = CommitSha::parse(&git(origin.path(), &["rev-parse", "feature"])).unwrap();
    let base = CommitSha::parse(&git(origin.path(), &["rev-parse", "main"])).unwrap();

    let mut request = request(origin.path());
    request.expected_head = Some(head.clone());
    request.expected_base = Some(base.clone());

    let conflict = ConflictService::new().reproduce(&request).await.unwrap();
    assert_eq!(conflict.head(), &head);
    assert_eq!(conflict.base(), &base);
    conflict
        .verify(&ConflictService::new())
        .await
        .expect("a freshly reproduced checkout is at the state it reports");
}

#[tokio::test]
async fn a_checkout_moved_underneath_the_repair_is_refused() {
    let origin = conflicting_origin();
    let service = ConflictService::new();
    let conflict = service.reproduce(&request(origin.path())).await.unwrap();

    git(conflict.path(), &["reset", "--hard", "--quiet"]);
    git(
        conflict.path(),
        &["checkout", "--detach", "--quiet", "refs/zone/conflict/base"],
    );

    assert!(
        matches!(
            conflict.verify(&service).await,
            Err(ConflictError::CheckoutMoved)
        ),
        "a repair applied to a checkout that moved is not a repair of this conflict"
    );
}

#[tokio::test]
async fn only_the_conflicted_files_can_be_reached_from_the_checkout() {
    let origin = conflicting_origin();
    let conflict = ConflictService::new()
        .reproduce(&request(origin.path()))
        .await
        .unwrap();

    let allowed = conflict
        .confine("src/value.rs")
        .expect("a conflicted file is in scope");
    assert_eq!(
        allowed,
        conflict.path().canonicalize().unwrap().join("src/value.rs")
    );

    assert_eq!(
        conflict
            .confine(&allowed.to_string_lossy())
            .expect("the absolute form of a conflicted file is the same file"),
        allowed
    );

    for refused in [
        "README.md",
        "src/../README.md",
        "../outside.rs",
        "/etc/passwd",
        ".git/config",
        "",
    ] {
        assert!(
            conflict.confine(refused).is_err(),
            "{refused:?} is not one of the conflicted files and must be refused"
        );
    }

    let escape = origin.path().join("README.md");
    assert!(
        conflict.confine(&escape.to_string_lossy()).is_err(),
        "an absolute path outside the checkout must be refused"
    );
}
