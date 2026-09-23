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
                Error::Failed(format!(
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
        fs::rename(self.path.join(CREDENTIALS), self.home.join(CREDENTIALS))
            .map_err(|error| Error::Failed(format!("Could not save codex's new login: {error}")))
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
