//! Failures on the GitHub App path, worded so none can carry a credential.

use abnegate_vcs::pull_request::ApplicationError;
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
    #[error(transparent)]
    Application(#[from] ApplicationError),
    #[error("The GitHub App secret could not be sealed for storage.")]
    Sealing,
    #[error("The GitHub App secret is not sealed, or could not be unsealed.")]
    Unsealing,
    #[error("The stored credential could not be decrypted.")]
    Credential,
    #[error(
        "The GitHub account this was connected through was removed from GitHub. Reconnect GitHub."
    )]
    Revoked,
    #[error(
        "The GitHub account this was connected through is suspended on GitHub. Reconnect GitHub."
    )]
    Suspended,
    #[error("The GitHub connection this was using no longer exists. Reconnect GitHub.")]
    NotConnected,
    #[error("GitHub App installations need an HTTPS GitHub API origin.")]
    Unavailable,
    #[error("The GitHub connection could not be read: {0}")]
    Database(#[from] sqlx::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_application_failure_renders_as_the_crate_worded_it() {
        assert_eq!(
            Error::from(ApplicationError::Status(404)).to_string(),
            "GitHub returned HTTP 404 for the installation token request."
        );
    }

    #[test]
    fn no_variant_echoes_key_material() {
        for error in [
            Error::Sealing,
            Error::Unsealing,
            Error::Credential,
            Error::from(ApplicationError::PrivateKey),
            Error::from(ApplicationError::Signing),
        ] {
            assert!(!error.to_string().contains("BEGIN"), "{error}");
        }
    }
}
