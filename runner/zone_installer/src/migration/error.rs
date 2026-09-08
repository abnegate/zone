//! Failures the embedded migration runner reports instead of touching the schema.

use thiserror::Error;

use super::checksum::Checksum;
use super::ledger::MIGRATIONS_TABLE;

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("migration file name `{file_name}` must be `<version>_<description>.sql`")]
    MalformedFileName { file_name: String },

    #[error("migration file name `{file_name}` must start with a positive integer version")]
    InvalidVersion { file_name: String },

    #[error("reversible migration `{file_name}` is not supported by the embedded runner")]
    ReversibleUnsupported { file_name: String },

    #[error("version {version} is declared by more than one embedded migration")]
    DuplicateVersion { version: i64 },

    #[error("migration {version} was applied to this database but is not embedded in this binary")]
    MissingFromBinary { version: i64 },

    #[error(
        "migration {version} was applied with checksum {applied} but this binary carries {embedded}"
    )]
    ChecksumMismatch {
        version: i64,
        applied: Checksum,
        embedded: Checksum,
    },

    #[error(
        "migration {version} is pending but the newer migration {highest_applied} has already been applied"
    )]
    OutOfOrder { version: i64, highest_applied: i64 },

    #[error(
        "migration {version} is partially applied; repair the schema and delete its `{MIGRATIONS_TABLE}` row"
    )]
    PartiallyApplied { version: i64 },

    #[error("while migrating: {0}")]
    Database(#[from] sqlx::Error),

    #[error("while executing migration {version}: {source}")]
    Execute {
        version: i64,
        #[source]
        source: sqlx::Error,
    },
}
