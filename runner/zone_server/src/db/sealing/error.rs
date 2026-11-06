use std::io;
use std::path::PathBuf;

use super::column::Column;

/// Why sealing refused to run, or stopped.
#[derive(Debug, thiserror::Error)]
pub enum SealingError {
    #[error("No backup at {}: {source}", path.display())]
    BackupMissing { path: PathBuf, source: io::Error },
    #[error("The backup at {} is not a regular file", path.display())]
    BackupNotFile { path: PathBuf },
    #[error("The backup at {} is empty", path.display())]
    BackupEmpty { path: PathBuf },
    #[error("The backup at {} is not a pg_dump -Fc archive: {stderr}", path.display())]
    NotAnArchive { path: PathBuf, stderr: String },
    #[error("The backup at {} is a {format:?} archive, not a pg_dump -Fc archive", path.display())]
    NotCustomFormat { path: PathBuf, format: String },
    #[error(
        "pg_restore is not installed, so the backup cannot be verified; install postgresql-client"
    )]
    PgRestoreMissing,
    #[error("pg_restore could not be run: {0}")]
    PgRestore(io::Error),
    #[error("The backup is of database {found:?}, not {expected:?}")]
    DatabaseMismatch { expected: String, found: String },
    #[error("The backup holds no data for table {table}")]
    TableMissing { table: String },
    #[error(
        "None of the {unopened} values that look sealed opens under this ENCRYPTION_KEY, so it is not the key they were sealed with; nothing was written"
    )]
    KeyMismatch { unopened: u64 },
    #[error("{0} is counted, never written")]
    Counted(Column),
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Sealing error: {0}")]
    Secret(#[from] abnegate_secret::Error),
}
