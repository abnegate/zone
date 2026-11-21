use uuid::Uuid;
use zone_enterprise::sso::Error;

use crate::auth::password::PasswordError;
use crate::db::sso::ProviderError;
use crate::services::account::Refusal;

#[derive(Debug, thiserror::Error)]
pub enum SignInFailure {
    #[error(transparent)]
    Refused(#[from] Error),
    #[error("The session was refused: {0:?}")]
    Session(Refusal),
    #[error(transparent)]
    Storage(#[from] sqlx::Error),
    #[error(transparent)]
    Provider(ProviderError),
    #[error("The stored settings of SSO provider {0} no longer parse: {1}")]
    Unreadable(Uuid, Error),
    #[error(transparent)]
    Password(#[from] PasswordError),
    #[error("The account changed while the sign-in was linking it")]
    Stale,
    #[error("Too many sign-ins are waiting for the identity provider")]
    Busy,
}

impl SignInFailure {
    pub fn raced(&self, constraints: &[&str]) -> bool {
        match self {
            Self::Stale => true,
            Self::Storage(sqlx::Error::Database(database)) => {
                database.is_unique_violation()
                    && database
                        .constraint()
                        .is_some_and(|constraint| constraints.contains(&constraint))
            }
            _ => false,
        }
    }
}

impl From<ProviderError> for SignInFailure {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::Database(error) => Self::Storage(error),
            error => Self::Provider(error),
        }
    }
}

impl From<Refusal> for SignInFailure {
    fn from(refusal: Refusal) -> Self {
        Self::Session(refusal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_unique_violation_on_a_named_constraint_is_a_race() {
        assert!(!SignInFailure::from(sqlx::Error::RowNotFound).raced(&["users_email_key"]));
        assert!(!SignInFailure::from(Error::EmailConflict).raced(&["users_email_key"]));
        assert!(!SignInFailure::from(Refusal::DeviceBlocked).raced(&["users_email_key"]));
    }

    #[test]
    fn an_account_that_changed_before_it_was_linked_is_a_race() {
        assert!(SignInFailure::Stale.raced(&[]));
    }

    #[test]
    fn a_provider_database_error_is_a_storage_error() {
        assert!(matches!(
            SignInFailure::from(ProviderError::Database(sqlx::Error::PoolClosed)),
            SignInFailure::Storage(sqlx::Error::PoolClosed)
        ));
        assert!(matches!(
            SignInFailure::from(ProviderError::Duplicate),
            SignInFailure::Provider(ProviderError::Duplicate)
        ));
    }
}
