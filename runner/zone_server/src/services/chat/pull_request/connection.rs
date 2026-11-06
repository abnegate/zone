use serde::Serialize;

use super::error::Error;
use crate::services::github_app;

/// How the workspace reaches GitHub for one pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    /// A source holds a credential that can read it.
    Credentialed,
    /// No source holds a credential for it.
    Uncredentialed,
    /// Its source's GitHub App installation was removed or suspended, so
    /// GitHub must be connected again.
    Disconnected,
}

impl Connection {
    pub fn credentialed(self) -> bool {
        self == Self::Credentialed
    }
}

impl From<github_app::Error> for Connection {
    fn from(error: github_app::Error) -> Self {
        match Error::from(error) {
            Error::Disconnected => Self::Disconnected,
            _ => Self::Uncredentialed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lost_installation_is_disconnected_and_any_other_failure_uncredentialed() {
        for error in [
            github_app::Error::Revoked,
            github_app::Error::Suspended,
            github_app::Error::NotConnected,
        ] {
            assert_eq!(Connection::from(error), Connection::Disconnected);
        }
        for error in [
            github_app::Error::Credential,
            github_app::Error::Database(sqlx::Error::RowNotFound),
        ] {
            assert_eq!(Connection::from(error), Connection::Uncredentialed);
        }
        assert!(Connection::Credentialed.credentialed());
        assert!(!Connection::Disconnected.credentialed());
        assert!(!Connection::Uncredentialed.credentialed());
    }

    #[test]
    fn it_reads_as_snake_case() {
        assert_eq!(
            serde_json::to_value(Connection::Disconnected).expect("a connection serializes"),
            serde_json::json!("disconnected")
        );
    }
}
