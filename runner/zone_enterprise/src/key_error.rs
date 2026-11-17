#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("The trusted public key is not a valid, strong Ed25519 key.")]
pub struct KeyError;
