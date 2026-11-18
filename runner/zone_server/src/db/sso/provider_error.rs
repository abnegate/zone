use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("A provider with this issuer and client ID already exists")]
    Duplicate,
    #[error("The client secret of SSO provider {0} did not open: {1}")]
    Unopenable(Uuid, abnegate_secret::Error),
    #[error("The client secret could not be sealed: {0}")]
    Unsealable(abnegate_secret::Error),
    #[error("SSO provider {0} names an unknown protocol {1}")]
    Protocol(Uuid, String),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

pub type ProviderResult<T> = Result<T, ProviderError>;
