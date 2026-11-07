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
    /// GitHub, or the store holding the source's connection, could not be
    /// reached, so whether a source can read it is not known yet.
    Unavailable,
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
            Error::Unreachable | Error::Storage => Self::Unavailable,
            Error::Uncredentialed | Error::Unauthenticated | Error::Refused(_) | Error::Unsaved => {
                Self::Uncredentialed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use abnegate_vcs::pull_request::ApplicationError;

    use super::*;

    #[test]
    fn a_lost_installation_is_disconnected() {
        for error in [
            github_app::Error::Revoked,
            github_app::Error::Suspended,
            github_app::Error::NotConnected,
        ] {
            assert_eq!(Connection::from(error), Connection::Disconnected);
        }
    }

    #[test]
    fn github_or_the_database_failing_is_unavailable_rather_than_uncredentialed() {
        for error in [
            github_app::Error::from(sqlx::Error::RowNotFound),
            github_app::Error::from(ApplicationError::Status(502)),
            github_app::Error::from(ApplicationError::Transport(
                "connection refused".to_string(),
            )),
        ] {
            let rendered = error.to_string();
            assert_eq!(
                Connection::from(error),
                Connection::Unavailable,
                "{rendered}"
            );
        }
    }

    #[test]
    fn a_credential_nobody_can_use_is_uncredentialed() {
        for error in [
            github_app::Error::Credential,
            github_app::Error::from(ApplicationError::Status(422)),
        ] {
            let rendered = error.to_string();
            assert_eq!(
                Connection::from(error),
                Connection::Uncredentialed,
                "{rendered}"
            );
        }
    }

    #[test]
    fn only_a_credentialed_connection_is_credentialed() {
        assert!(Connection::Credentialed.credentialed());
        for connection in [
            Connection::Uncredentialed,
            Connection::Disconnected,
            Connection::Unavailable,
        ] {
            assert!(!connection.credentialed(), "{connection:?}");
        }
    }

    #[test]
    fn it_reads_as_snake_case() {
        assert_eq!(
            serde_json::to_value(Connection::Disconnected).expect("a connection serializes"),
            serde_json::json!("disconnected")
        );
        assert_eq!(
            serde_json::to_value(Connection::Unavailable).expect("a connection serializes"),
            serde_json::json!("unavailable")
        );
    }
}
