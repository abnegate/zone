use std::path::{Path, PathBuf};

use abnegate_secret::SecretValue;
use abnegate_vcs::git::GitService;
use abnegate_vcs::{BranchName, Checkout, CommitSha, RepositoryUrl};
use tokio::process::Command;
use uuid::Uuid;

use crate::services::project_directory::Origin;

/// Publishes `head` from the checkout a chat committed it in to `repository`
/// with the source's own `token`, so the agent never holds a credential and
/// the checkout's own remote, SSH or otherwise, is never used. Answers `None`
/// when the checkout has no such branch, which leaves GitHub to find it.
///
/// The checkout is often the user's own, whose configuration Zone's hardened
/// git refuses to run in. So the branch is only fetched out of it, into a
/// scratch repository Zone made, and pushed from there.
pub async fn push(
    git: &GitService,
    directory: &Path,
    repository: &RepositoryUrl,
    origin: &Origin,
    head: &str,
    token: &SecretValue,
) -> Result<Option<CommitSha>, String> {
    let branch = BranchName::parse(head).map_err(|_| format!("{head} is not a branch name."))?;
    if !directory.is_absolute() {
        return Err(format!("{} is not an absolute path.", directory.display()));
    }
    let remote = plain(
        Some(directory),
        &["config", "--get", "remote.origin.url"],
        directory,
    )
    .await
    .map_err(|_| format!("{} is not a checkout with an origin.", directory.display()))?;
    if Origin::parse(&remote).as_ref() != Some(origin) {
        return Err(format!(
            "{} is a checkout of {remote}, not of {origin}.",
            directory.display()
        ));
    }
    let scratch = Scratch::create()?;
    let source = directory.to_string_lossy().to_string();
    let reference = branch.reference();
    plain(Some(&scratch.0), &["init", "-q"], directory).await?;
    let listed = plain(
        Some(&scratch.0),
        &[
            "-c",
            FILE_PROTOCOL,
            "ls-remote",
            "--heads",
            "--",
            &source,
            &reference,
        ],
        directory,
    )
    .await
    .map_err(|error| format!("Could not read {}: {error}", directory.display()))?;
    if listed.is_empty() {
        return Ok(None);
    }
    plain(
        Some(&scratch.0),
        &[
            "-c",
            FILE_PROTOCOL,
            "fetch",
            "-q",
            "--no-tags",
            "--no-write-fetch-head",
            "--no-recurse-submodules",
            "--update-shallow",
            "--update-head-ok",
            "--",
            &source,
            &format!("+{reference}:{reference}"),
        ],
        directory,
    )
    .await
    .map_err(|error| {
        format!(
            "Could not fetch {head} from {}: {error}",
            directory.display()
        )
    })?;
    plain(
        Some(&scratch.0),
        &["rev-parse", "--verify", "--quiet", &reference],
        directory,
    )
    .await
    .map_err(|_| {
        format!(
            "{head} could not be copied out of {}, which is a shallow checkout; deepen it with git \
             fetch --unshallow and call again.",
            directory.display()
        )
    })?;
    plain(
        Some(&scratch.0),
        &["symbolic-ref", "HEAD", &reference],
        directory,
    )
    .await?;
    git.push_with_token(&Checkout::base(&scratch.0), &branch, repository, token)
        .await
        .map(Some)
        .map_err(|error| error.to_string())
}

const FILE_PROTOCOL: &str = "protocol.file.allow=always";

