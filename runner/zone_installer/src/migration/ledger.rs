//! The bookkeeping table, shared verbatim with the sqlx migrator.

use super::checksum::Checksum;

/// The sqlx default. `zone_server` migrates through `sqlx::migrate!`, which reads and
/// writes this table, so the embedded runner must use it too rather than keep a second
/// ledger that would let both paths apply the same migration.
pub const MIGRATIONS_TABLE: &str = "_sqlx_migrations";

pub const CREATE_TABLE: &str = "\
CREATE TABLE IF NOT EXISTS _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMPTZ NOT NULL DEFAULT now(),
    success BOOLEAN NOT NULL,
    checksum BYTEA NOT NULL,
    execution_time BIGINT NOT NULL
)";

pub const SELECT_APPLIED: &str =
    "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version";

pub const INSERT_APPLIED: &str = "\
INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
VALUES ($1, $2, TRUE, $3, -1)";

pub const UPDATE_EXECUTION_TIME: &str =
    "UPDATE _sqlx_migrations SET execution_time = $1 WHERE version = $2";

#[derive(Clone, Debug)]
pub struct AppliedMigration {
    pub version: i64,
    pub checksum: Checksum,
    pub success: bool,
}

#[cfg(test)]
mod tests {
    use super::{CREATE_TABLE, INSERT_APPLIED, MIGRATIONS_TABLE, SELECT_APPLIED};

    #[test]
    fn every_statement_targets_the_sqlx_table() {
        for statement in [CREATE_TABLE, SELECT_APPLIED, INSERT_APPLIED] {
            assert!(
                statement.contains(MIGRATIONS_TABLE),
                "statement must address the shared ledger: {statement}"
            );
        }
    }

    #[test]
    fn create_table_is_idempotent() {
        assert!(CREATE_TABLE.contains("IF NOT EXISTS"));
    }
}
