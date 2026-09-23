//! The directory a device sign-in runs in. Codex deletes the login in its `CODEX_HOME` before it
//! prints a code, so it never runs in the organization's home: a refused, cancelled or expired
//! attempt must leave the existing login where it was.

use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::{CREDENTIALS, Error, signed_in};

const DIRECTORY: &str = ".login";
const MODE: u32 = 0o700;

pub(super) struct Staging {
    home: PathBuf,
    path: PathBuf,
}

impl Staging {
    /// A fresh, private directory inside `home`, replacing whatever an earlier attempt left there.
    pub(super) fn create(home: &Path) -> Result<Self, Error> {
        let path = home.join(DIRECTORY);
        remove(&path)
            .and_then(|()| DirBuilder::new().mode(MODE).create(&path))
            .and_then(|()| fs::set_permissions(&path, Permissions::from_mode(MODE)))
            .map_err(|error| {
                Error::Filesystem(format!(
                    "Could not prepare {} for codex's sign-in: {error}",
                    path.display()
                ))
            })?;
        Ok(Self {
            home: home.to_path_buf(),
            path,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// Moves the login codex saved over the organization's, in one rename on one filesystem.
    pub(super) fn promote(self) -> Result<(), Error> {
        if !signed_in(&self.path) {
            return Err(Error::Failed(
                "codex reported a sign-in but saved no login".to_string(),
            ));
        }
        let credentials = self.home.join(CREDENTIALS);
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
    }
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
        let home = TempDir::new().expect("an organization's home");
        let leftover = home.path().join(DIRECTORY).join("leftover");
        fs::create_dir_all(&leftover).expect("a directory an earlier attempt left");
        fs::write(leftover.join("file"), b"").expect("a file an earlier attempt left");
        fs::set_permissions(&leftover, Permissions::from_mode(LOCKED))
            .expect("the leftover locked");

        let staging = Staging::create(home.path());
        fs::set_permissions(&leftover, Permissions::from_mode(UNLOCKED))
            .expect("the leftover unlocked");

        let error = staging.err();
        assert!(matches!(error, Some(Error::Filesystem(_))), "{error:?}");
    }

    #[test]
    fn a_login_that_cannot_be_moved_into_the_home_is_a_filesystem_failure() {
        let home = TempDir::new().expect("an organization's home");
        let staging = Staging::create(home.path()).expect("a staging directory");
        fs::write(staging.path().join(CREDENTIALS), b"{}").expect("the login codex saved");
        let occupied = home.path().join(CREDENTIALS);
        fs::create_dir(&occupied).expect("a directory where the login goes");
        fs::write(occupied.join("file"), b"").expect("a file inside it");

        let promoted = staging.promote();

        assert!(
            matches!(promoted, Err(Error::Filesystem(_))),
            "{promoted:?}"
        );
    }
}
