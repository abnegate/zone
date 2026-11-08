//! Why a Claude sign-in's tokens could not be sealed for storage, or opened from it.

#[derive(Debug, thiserror::Error)]
#[error("The Claude sign-in could not be sealed or opened")]
pub struct SealingError;
