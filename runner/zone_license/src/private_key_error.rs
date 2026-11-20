use std::io;
use std::path::PathBuf;

use crate::variable::Variable;

#[derive(Debug, thiserror::Error)]
pub enum PrivateKeyError {
    #[error(
        "No private key is configured. Set {} or {}.",
        Variable::PrivateKeyFile.name(),
        Variable::PrivateKey.name()
    )]
    Missing,
    #[error(
        "Both a private key file and {} are set. Configure exactly one.",
        Variable::PrivateKey.name()
    )]
    Ambiguous,
    #[error("The private key file {} could not be read: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("The private key file {} is not a regular file.", path.display())]
    NotAFile { path: PathBuf },
    #[error(
        "The private key file {} has mode {mode:o}, which other users can access. Restrict it to 600.",
        path.display()
    )]
    Permissions { path: PathBuf, mode: u32 },
    #[error("The private key is not a single line of standard base64.")]
    Encoding,
    #[error("The private key decodes to {length} bytes, not the {expected} of an Ed25519 seed.", expected = crate::private_key::SEED_LENGTH)]
    Length { length: usize },
}
