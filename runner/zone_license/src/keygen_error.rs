use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum KeygenError {
    #[error("The operating system could not provide random bytes: {0}")]
    Random(#[from] getrandom::Error),
    #[error("{} already exists. Choose a new path; an existing key is never overwritten.", path.display())]
    Exists { path: PathBuf },
    #[error("The private key could not be written to {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
    #[error("The key could not be printed: {0}")]
    Output(#[from] io::Error),
}
