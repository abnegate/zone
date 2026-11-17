use uuid::Uuid;

use crate::db::audit::destinations::DestinationError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Refused(#[from] zone_enterprise::audit::Error),
    #[error("Audit destination {0} is stored in a form this server can't read")]
    Unreadable(Uuid),
    #[error(transparent)]
    Storage(DestinationError),
}

impl From<DestinationError> for Error {
    fn from(error: DestinationError) -> Self {
        match error {
            DestinationError::Duplicate => {
                Self::Refused(zone_enterprise::audit::Error::DestinationDuplicate)
            }
            error => Self::Storage(error),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Self::Storage(DestinationError::Database(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duplicate_name_is_refused_with_its_contract_code() {
        assert!(matches!(
            Error::from(DestinationError::Duplicate),
            Error::Refused(zone_enterprise::audit::Error::DestinationDuplicate)
        ));
    }

    #[test]
    fn database_errors_stay_storage_errors() {
        assert!(matches!(
            Error::from(sqlx::Error::PoolClosed),
            Error::Storage(DestinationError::Database(_))
        ));
    }
}
