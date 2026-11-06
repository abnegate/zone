use std::io;
use std::path::Path;
use std::path::PathBuf;

use tokio::process::Command;

use super::error::SealingError;
use super::listing::Listing;

/// The client that reads a `pg_dump -Fc` archive's table of contents.
pub const PG_RESTORE: &str = "pg_restore";

const LIST: &str = "--list";
const END_OF_OPTIONS: &str = "--";
const LOCALE: &str = "LC_ALL";
const UNTRANSLATED: &str = "C";

/// A `pg_dump -Fc` archive that holds the data of every table sealing writes to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Backup {
    path: PathBuf,
    database: String,
}

impl Backup {
    /// The archive at `path`, once it is shown to be a non-empty `pg_dump -Fc` archive of
    /// `database` holding the rows of every one of `tables`.
    pub async fn verify(
        path: &Path,
        database: &str,
        tables: &[&str],
    ) -> Result<Self, SealingError> {
        let metadata =
            tokio::fs::metadata(path)
                .await
                .map_err(|source| SealingError::BackupMissing {
                    path: path.to_path_buf(),
                    source,
                })?;
        if !metadata.is_file() {
            return Err(SealingError::BackupNotFile {
                path: path.to_path_buf(),
            });
        }
        if metadata.len() == 0 {
            return Err(SealingError::BackupEmpty {
                path: path.to_path_buf(),
            });
        }

        let output = Command::new(PG_RESTORE)
            .arg(LIST)
            .arg(END_OF_OPTIONS)
            .arg(path)
            .env(LOCALE, UNTRANSLATED)
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|error| match error.kind() {
                io::ErrorKind::NotFound => SealingError::PgRestoreMissing,
                _ => SealingError::PgRestore(error),
            })?;
        if !output.status.success() {
            return Err(SealingError::NotAnArchive {
                path: path.to_path_buf(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        let listing = Listing::parse(&String::from_utf8_lossy(&output.stdout));
        if listing.database() != database {
            return Err(SealingError::DatabaseMismatch {
                expected: database.to_string(),
                found: listing.database().to_string(),
            });
        }
        if let Some(table) = tables.iter().find(|table| !listing.holds_data_for(table)) {
            return Err(SealingError::TableMissing {
                table: table.to_string(),
            });
        }

        Ok(Self {
            path: path.to_path_buf(),
            database: database.to_string(),
        })
    }

    /// Where the archive is.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The database the archive was dumped from.
    pub fn database(&self) -> &str {
        &self.database
    }
}
