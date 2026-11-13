use super::credential_message;
use crate::services::github::client::UNAUTHORIZED;
use crate::services::github::client::status_error;
use crate::services::github_app::Error as GitHubError;
use crate::services::github_app::Unauthenticated;

/// A failed GitHub operation, as the model reads it.
pub(super) struct Failure(pub(super) String);

impl Unauthenticated for Failure {
    fn unauthenticated(&self) -> bool {
        self.0 == status_error(UNAUTHORIZED)
    }
}

impl From<GitHubError> for Failure {
    fn from(error: GitHubError) -> Self {
        Self(credential_message(&error).to_string())
    }
}
