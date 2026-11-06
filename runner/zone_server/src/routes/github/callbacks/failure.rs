use thiserror::Error;

use crate::services::github_app::error::Error as GitHubError;
use crate::services::github_app::unrecorded::Unrecorded;

const OWNER_INDEX: &str = "github_apps_organization_owner_idx";

#[derive(Debug, Error)]
pub enum Failure {
    #[error("GitHub sent no manifest code.")]
    MissingCode,
    #[error(
        "The user who started the connection is no longer an active admin of the organization."
    )]
    NotAdmin,
    #[error("The state was issued for no GitHub App.")]
    Unbound,
    #[error("The GitHub App the state was issued for is gone.")]
    MissingApp,
    #[error("GitHub sent no valid installation number.")]
    MissingInstallation,
    #[error("GitHub registered the App under a kind of account Zone does not know.")]
    UnknownOwnerType,
    #[error("GitHub did not give an http(s) address for the App.")]
    Unaddressable,
    #[error("GitHub numbered the App beyond what Zone can store.")]
    OutOfRange,
    #[error("The organization already connected an App for that GitHub account.")]
    Duplicate,
    #[error(transparent)]
    GitHub(#[from] GitHubError),
    #[error(transparent)]
    Unrecorded(#[from] Unrecorded),
    #[error("The GitHub connection could not be stored: {0}")]
    Database(#[source] sqlx::Error),
}

impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error() {
            Some(database)
                if database.is_unique_violation() && database.constraint() == Some(OWNER_INDEX) =>
            {
                Self::Duplicate
            }
            _ => Self::Database(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::error::Error as StdError;
    use std::fmt;

    use abnegate_vcs::pull_request::ApplicationError;
    use sqlx::error::{DatabaseError, ErrorKind};

    use super::*;
    use crate::db::github_apps::testing::{pool, seed_app, seed_organization};
    use crate::db::github_apps::{NewApp, OwnerType, insert};

    #[derive(Debug)]
    struct Violation(&'static str);

    impl fmt::Display for Violation {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "duplicate key value violates {}", self.0)
        }
    }

    impl StdError for Violation {}

    impl DatabaseError for Violation {
        fn message(&self) -> &str {
            "duplicate key value violates a unique constraint"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed("23505"))
        }

        fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
            self
        }

        fn constraint(&self) -> Option<&str> {
            Some(self.0)
        }

        fn kind(&self) -> ErrorKind {
            ErrorKind::UniqueViolation
        }
    }

    fn violating(constraint: &'static str) -> sqlx::Error {
        sqlx::Error::Database(Box::new(Violation(constraint)))
    }

    #[test]
    fn a_github_failure_renders_as_the_crate_worded_it() {
        let failure = Failure::from(GitHubError::from(ApplicationError::Status(422)));
        assert_eq!(
            failure.to_string(),
            "GitHub returned HTTP 422 for the GitHub App request."
        );
    }

    #[test]
    fn a_missing_row_is_a_database_failure() {
        assert!(matches!(
            Failure::from(sqlx::Error::RowNotFound),
            Failure::Database(_)
        ));
    }

    #[test]
    fn only_the_owner_index_reads_as_a_duplicate() {
        assert!(matches!(
            Failure::from(violating(OWNER_INDEX)),
            Failure::Duplicate
        ));
        for constraint in [
            "github_apps_pkey",
            "github_connection_states_nonce_hash_key",
            "github_installations_installation_id_key",
        ] {
            assert!(
                matches!(Failure::from(violating(constraint)), Failure::Database(_)),
                "{constraint}"
            );
        }
    }

    #[tokio::test]
    async fn a_second_app_for_the_same_owner_is_a_duplicate() {
        let pool = pool().await;
        let organization = seed_organization(&pool).await;
        seed_app(&pool, organization, "acme").await;

        let error = insert(
            &pool,
            &NewApp {
                organization_id: organization,
                app_id: 1,
                slug: "zone-acme-second",
                name: "Zone ACME second",
                owner_login: "ACME",
                owner_type: OwnerType::Organization,
                html_url: "https://github.com/apps/zone-acme-second",
                client_id: "Iv1.second",
                client_secret: "ENC[v1:client-secret]",
                private_key: "ENC[v1:private-key]",
                webhook_secret: None,
                created_by: None,
            },
        )
        .await
        .expect_err("the owner already has an App");

        assert!(matches!(Failure::from(error), Failure::Duplicate));
    }
}
