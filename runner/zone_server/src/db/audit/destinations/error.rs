use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum DestinationError {
    #[error("An audit destination with this name already exists")]
    Duplicate,
    #[error("The secrets of the audit destination could not be sealed: {0}")]
    Unsealable(abnegate_secret::Error),
    #[error("The secrets of audit destination {0} did not open: {1}")]
    Unopenable(Uuid, abnegate_secret::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

pub type DestinationResult<T> = Result<T, DestinationError>;
