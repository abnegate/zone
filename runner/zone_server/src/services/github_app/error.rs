//! Failures on the GitHub App path, worded so none can carry a credential.

use std::sync::Arc;

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
    #[error("The GitHub connection could not be read: {0}")]
    Database(#[source] Arc<sqlx::Error>),
}

impl From<sqlx::Error> for Error {
    fn from(fault: sqlx::Error) -> Self {
        Self::Database(Arc::new(fault))
    }
}

impl Error {
    pub fn disconnects(&self) -> bool {
        matches!(self, Self::Revoked | Self::Suspended | Self::NotConnected)
    }

    pub(super) fn duplicate(&self) -> Self {
        match self {
            Self::Application(fault) => Self::Application(Self::application(fault)),
            Self::Sealing => Self::Sealing,
            Self::Unsealing => Self::Unsealing,
            Self::Credential => Self::Credential,
            Self::Revoked => Self::Revoked,
            Self::Suspended => Self::Suspended,
            Self::NotConnected => Self::NotConnected,
            Self::Database(fault) => Self::Database(Arc::clone(fault)),
        }
    }

    fn application(fault: &ApplicationError) -> ApplicationError {
        match fault {
            ApplicationError::InvalidOrigin => ApplicationError::InvalidOrigin,
            ApplicationError::Client => ApplicationError::Client,
            ApplicationError::PrivateKey => ApplicationError::PrivateKey,
            ApplicationError::Signing => ApplicationError::Signing,
            ApplicationError::Transport(cause) => ApplicationError::Transport(cause.clone()),
            ApplicationError::Status(status) => ApplicationError::Status(*status),
            ApplicationError::Response => ApplicationError::Response,
            ApplicationError::InvalidCode => ApplicationError::InvalidCode,
            ApplicationError::AuthenticationRequired => ApplicationError::AuthenticationRequired,
            unknown => ApplicationError::Transport(unknown.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_application_failure_renders_as_the_crate_worded_it() {
        assert_eq!(
            Error::from(ApplicationError::Status(404)).to_string(),
            "GitHub returned HTTP 404 for the GitHub App request."
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

    #[test]
    fn a_duplicate_is_the_same_failure() {
        for error in [
            Error::Sealing,
            Error::Unsealing,
            Error::Credential,
            Error::Revoked,
            Error::Suspended,
            Error::NotConnected,
            Error::from(ApplicationError::InvalidOrigin),
            Error::from(ApplicationError::Client),
            Error::from(ApplicationError::PrivateKey),
            Error::from(ApplicationError::Signing),
            Error::from(ApplicationError::Transport(
                "connection refused".to_string(),
            )),
            Error::from(ApplicationError::Status(503)),
            Error::from(ApplicationError::Response),
            Error::from(ApplicationError::InvalidCode),
            Error::from(ApplicationError::AuthenticationRequired),
            Error::from(sqlx::Error::PoolTimedOut),
        ] {
            let duplicate = error.duplicate();
            assert_eq!(duplicate.to_string(), error.to_string());
            assert_eq!(
                std::mem::discriminant(&duplicate),
                std::mem::discriminant(&error)
            );
        }
    }

    #[test]
    fn only_a_lost_connection_disconnects() {
        for error in [Error::Revoked, Error::Suspended, Error::NotConnected] {
            assert!(error.disconnects(), "{error:?}");
        }

        for error in [
            Error::Sealing,
            Error::Unsealing,
            Error::Credential,
            Error::from(ApplicationError::Status(404)),
            Error::from(ApplicationError::Transport("refused".to_string())),
            Error::from(sqlx::Error::PoolTimedOut),
        ] {
            assert!(!error.disconnects(), "{error:?}");
        }
    }

    #[test]
    fn a_duplicate_status_keeps_its_variant() {
        assert!(matches!(
            Error::from(ApplicationError::Status(404)).duplicate(),
            Error::Application(ApplicationError::Status(404))
        ));
    }
}
