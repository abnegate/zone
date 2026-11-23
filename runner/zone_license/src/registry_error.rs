use std::io;

use crate::private_key_error::PrivateKeyError;
use crate::untrusted::Untrusted;
use crate::variable::Variable;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("No key id is configured. Pass --key-id or set {}.", Variable::KeyId.name())]
    MissingKeyId,
    #[error(transparent)]
    PrivateKey(#[from] PrivateKeyError),
    #[error("The production keyring does not trust this signing key: {0}")]
    Untrusted(#[from] Untrusted),
    #[error("The key set could not be printed: {0}")]
    Output(#[from] io::Error),
}
