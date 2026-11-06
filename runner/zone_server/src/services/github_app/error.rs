//! Failures on the GitHub App path, worded so none can carry a credential.

use abnegate_vcs::ApplicationError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GithubAppError {
    #[error("The GitHub App configuration is missing or invalid.")]
    Configuration,
    #[error("The GitHub App private key could not be read.")]
    PrivateKey,
    #[error("The GitHub App private key could not be sealed for storage.")]
    Sealing,
    #[error("The GitHub App private key is not sealed, or could not be unsealed.")]
    Unsealing,
    #[error("The GitHub App JWT could not be signed.")]
    Signing,
    #[error("The installation token request failed: {0}")]
    Transport(String),
    #[error("GitHub returned HTTP {0} for the installation token request.")]
    Status(u16),
    #[error("The installation token response could not be read.")]
    Response,
    #[error("The stored source credential could not be decrypted.")]
    Credential,
}

impl From<ApplicationError> for GithubAppError {
    fn from(error: ApplicationError) -> Self {
        match error {
            ApplicationError::PrivateKey => Self::PrivateKey,
            ApplicationError::Signing => Self::Signing,
            ApplicationError::Transport(cause) => Self::Transport(cause),
            ApplicationError::Status(status) => Self::Status(status),
            ApplicationError::Response => Self::Response,
            _ => Self::Configuration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAUSE: &str = "error sending request";
    const STATUS: u16 = 403;

    #[test]
    fn no_variant_echoes_key_material() {
        assert_eq!(
            GithubAppError::Status(404).to_string(),
            "GitHub returned HTTP 404 for the installation token request."
        );
        assert!(!GithubAppError::PrivateKey.to_string().contains("BEGIN"));
        assert!(!GithubAppError::Signing.to_string().contains("BEGIN"));
    }

    #[test]
    fn application_errors_keep_zones_variants() {
        assert!(matches!(
            GithubAppError::from(ApplicationError::PrivateKey),
            GithubAppError::PrivateKey
        ));
        assert!(matches!(
            GithubAppError::from(ApplicationError::Signing),
            GithubAppError::Signing
        ));
        assert!(matches!(
            GithubAppError::from(ApplicationError::Transport(CAUSE.to_string())),
            GithubAppError::Transport(cause) if cause == CAUSE
        ));
        assert!(matches!(
            GithubAppError::from(ApplicationError::Status(STATUS)),
            GithubAppError::Status(STATUS)
        ));
        assert!(matches!(
            GithubAppError::from(ApplicationError::Response),
            GithubAppError::Response
        ));
        assert!(matches!(
            GithubAppError::from(ApplicationError::InvalidOrigin),
            GithubAppError::Configuration
        ));
        assert!(matches!(
            GithubAppError::from(ApplicationError::Client),
            GithubAppError::Configuration
        ));

        for error in [
            ApplicationError::PrivateKey,
            ApplicationError::Signing,
            ApplicationError::Transport(CAUSE.to_string()),
            ApplicationError::Status(STATUS),
            ApplicationError::Response,
        ] {
            let expected = error.to_string();
            assert_eq!(
                GithubAppError::from(error).to_string(),
                expected,
                "a mapped variant must keep the crate's wording, which is Zone's"
            );
        }
    }
}
