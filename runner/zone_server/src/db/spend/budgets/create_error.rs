#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error("The target already has a budget")]
    Exists,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}
