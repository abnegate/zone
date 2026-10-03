//! The directory a device sign-in runs in. Codex deletes the login in its `CODEX_HOME` before it
//! prints a code, so it never runs in a login's home: a refused, cancelled or expired attempt must
//! leave every login where it was, and which home a new login belongs in is known only once codex
//! has saved it.

use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::{CREDENTIALS, Error, signed_in};

const DIRECTORY: &str = ".login";
const MODE: u32 = 0o700;

#[derive(Debug)]
pub struct Staging {
    path: PathBuf,
}

impl Staging {
    /// A fresh, private `<root>/.login/<attempt>`. One device sign-in runs per organization at a
    /// time, so whatever earlier attempts left under `.login` is removed first.
    pub(super) fn create(root: &Path, attempt: Uuid) -> Result<Self, Error> {
        let parent = root.join(DIRECTORY);
        let path = parent.join(attempt.as_hyphenated().to_string());
        remove(&parent)
            .and_then(|()| private(&parent))
            .and_then(|()| private(&path))
            .map_err(|error| {
                Error::Filesystem(format!(
                    "Could not prepare {} for codex's sign-in: {error}",
                    path.display()
                ))
            })?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The staging directory, once codex has saved a login in it.
    pub(super) fn saved(self) -> Result<Self, Error> {
        if signed_in(&self.path) {
            Ok(self)
        } else {
            Err(Error::Failed(
                "codex reported a sign-in but saved no login".to_string(),
            ))
        }
    }

    /// Moves the login codex saved into `into`, a login's home on the same filesystem, over any
    /// login already there, in one rename.
    pub fn promote(&self, into: &Path) -> Result<(), Error> {
        let credentials = into.join(CREDENTIALS);
        fs::rename(self.path.join(CREDENTIALS), &credentials).map_err(|error| {
            Error::Filesystem(format!(
                "Could not save codex's new login as {}: {error}",
                credentials.display()
            ))
        })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if let Err(error) = remove(&self.path) {
            tracing::warn!(
                path = %self.path.display(),
                %error,
                "could not remove codex's sign-in staging directory"
            );
        }
        if let Some(parent) = self.path.parent() {
            let _ = fs::remove_dir(parent);
        }
    }
}

fn private(path: &Path) -> io::Result<()> {
    DirBuilder::new().mode(MODE).create(path)?;
    fs::set_permissions(path, Permissions::from_mode(MODE))
}

/// Removes `path` whatever it is, without following it if it is a link.
fn remove(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const LOCKED: u32 = 0o500;
    const UNLOCKED: u32 = 0o700;

    #[test]
    fn a_staging_directory_that_cannot_be_cleared_is_a_filesystem_failure() {
        let root = TempDir::new().expect("an agent's root");
        let leftover = root.path().join(DIRECTORY).join("leftover");
        fs::create_dir_all(&leftover).expect("a directory an earlier attempt left");
        fs::write(leftover.join("file"), b"").expect("a file an earlier attempt left");
        fs::set_permissions(&leftover, Permissions::from_mode(LOCKED))
            .expect("the leftover locked");

        let staging = Staging::create(root.path(), Uuid::new_v4());
        fs::set_permissions(&leftover, Permissions::from_mode(UNLOCKED))
            .expect("the leftover unlocked");

        let error = staging.err();
        assert!(matches!(error, Some(Error::Filesystem(_))), "{error:?}");
    }

    #[test]
    fn an_attempt_stages_under_the_root_in_a_private_directory_of_its_own() {
        let root = TempDir::new().expect("an agent's root");
        let attempt = Uuid::new_v4();
        let stale = root.path().join(DIRECTORY).join(Uuid::new_v4().to_string());
        fs::create_dir_all(&stale).expect("a directory a crashed attempt left");

        let staging = Staging::create(root.path(), attempt).expect("a staging directory");

        assert_eq!(
            staging.path(),
            root.path().join(DIRECTORY).join(attempt.to_string())
        );
        let mode = fs::metadata(staging.path())
            .expect("the staging directory")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, MODE);
        assert!(!stale.exists(), "a crashed attempt's directory was kept");
        drop(staging);
        assert!(
            !root.path().join(DIRECTORY).exists(),
            "the staging directory was left"
        );
    }

    #[test]
    fn a_saved_login_is_promoted_into_the_home_it_is_given() {
        let root = TempDir::new().expect("an agent's root");
        let home = root.path().join("logins").join("one");
        fs::create_dir_all(&home).expect("a login's home");
        fs::write(home.join(CREDENTIALS), b"old").expect("the login it replaces");
        let staging = Staging::create(root.path(), Uuid::new_v4()).expect("a staging directory");
        fs::write(staging.path().join(CREDENTIALS), b"new").expect("the login codex saved");

        let staging = staging.saved().expect("a saved login");
        staging.promote(&home).expect("the login promoted");

        assert_eq!(
            fs::read(home.join(CREDENTIALS)).expect("the promoted login"),
            b"new"
        );
        assert!(!signed_in(staging.path()));
    }

    #[test]
    fn a_staging_directory_codex_saved_nothing_in_is_no_sign_in() {
        let root = TempDir::new().expect("an agent's root");
        let staging = Staging::create(root.path(), Uuid::new_v4()).expect("a staging directory");

        let saved = staging.saved();

        assert!(matches!(saved, Err(Error::Failed(_))), "{saved:?}");
    }

    #[test]
    fn a_login_that_cannot_be_moved_into_its_home_is_a_filesystem_failure() {
        let root = TempDir::new().expect("an agent's root");
        let home = root.path().join("home");
        let staging = Staging::create(root.path(), Uuid::new_v4()).expect("a staging directory");
        fs::write(staging.path().join(CREDENTIALS), b"{}").expect("the login codex saved");
        let occupied = home.join(CREDENTIALS);
        fs::create_dir_all(&occupied).expect("a directory where the login goes");
        fs::write(occupied.join("file"), b"").expect("a file inside it");

        let promoted = staging.promote(&home);

        assert!(
            matches!(promoted, Err(Error::Filesystem(_))),
            "{promoted:?}"
        );
    }
}