/// Runs git outside Zone's hardened service, with no system or global
/// configuration, trusting only `checkout`, which is commonly owned by the
/// host's user rather than Zone's.
async fn plain(
    within: Option<&Path>,
    arguments: &[&str],
    checkout: &Path,
) -> Result<String, String> {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg(format!("safe.directory={}", checkout.display()))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    if let Some(within) = within {
        command.arg("-C").arg(within);
    }
    let output = command
        .args(arguments)
        .output()
        .await
        .map_err(|error| format!("Could not run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// A directory only this process's user can open, removed when dropped: what
/// is copied into it is a branch nobody has published yet.
struct Scratch(PathBuf);

impl Scratch {
    fn create() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("zone-push-{}", Uuid::new_v4().simple()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
            .create(&path)
            .map_err(|error| format!("Could not make a scratch repository: {error}"))?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn run(directory: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(directory)
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

    struct Fixture {
        _root: tempfile::TempDir,
        checkout: PathBuf,
        bare: PathBuf,
    }

    fn fixture(remote: &str, branch: &str) -> Fixture {
        let root = tempfile::tempdir().unwrap();
        let bare = root.path().join("remote.git");
        let checkout = root.path().join("checkout");
        std::fs::create_dir_all(&checkout).unwrap();
        run(
            root.path(),
            &["init", "--bare", "-q", bare.to_str().unwrap()],
        );
        run(&checkout, &["init", "-q", "-b", "main"]);
        run(&checkout, &["remote", "add", "origin", remote]);
        std::fs::write(checkout.join("file.txt"), "work").unwrap();
        run(&checkout, &["add", "."]);
        run(&checkout, &["commit", "-q", "-m", "work"]);
        run(&checkout, &["checkout", "-q", "-b", branch]);
        Fixture {
            _root: root,
            checkout,
            bare,
        }
    }

    fn origin() -> Origin {
        Origin::parse("https://github.com/owner/repository").unwrap()
    }

    async fn pushed(fixture: &Fixture, head: &str) -> Result<Option<CommitSha>, String> {
        push(
            &GitService::new(),
            &fixture.checkout,
            &RepositoryUrl::local(&fixture.bare).unwrap(),
            &origin(),
            head,
            &SecretValue::new("token"),
        )
        .await
    }

    #[tokio::test]
    async fn pushes_a_branch_of_an_ssh_checkout_with_the_token() {
        let fixture = fixture("git@github.com:owner/repository.git", "feat/settings-tab");
        let commit = pushed(&fixture, "feat/settings-tab")
            .await
            .expect("the branch is pushed")
            .expect("the checkout has the branch");
        assert_eq!(
            run(
                &fixture.bare,
                &["rev-parse", "refs/heads/feat/settings-tab"]
            ),
            commit.as_str(),
            "the remote holds the branch's tip"
        );
    }

    #[tokio::test]
    async fn pushes_from_a_checkout_whose_configuration_hardened_git_refuses() {
        let fixture = fixture("git@github.com:owner/repository.git", "feature");
        run(
            &fixture.checkout,
            &["config", "core.untrackedCache", "true"],
        );
        let commit = pushed(&fixture, "feature")
            .await
            .expect("a user's own configuration does not stop the push")
            .expect("the checkout has the branch");
        assert_eq!(
            run(&fixture.bare, &["rev-parse", "refs/heads/feature"]),
            commit.as_str()
        );
    }

    #[tokio::test]
    async fn pushes_a_branch_that_is_not_checked_out() {
        let fixture = fixture("https://github.com/owner/repository.git", "feature");
        run(&fixture.checkout, &["checkout", "-q", "main"]);
        let commit = pushed(&fixture, "feature")
            .await
            .expect("the branch is pushed")
            .expect("the checkout has the branch");
        assert_eq!(
            run(&fixture.checkout, &["rev-parse", "feature"]),
            commit.as_str()
        );
    }

    #[tokio::test]
    async fn refuses_a_directory_that_could_be_read_as_an_option() {
        let refused = push(
            &GitService::new(),
            Path::new("--upload-pack=touch pwned"),
            &RepositoryUrl::local(Path::new("/nonexistent")).unwrap(),
            &origin(),
            "feature",
            &SecretValue::new("token"),
        )
        .await
        .expect_err("a relative directory is refused before git runs");
        assert!(refused.contains("not an absolute path"), "{refused}");
    }

    #[tokio::test]
    async fn refuses_a_checkout_of_another_repository() {
        let fixture = fixture("git@github.com:someone/else.git", "feature");
        let refused = pushed(&fixture, "feature")
            .await
            .expect_err("another repository's checkout is not pushed");
        assert!(
            refused.contains("not of github.com/owner/repository"),
            "{refused}"
        );
    }

    #[tokio::test]
    async fn pushes_the_branch_a_fresh_repository_starts_on() {
        let fixture = fixture("https://github.com/owner/repository.git", "master");
        let commit = pushed(&fixture, "master")
            .await
            .expect("the scratch repository's own unborn branch is fetched into")
            .expect("the checkout has the branch");
        assert_eq!(
            run(&fixture.bare, &["rev-parse", "refs/heads/master"]),
            commit.as_str()
        );
    }

    #[tokio::test]
    async fn leaves_a_branch_the_checkout_does_not_have_to_github() {
        let fixture = fixture("https://github.com/owner/repository.git", "feature");
        let pushed = pushed(&fixture, "missing")
            .await
            .expect("a branch only GitHub has is not an error");
        assert!(pushed.is_none(), "nothing is pushed for it");
    }

    #[tokio::test]
    async fn pushes_a_branch_of_a_shallow_checkout() {
        let fixture = fixture("https://github.com/owner/repository.git", "base");
        for round in 0..3 {
            std::fs::write(fixture.checkout.join("file.txt"), round.to_string()).unwrap();
            run(&fixture.checkout, &["commit", "-q", "-am", "more"]);
        }
        run(
            &fixture.checkout,
            &["push", "-q", fixture.bare.to_str().unwrap(), "base"],
        );
        let shallow = fixture._root.path().join("shallow");
        run(
            fixture._root.path(),
            &[
                "clone",
                "-q",
                "--depth",
                "1",
                &format!("file://{}", fixture.checkout.display()),
                shallow.to_str().unwrap(),
            ],
        );
        run(
            &shallow,
            &[
                "remote",
                "set-url",
                "origin",
                "git@github.com:owner/repository.git",
            ],
        );
        run(&shallow, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(shallow.join("new.txt"), "new").unwrap();
        run(&shallow, &["add", "."]);
        run(&shallow, &["commit", "-q", "-m", "new"]);
        let commit = push(
            &GitService::new(),
            &shallow,
            &RepositoryUrl::local(&fixture.bare).unwrap(),
            &origin(),
            "feature",
            &SecretValue::new("token"),
        )
        .await
        .expect("a shallow checkout's branch is copied out")
        .expect("the checkout has the branch");
        assert_eq!(run(&shallow, &["rev-parse", "feature"]), commit.as_str());
    }

    #[cfg(unix)]
    #[test]
    fn a_scratch_repository_is_private_to_its_user() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::create().expect("a scratch directory");
        let mode = std::fs::metadata(&scratch.0).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
