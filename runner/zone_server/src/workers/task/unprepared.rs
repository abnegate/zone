use crate::services::backend;

/// Why a run cannot start, or an attempt cannot run, on what its route chose.
#[derive(Debug, thiserror::Error)]
pub(super) enum Unprepared {
    /// The route has no backend for it.
    #[error(transparent)]
    Backend(#[from] backend::Error),
    /// The backend cannot do what the task asks of it.
    #[error("{0}")]
    Refused(&'static str),
    /// No model the backend can run is left to run it on.
    #[error("{0}")]
    Model(String),
}
