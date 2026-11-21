use zone_enterprise::sso::Error;

use crate::auth::password::PasswordError;
use crate::db::sso::ProviderError;
use crate::services::account::Refusal;
use crate::services::members::AdmissionError;

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
    #[error(transparent)]
    Password(#[from] PasswordError),
    #[error("The account changed while the sign-in was linking it")]
    Stale,
    #[error("Too many sign-ins are waiting for the identity provider")]
    Busy,
    #[error("The server is at its cap of {cap} active members")]
    Full { cap: u32 },
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

impl From<AdmissionError> for SignInFailure {
    fn from(error: AdmissionError) -> Self {
        match error {
            AdmissionError::Full { cap } => Self::Full { cap },
            AdmissionError::Database(error) => Self::Storage(error),
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

    #[test]
    fn a_full_server_is_full_and_an_admission_database_error_is_a_storage_error() {
        assert!(matches!(
            SignInFailure::from(AdmissionError::Full { cap: 3 }),
            SignInFailure::Full { cap: 3 }
        ));
        assert!(matches!(
            SignInFailure::from(AdmissionError::Database(sqlx::Error::PoolClosed)),
            SignInFailure::Storage(sqlx::Error::PoolClosed)
        ));
        assert!(!SignInFailure::Full { cap: 3 }.raced(&["users_email_key"]));
    }
}
