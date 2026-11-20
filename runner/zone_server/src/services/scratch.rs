//! Where a task run's tools put what they make and do not keep: temporary
//! files and build output.
//!
//! Each run gets a private directory of its own, and every command the run
//! starts finds `TMPDIR` naming it and `CARGO_TARGET_DIR` inside it. It is
//! not inside the checkout, which is the run's git worktree: what lands there
//! would show as work the run made, and a worktree holding unpublished work
//! outlives its run. The scratch never does: it goes when the run ends, and
//! recovery takes what a crash left.
//!
//! The directory sits directly in the host's temporary directory under a
//! short name, `zs-` and twelve hex digits, because programs make Unix
//! sockets under `TMPDIR` (`ssh-agent`, Python's `multiprocessing`, test
//! servers) and a socket path holds barely a hundred bytes. Which run a
//! scratch belongs to is kept in a marker named `<run>.<owner>` under the
//! checkout root's private `scratch/`, and the name is a digest of the root,
//! the run and the owner, so recovery derives every path it removes and
//! never trusts one a run could write.

pub mod cache;

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::services::checkout::Checkout;

/// The directory under the checkout root the markers are kept in.
const REGISTRY: &str = "scratch";

const PREFIX: &str = "zs-";

/// Hex digits of the digest a scratch is named by.
const NAME_LENGTH: usize = 12;

/// The directory `CARGO_TARGET_DIR` names, inside `TMPDIR` so a sandbox that
/// lets an agent write its temporary directory lets it build too.
const TARGET: &str = "target";

const TEMPORARY_VARIABLE: &str = "TMPDIR";

const TARGET_VARIABLE: &str = "CARGO_TARGET_DIR";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scratch {
    path: PathBuf,
    marker: PathBuf,
}

impl Scratch {
    /// The directory under the checkout `root` the markers are kept in.
    pub(crate) fn registry(root: &Path) -> PathBuf {
        root.join(REGISTRY)
    }

    /// The scratch of `run` held by `owner` among the runs of `root`, made or
    /// not.
    pub(crate) fn of(root: &Path, run: Uuid, owner: Uuid) -> Self {
        let mut digest = Sha256::new();
        digest.update(root.as_os_str().as_encoded_bytes());
        digest.update([0]);
        digest.update(run.as_bytes());
        digest.update(owner.as_bytes());
        let name = hex::encode(digest.finalize());
        Self {
            path: std::env::temp_dir().join(format!("{PREFIX}{}", &name[..NAME_LENGTH])),
            marker: Self::registry(root).join(format!("{run}.{owner}")),
        }
    }

    /// Make the scratch, its marker first so a crash between the two leaves
    /// recovery something to find.
    pub(crate) fn create(root: &Path, run: Uuid, owner: Uuid) -> std::io::Result<Self> {
        let registry = Self::registry(root);
        match Checkout::directory(&registry) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        Checkout::validate_root(&registry)?;
        let scratch = Self::of(root, run, owner);
        Self::private_file(&scratch.marker)?;
        Checkout::directory(&scratch.path)?;
        Checkout::validate_root(&scratch.path)?;
        Ok(scratch)
    }

