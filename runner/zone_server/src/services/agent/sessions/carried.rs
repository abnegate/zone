use std::path::PathBuf;

/// A session file written into another login's home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Carried {
    pub path: PathBuf,
    pub bytes: u64,
}
