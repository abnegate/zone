use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Output;

use tokio::process::Command;

use super::error::SealingError;
use super::listing::CUSTOM_FORMAT;
use super::listing::Listing;
use super::table::Table;

/// The client that reads a `pg_dump -Fc` archive's table of contents.
pub const PG_RESTORE: &str = "pg_restore";

const LIST: &str = "--list";
const FILE: &str = "--file";
const DISCARDED: &str = "/dev/null";
const END_OF_OPTIONS: &str = "--";
const LOCALE: &str = "LC_ALL";
const UNTRANSLATED: &str = "C";

/// A readable `pg_dump -Fc` archive of one database that holds every one of its tables, defined
/// and with their rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Backup {
    path: PathBuf,
    database: String,
}

impl Backup {
    /// The archive at `path`, once it is shown to be a non-empty `pg_dump -Fc` archive of
    /// `database` holding the definition and the rows of every one of `tables`, all of which
    /// `pg_restore` reads.
    pub async fn verify(
        path: &Path,
        database: &str,
        tables: &[Table],
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

        let output = restore(&[LIST], path).await?;
        if !output.status.success() {
            return Err(SealingError::NotAnArchive {
                path: path.to_path_buf(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        let listing = Listing::parse(&String::from_utf8_lossy(&output.stdout));
        if listing.format() != CUSTOM_FORMAT {
            return Err(SealingError::NotCustomFormat {
                path: path.to_path_buf(),
                format: listing.format().to_string(),
            });
        }
        if listing.database() != database {
            return Err(SealingError::DatabaseMismatch {
                expected: database.to_string(),
                found: listing.database().to_string(),
            });
        }
        if let Some(table) = tables.iter().find(|table| !listing.holds(table)) {
            return Err(SealingError::TableMissing {
                table: table.clone(),
            });
        }

        let output = restore(&[FILE, DISCARDED], path).await?;
        if !output.status.success() {
            return Err(SealingError::Unreadable {
                path: path.to_path_buf(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
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

/// `pg_restore` run with `options` over the archive at `path`.
async fn restore(options: &[&str], path: &Path) -> Result<Output, SealingError> {
    Command::new(PG_RESTORE)
        .args(options)
        .arg(END_OF_OPTIONS)
        .arg(path)
        .env(LOCALE, UNTRANSLATED)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => SealingError::PgRestoreMissing,
            _ => SealingError::PgRestore(error),
        })
}
