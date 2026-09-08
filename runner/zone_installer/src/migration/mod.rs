//! Schema migrations compiled into the client binary.
//!
//! The client ships as one file with no migrate step beside it, so the SQL that
//! `zone_server` applies through `sqlx::migrate!` is embedded here and applied by the
//! same rules: the same version order, the same SHA-384 checksums, the same
//! `_sqlx_migrations` ledger and the same advisory lock. Sharing all four is what keeps
//! the two paths one schema rather than two.

mod checksum;
mod embedded;
mod error;
mod ledger;
mod lock;
mod plan;
mod resolve;
mod runner;

pub use checksum::Checksum;
pub use embedded::{EMBEDDED, MIGRATIONS_DIRECTORY, Source};
pub use error::MigrationError;
pub use ledger::{AppliedMigration, MIGRATIONS_TABLE};
pub use plan::Plan;
pub use resolve::{Migration, resolve};
pub use runner::run;

/// Every embedded migration in version order, without touching a database.
pub fn embedded() -> Result<Vec<Migration>, MigrationError> {
    resolve(EMBEDDED)
}
