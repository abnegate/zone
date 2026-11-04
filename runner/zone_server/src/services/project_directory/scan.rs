use std::fs;
use std::path::{Path, PathBuf};

use super::Origin;
use crate::services::checkout;

/// How far below a mounted folder a checkout is looked for. A mount is often a whole home, and
/// this walk runs while a person waits for their turn to start.
const DEPTH: usize = 3;

/// Folders no checkout a person keeps lives in, each with enough beneath it to dominate the walk.
const SKIPPED: [&str; 3] = ["node_modules", "target", "vendor"];

const GIT: &str = ".git";
const CONFIG: &str = "config";

/// The first checkout under `mounted`, folder by folder and shallowest first, whose `origin` is
/// `origin`. Nothing is run inside a checkout and nothing in one is written: its configuration
/// is read as data, because it is the person's own and may name programs for git to run.
pub fn checkout(mounted: &[PathBuf], origin: &Origin) -> Option<PathBuf> {
    mounted.iter().find_map(|root| within(root, origin))
}

fn within(root: &Path, origin: &Origin) -> Option<PathBuf> {
    let mut level = vec![root.to_path_buf()];
    for _ in 0..=DEPTH {
        let mut below = Vec::new();
        for directory in &level {
            match fs::symlink_metadata(directory.join(GIT)) {
                Ok(metadata) if metadata.is_dir() => {
                    if matches(directory, origin) {
                        return Some(directory.clone());
                    }
                }
                Ok(_) => {}
                Err(_) => below.extend(children(directory)),
            }
        }
        level = below;
    }
    None
}

/// `directory`'s own folders by name, without following a link out of the mount.
fn children(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && !SKIPPED.contains(&name.as_ref())
        })
        .map(|entry| entry.path())
        .collect();
    children.sort();
    children
}

/// Whether `checkout`'s `origin` is `origin`. A checkout whose configuration does not even hold
/// the repository's path is passed over without starting git for it, since a mount can hold
/// hundreds of checkouts.
fn matches(checkout: &Path, origin: &Origin) -> bool {
    let file = checkout.join(GIT).join(CONFIG);
    let Ok(configuration) = fs::read(&file) else {
        return false;
    };
    if !String::from_utf8_lossy(&configuration)
        .to_ascii_lowercase()
        .contains(origin.path())
    {
        return false;
    }
    checkout::configured_origin(&file)
        .ok()
        .flatten()
        .and_then(|remote| Origin::parse(&remote))
        .as_ref()
        == Some(origin)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    const REMOTE: &str = "git@github.com:owner/repository.git";

    fn origin() -> Origin {
        Origin::parse("https://github.com/owner/repository").expect("a repository")
    }

    fn cloned(path: &Path, remote: &str) -> PathBuf {
        fs::create_dir_all(path).unwrap();
        for arguments in [
            vec!["init", "--quiet"],
            vec!["remote", "add", "origin", remote],
        ] {
            let status = Command::new("git")
                .args(arguments)
                .current_dir(path)
                .status()
                .unwrap();
            assert!(status.success());
        }
        path.to_path_buf()
    }

    fn mount() -> TempDir {
        TempDir::new().unwrap()
    }

    fn found(mount: &TempDir) -> Option<PathBuf> {
        checkout(&[mount.path().to_path_buf()], &origin())
    }

    #[test]
    fn a_checkout_is_found_by_its_origin_in_any_spelling() {
        for remote in [
            "https://github.com/owner/repository.git",
            "git@github.com:Owner/Repository.git",
        ] {
            let mount = mount();
            let expected = cloned(&mount.path().join("code/nested/renamed"), remote);
            assert_eq!(found(&mount), Some(expected), "{remote}");
        }
    }

    #[test]
    fn a_folder_named_for_the_repository_is_not_its_checkout() {
        let mount = mount();
        cloned(
            &mount.path().join("repository"),
            "git@github.com:someone-else/repository.git",
        );
        fs::create_dir_all(mount.path().join("owner/repository")).unwrap();
        assert_eq!(found(&mount), None);
    }

    #[test]
    fn another_remote_of_a_checkout_does_not_make_it_a_checkout_of_that_repository() {
        let mount = mount();
        let fork = cloned(
            &mount.path().join("fork"),
            "git@github.com:someone-else/repository.git",
        );
        let status = Command::new("git")
            .args(["remote", "add", "upstream", REMOTE])
            .current_dir(&fork)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(found(&mount), None);
    }

    #[test]
    fn the_first_mount_and_the_shallowest_checkout_win() {
        let (first, second) = (mount(), mount());
        let expected = cloned(&first.path().join("z"), REMOTE);
        cloned(&first.path().join("a/deeper"), REMOTE);
        cloned(&second.path().join("a"), REMOTE);
        assert_eq!(
            checkout(
                &[first.path().to_path_buf(), second.path().to_path_buf()],
                &origin()
            ),
            Some(expected)
        );
    }

    #[test]
    fn hidden_folders_links_and_a_checkouts_own_contents_are_not_walked() {
        let mount = mount();
        cloned(&mount.path().join(".cache/repository"), REMOTE);
        cloned(&mount.path().join("node_modules/repository"), REMOTE);
        let elsewhere = TempDir::new().unwrap();
        cloned(&elsewhere.path().join("repository"), REMOTE);
        std::os::unix::fs::symlink(elsewhere.path(), mount.path().join("link")).unwrap();
        cloned(
            &mount.path().join("other"),
            "git@github.com:someone-else/other.git",
        );
        cloned(&mount.path().join("other/vendored"), REMOTE);
        assert_eq!(found(&mount), None);
    }

    #[test]
    fn nothing_is_found_below_the_depth_a_turn_waits_for() {
        let mount = mount();
        cloned(&mount.path().join("a/b/c/d"), REMOTE);
        assert_eq!(found(&mount), None);
    }

    #[test]
    fn a_mount_that_is_itself_the_checkout_is_found() {
        let mount = mount();
        let expected = cloned(mount.path(), REMOTE);
        assert_eq!(found(&mount), Some(expected));
    }

    #[test]
    fn a_linked_worktree_is_left_alone() {
        let mount = mount();
        let worktree = mount.path().join("worktree");
        fs::create_dir_all(&worktree).unwrap();
        fs::write(worktree.join(GIT), "gitdir: /elsewhere\n").unwrap();
        cloned(&worktree.join("inside"), REMOTE);
        assert_eq!(found(&mount), None);
    }
}
