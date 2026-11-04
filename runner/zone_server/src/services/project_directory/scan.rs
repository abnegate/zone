use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

use uuid::Uuid;

use super::Origin;
use super::search::Search;
use crate::services::checkout;

/// How far below a mounted folder a checkout is looked for. A mount is often a whole home, and
/// this walk runs while a person waits for their turn to start.
const DEPTH: usize = 3;

/// Folders no checkout a person keeps lives in, each with enough beneath it to dominate the walk.
const SKIPPED: [&str; 3] = ["node_modules", "target", "vendor"];

const GIT: &str = ".git";
const CONFIG: &str = "config";

/// The checkout each workspace's turns last found of each repository.
static KNOWN: LazyLock<Mutex<HashMap<(Uuid, String), PathBuf>>> = LazyLock::new(Mutex::default);

/// A checkout of `origin` in `workspace`'s `mounted` folders: the one last found while it is
/// still there, in a folder still mounted, and still of that repository, else, when `search`
/// allows, the one a walk finds. A preview cannot afford the walk, and repeats what a turn found.
pub fn find(
    workspace: Uuid,
    mounted: &[PathBuf],
    clones: &Path,
    origin: &Origin,
    search: Search,
) -> Option<PathBuf> {
    let key = (workspace, origin.to_string());
    let mut known = KNOWN.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(checkout) = known.get(&key) {
        if mounted.iter().any(|root| checkout.starts_with(root)) && matches(checkout, origin) {
            return Some(checkout.clone());
        }
        known.remove(&key);
    }
    drop(known);
    if search == Search::Known {
        return None;
    }
    let found = checkout(mounted, clones, origin)?;
    KNOWN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(key, found.clone());
    Some(found)
}

/// The first checkout under `mounted`, folder by folder and shallowest first, whose `origin` is
/// `origin`, leaving out Zone's own `clones`: what is in there may be another workspace's, or a
/// clone still being made. Nothing is run inside a checkout and nothing in one is written: its
/// configuration is read as data, because it is the person's own and may name programs for git
/// to run.
fn checkout(mounted: &[PathBuf], clones: &Path, origin: &Origin) -> Option<PathBuf> {
    mounted.iter().find_map(|root| within(root, clones, origin))
}

/// A checkout's own folders aren't walked, with one exception: a mounted folder that is itself
/// another repository, as a home kept under git for its dotfiles is, holds the person's checkouts.
fn within(root: &Path, clones: &Path, origin: &Origin) -> Option<PathBuf> {
    let mut level = vec![root.to_path_buf()];
    for _ in 0..=DEPTH {
        let mut below = Vec::new();
        for directory in &level {
            if directory.starts_with(clones) {
                continue;
            }
            let git = fs::symlink_metadata(directory.join(GIT));
            if git.as_ref().is_ok_and(|metadata| metadata.is_dir()) && matches(directory, origin) {
                return Some(directory.clone());
            }
            if git.is_err() || directory == root {
                below.extend(children(directory));
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
    if !fs::symlink_metadata(&file).is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
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

    fn clones() -> &'static Path {
        Path::new("/nonexistent/clones")
    }

    fn found(mount: &TempDir) -> Option<PathBuf> {
        checkout(&[mount.path().to_path_buf()], clones(), &origin())
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
                clones(),
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

    /// A home folder kept under git for its dotfiles still holds the checkouts below it.
    #[test]
    fn a_mount_that_is_some_other_repository_is_still_walked() {
        let mount = mount();
        cloned(mount.path(), "git@github.com:owner/dotfiles.git");
        let expected = cloned(&mount.path().join("code/repository"), REMOTE);
        assert_eq!(found(&mount), Some(expected));
    }

    #[test]
    fn a_configuration_that_is_a_link_is_not_read() {
        let mount = mount();
        let elsewhere = TempDir::new().unwrap();
        let real = cloned(&elsewhere.path().join("real"), REMOTE);
        let planted = mount.path().join("planted");
        fs::create_dir_all(planted.join(GIT)).unwrap();
        std::os::unix::fs::symlink(real.join(GIT).join(CONFIG), planted.join(GIT).join(CONFIG))
            .unwrap();
        assert_eq!(found(&mount), None);
    }

    #[test]
    fn a_known_checkout_is_used_only_while_it_is_still_mounted_and_of_that_repository() {
        let (workspace, mounted, other) = (Uuid::new_v4(), mount(), mount());
        let path = cloned(&mounted.path().join("repository"), REMOTE);
        let folders = [mounted.path().to_path_buf()];
        let find =
            |folders: &[PathBuf], search| find(workspace, folders, clones(), &origin(), search);

        assert_eq!(find(&folders, Search::Known), None, "nothing is known yet");
        assert_eq!(find(&folders, Search::Walk), Some(path.clone()));
        assert_eq!(find(&folders, Search::Known), Some(path.clone()));
        assert_eq!(
            find(&[other.path().to_path_buf()], Search::Known),
            None,
            "a folder no longer mounted is not reached through what was known of it"
        );
        assert_eq!(find(&folders, Search::Walk), Some(path.clone()));

        let status = Command::new("git")
            .args(["remote", "set-url", "origin", "git@github.com:o/other.git"])
            .current_dir(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(find(&folders, Search::Known), None);
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
