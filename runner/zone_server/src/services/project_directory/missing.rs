use std::path::{Path, PathBuf};
use std::time::Duration;

use abnegate_secret::{REDACTED, SecretValue};
use abnegate_vcs::RepositoryUrl;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::services::checkout::{self, Checkout};

const GIT: &str = ".git";
const STAGING: &str = "staging";

/// A repository a project's chat starts in that nobody has cloned yet.
///
/// Its clone is the workspace's: a private repository cloned with one workspace's token is never
/// handed to another's chat. Under the workspace it is named by a digest of the URL, so a URL
/// never becomes a path. The clone is made beside where it will stand and moved there whole, so
/// a clone that was stopped or crashed is never mistaken for one that finished.
pub struct Missing {
    url: RepositoryUrl,
    token: Option<SecretValue>,
    destination: PathBuf,
    timeout: Duration,
}

impl Missing {
    pub fn new(
        root: &Path,
        workspace: Uuid,
        url: RepositoryUrl,
        token: Option<SecretValue>,
        timeout: Duration,
    ) -> Self {
        let destination = root
            .join(workspace.as_hyphenated().to_string())
            .join(hex::encode(Sha256::digest(url.as_str().as_bytes())));
        Self {
            url,
            token,
            destination,
            timeout,
        }
    }

    pub fn url(&self) -> &RepositoryUrl {
        &self.url
    }

    /// Where the clone stands, once it does.
    pub fn destination(&self) -> &Path {
        &self.destination
    }

    /// Whether the clone has already been made.
    pub fn present(&self) -> bool {
        std::fs::symlink_metadata(self.destination.join(GIT))
            .is_ok_and(|metadata| metadata.is_dir())
    }

    /// Clones the repository, unless another chat did while this one waited for the
    /// repository's lock, and returns where it stands. Dropping this ends the `git` it started.
    pub async fn clone_repository(self) -> Result<PathBuf, String> {
        self.attempt().await.map_err(|error| {
            let error = match &self.token {
                Some(token) => error.replace(token.expose(), REDACTED),
                None => error,
            };
            format!("Zone couldn't clone {}: {error}", self.url)
        })
    }

    async fn attempt(&self) -> Result<PathBuf, String> {
        let workspace = self
            .destination
            .parent()
            .ok_or("The clone has no parent directory")?
            .to_path_buf();
        Checkout::filesystem(move || prepare_parent(&workspace)).await?;
        let _lock = Checkout::lock_base(&self.destination).await?;
        if self.present() {
            return Ok(self.destination.clone());
        }
        if self.destination.exists() {
            return Err(format!(
                "{} exists but is not a checkout; move it aside so Zone can clone there",
                self.destination.display()
            ));
        }
        let staging = self.destination.with_extension(STAGING);
        Checkout::filesystem({
            let staging = staging.clone();
            move || remove_directory(&staging)
        })
        .await?;
        checkout::git()
            .with_timeout(self.timeout)
            .clone_repository(&self.url, &staging, self.token.as_ref())
            .await
            .map_err(|error| error.to_string())?;
        let destination = self.destination.clone();
        Checkout::filesystem(move || std::fs::rename(&staging, &destination)).await?;
        Ok(self.destination.clone())
    }
}

/// Makes the clone root, which an operator may already own and keeps its mode when it does, and
/// the workspace's directory under it, which only this user can read: what it holds may have
/// been cloned with a credential.
fn prepare_parent(workspace: &Path) -> std::io::Result<()> {
    if let Some(root) = workspace.parent() {
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.recursive(true).create(root)?;
    }
    match Checkout::directory(workspace) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    Checkout::validate_root(workspace)
}

fn remove_directory(directory: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(directory) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
