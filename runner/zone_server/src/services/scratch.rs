//! Where a task run's tools put what they make and do not keep: temporary
//! files and build output.
//!
//! Each run gets a private directory of its own beside its checkout, under
//! the checkout root's `scratch/`, and every command the run starts finds
//! `TMPDIR` and `CARGO_TARGET_DIR` pointing into it. It is not inside the
//! checkout, which is the run's git worktree: what lands there would show as
//! work the run made, and a worktree holding unpublished work outlives its
//! run. The scratch never does: it goes when the run ends, and recovery
//! takes what a crash left.

pub mod cache;

use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::services::checkout::Checkout;

/// The directory under the checkout root every run's scratch is made in.
const DIRECTORY: &str = "scratch";

/// The directory `TMPDIR` names, made with the scratch since a temporary
/// directory that does not exist fails every program that asks for one.
const TEMPORARY: &str = "tmp";

/// The directory `CARGO_TARGET_DIR` names, which cargo makes when it builds.
const TARGET: &str = "target";

const TEMPORARY_VARIABLE: &str = "TMPDIR";

const TARGET_VARIABLE: &str = "CARGO_TARGET_DIR";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// The directory under `root` the runs' scratch is made in.
    pub(crate) fn root(root: &Path) -> PathBuf {
        root.join(DIRECTORY)
    }

    /// The scratch of `run` held by `owner`, made private under `root`, named
    /// as its checkout is so recovery can tell whose it is.
    pub(crate) fn create(root: &Path, run: Uuid, owner: Uuid) -> std::io::Result<Self> {
        let directory = Self::root(root);
        match Checkout::directory(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        Checkout::validate_root(&directory)?;
        let path = directory.join(format!("{run}.{owner}"));
        Checkout::directory(&path)?;
        Checkout::directory(&path.join(TEMPORARY))?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What every command the run starts is given, so the files it makes for
    /// itself land here rather than in the host's temporary directory.
    pub fn variables(&self) -> [(&'static str, PathBuf); 2] {
        [
            (TEMPORARY_VARIABLE, self.path.join(TEMPORARY)),
            (TARGET_VARIABLE, self.path.join(TARGET)),
        ]
    }

    /// Remove the scratch, through nothing but the private directory it was
    /// made in.
    pub(crate) fn remove(&self) {
        let Some(directory) = self.path.parent() else {
            return;
        };
        if Checkout::validate_root(directory).is_err() {
            tracing::error!(path = %self.path.display(), "Refusing to remove a run's scratch through an unsafe root");
            return;
        }
        if let Err(error) = std::fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::error!(path = %self.path.display(), kind = ?error.kind(), "Failed to remove a run's scratch");
        }
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
    fn a_run_s_scratch_is_private_and_names_its_run() {
        let (_holder, root) = private_root();
        let (run, owner) = (Uuid::new_v4(), Uuid::new_v4());

        let scratch = Scratch::create(&root, run, owner).unwrap();

        assert_eq!(
            scratch.path(),
            root.join(DIRECTORY).join(format!("{run}.{owner}"))
        );
        Checkout::validate_root(scratch.path()).expect("the scratch is private");
        Checkout::validate_root(&Scratch::root(&root)).expect("its root is private");
    }

    #[test]
    fn its_variables_point_into_it_and_the_temporary_directory_exists() {
        let (_holder, root) = private_root();
        let scratch = Scratch::create(&root, Uuid::new_v4(), Uuid::new_v4()).unwrap();

        let variables = scratch.variables();

        assert_eq!(
            variables.clone().map(|(name, _)| name),
            ["TMPDIR", "CARGO_TARGET_DIR"]
        );
        for (name, value) in &variables {
            assert!(
                value.starts_with(scratch.path()),
                "{name} leaves the scratch"
            );
        }
        assert!(
            variables[0].1.is_dir(),
            "TMPDIR names a directory that exists"
        );
    }

    #[test]
    fn removing_it_takes_everything_a_build_left() {
        let (_holder, root) = private_root();
        let scratch = Scratch::create(&root, Uuid::new_v4(), Uuid::new_v4()).unwrap();
        let output = scratch.path().join(TARGET).join("debug").join("deps");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("libzone.rlib"), "artifact").unwrap();

        scratch.remove();

        assert!(!scratch.path().exists());
        assert!(Scratch::root(&root).is_dir(), "the shared root stays");
    }

    #[cfg(unix)]
    #[test]
    fn a_scratch_under_a_root_others_can_reach_is_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        let (_holder, root) = private_root();
        let scratch = Scratch::create(&root, Uuid::new_v4(), Uuid::new_v4()).unwrap();
        let directory = Scratch::root(&root);
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o777)).unwrap();

        scratch.remove();

        assert!(scratch.path().exists());
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
}
