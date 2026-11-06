use std::path::Path;
use std::path::PathBuf;

/// How a sealing run is carried out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Options {
    dry_run: bool,
    backup: PathBuf,
}

impl Options {
    /// A run behind the `pg_dump -Fc` archive at `backup`, writing nothing when `dry_run`.
    pub fn new(dry_run: bool, backup: PathBuf) -> Self {
        Self { dry_run, backup }
    }

    /// Whether the run only counts.
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }

    /// The archive the run is verified against before it reads anything.
    pub fn backup(&self) -> &Path {
        &self.backup
    }
}