    fn private_file(path: &Path) -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).map(drop)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What every command the run starts is given, so the files it makes for
    /// itself land here rather than in the host's temporary directory.
    pub fn variables(&self) -> [(&'static str, PathBuf); 2] {
        [
            (TEMPORARY_VARIABLE, self.path.clone()),
            (TARGET_VARIABLE, self.path.join(TARGET)),
        ]
    }

    /// Remove the scratch and then its marker, and say whether there was a
    /// scratch to remove. A link standing in its place is removed without
    /// being followed; a directory someone else owns is left, with its
    /// marker, and is the error.
    pub(crate) fn remove(&self) -> std::io::Result<bool> {
        if let Some(registry) = self.marker.parent() {
            Checkout::validate_root(registry)?;
        }
        let removed = match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                std::fs::remove_file(&self.path)?;
                true
            }
            Ok(metadata) if metadata.is_dir() && Self::owned(&metadata) => {
                match std::fs::remove_dir_all(&self.path) {
                    Ok(()) => true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(error) => return Err(error),
                }
            }
            Ok(_) => {
                return Err(std::io::Error::other(
                    "A run's scratch is not a directory of this user's",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        match std::fs::remove_file(&self.marker) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(removed)
    }

    /// [`Self::remove`] for a run ending, which has nobody to report to.
    pub(crate) fn release(&self) {
        if let Err(error) = self.remove() {
            tracing::error!(path = %self.path.display(), %error, "Failed to remove a run's scratch; recovery will retry");
        }
    }

    /// Remove the scratch of every run `root` holds a marker for: what a
    /// test that stages a crash leaves outside its own directory.
    #[cfg(test)]
    pub(crate) fn remove_all(root: &Path) {
        let Ok(entries) = std::fs::read_dir(Self::registry(root)) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some((run, owner)) = name.to_str().and_then(|name| name.split_once('.')) else {
                continue;
            };
            if let (Ok(run), Ok(owner)) = (Uuid::parse_str(run), Uuid::parse_str(owner)) {
                let _ = Self::of(root, run, owner).remove();
            }
        }
    }

    #[cfg(unix)]
    fn owned(metadata: &std::fs::Metadata) -> bool {
        use std::os::unix::fs::MetadataExt;
        metadata.uid() == nix::unistd::geteuid().as_raw()
    }

    #[cfg(not(unix))]
    fn owned(_: &std::fs::Metadata) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_root() -> (tempfile::TempDir, PathBuf) {
        let holder = tempfile::tempdir().unwrap();
        let root = holder.path().join("root");
        Checkout::directory(&root).unwrap();
        (holder, root)
    }

    #[test]
    fn a_run_s_scratch_is_private_and_its_marker_names_its_run() {
        let (_holder, root) = private_root();
        let (run, owner) = (Uuid::new_v4(), Uuid::new_v4());

        let scratch = Scratch::create(&root, run, owner).unwrap();

        assert_eq!(scratch, Scratch::of(&root, run, owner));
        assert_eq!(
            scratch.path().parent(),
            Some(std::env::temp_dir().as_path())
        );
        Checkout::validate_root(scratch.path()).expect("the scratch is private");
        assert!(
            Scratch::registry(&root)
                .join(format!("{run}.{owner}"))
                .is_file()
        );
        scratch.remove().unwrap();
    }

    /// A Unix socket's path holds 104 bytes on macOS and 108 on Linux, and
    /// Python's `multiprocessing` puts one 32 bytes below `TMPDIR`.
    #[test]
    fn its_temporary_directory_leaves_room_for_a_socket_beneath_it() {
        let (_holder, root) = private_root();
        let scratch = Scratch::of(&root, Uuid::new_v4(), Uuid::new_v4());
        let socket = scratch
            .path()
            .join("pymp-abcdefgh")
            .join("listener-abcdefgh");

        assert!(
            socket.as_os_str().len() < 104,
            "{} is too long to bind",
            socket.display()
        );
    }

    #[test]
    fn its_variables_point_into_it_with_the_build_inside_the_temporary_directory() {
        let (_holder, root) = private_root();
        let scratch = Scratch::create(&root, Uuid::new_v4(), Uuid::new_v4()).unwrap();

        let [(temporary, directory), (target, output)] = scratch.variables();

        assert_eq!((temporary, target), ("TMPDIR", "CARGO_TARGET_DIR"));
        assert_eq!(directory, scratch.path());
        assert!(directory.is_dir(), "TMPDIR names a directory that exists");
        assert!(output.starts_with(&directory));
        scratch.remove().unwrap();
    }

    #[test]
    fn distinct_runs_and_roots_never_share_a_scratch() {
        let (_holder, root) = private_root();
        let (_other_holder, other) = private_root();
        let (run, owner) = (Uuid::new_v4(), Uuid::new_v4());

        let scratch = Scratch::of(&root, run, owner);

        assert_ne!(scratch.path(), Scratch::of(&other, run, owner).path());
        assert_ne!(
            scratch.path(),
            Scratch::of(&root, run, Uuid::new_v4()).path()
        );
    }

    #[test]
    fn removing_it_takes_everything_a_build_left_and_its_marker() {
        let (_holder, root) = private_root();
        let (run, owner) = (Uuid::new_v4(), Uuid::new_v4());
        let scratch = Scratch::create(&root, run, owner).unwrap();
        let output = scratch.path().join(TARGET).join("debug").join("deps");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("libzone.rlib"), "artifact").unwrap();

        assert!(scratch.remove().unwrap());

        assert!(!scratch.path().exists());
        assert!(
            !Scratch::registry(&root)
                .join(format!("{run}.{owner}"))
                .exists()
        );
        assert!(!scratch.remove().unwrap(), "a second removal finds nothing");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_in_its_place_is_removed_without_being_followed() {
        let (holder, root) = private_root();
        let scratch = Scratch::create(&root, Uuid::new_v4(), Uuid::new_v4()).unwrap();
        std::fs::remove_dir(scratch.path()).unwrap();
        let elsewhere = holder.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("kept"), "kept").unwrap();
        std::os::unix::fs::symlink(&elsewhere, scratch.path()).unwrap();

        scratch.remove().unwrap();

        assert!(std::fs::symlink_metadata(scratch.path()).is_err());
        assert!(elsewhere.join("kept").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_scratch_whose_markers_others_can_reach_is_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        let (_holder, root) = private_root();
        let scratch = Scratch::create(&root, Uuid::new_v4(), Uuid::new_v4()).unwrap();
        let registry = Scratch::registry(&root);
        std::fs::set_permissions(&registry, std::fs::Permissions::from_mode(0o777)).unwrap();

        assert!(scratch.remove().is_err());
        assert!(scratch.path().exists());

        std::fs::set_permissions(&registry, std::fs::Permissions::from_mode(0o700)).unwrap();
        scratch.remove().unwrap();
    }
}
