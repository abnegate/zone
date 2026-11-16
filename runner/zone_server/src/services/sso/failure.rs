use zone_enterprise::sso::Error;

use crate::db::sso::ProviderError;

#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error(transparent)]
    Refused(#[from] Error),
    #[error(transparent)]
    Storage(ProviderError),
}

impl From<ProviderError> for Failure {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::Duplicate => Self::Refused(Error::ProviderDuplicate),
            error => Self::Storage(error),
        }
    }
}

impl From<sqlx::Error> for Failure {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(ProviderError::Database(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duplicate_provider_is_refused_with_its_contract_code() {
        assert!(matches!(
            Failure::from(ProviderError::Duplicate),
            Failure::Refused(Error::ProviderDuplicate)
        ));
    }

    #[test]
    fn storage_errors_stay_storage_errors() {
        assert!(matches!(
            Failure::from(sqlx::Error::PoolClosed),
            Failure::Storage(ProviderError::Database(_))
        ));
        assert!(matches!(
            Failure::from(ProviderError::Protocol(uuid::Uuid::nil(), "saml".into())),
            Failure::Storage(ProviderError::Protocol(..))
        ));
    }
}
