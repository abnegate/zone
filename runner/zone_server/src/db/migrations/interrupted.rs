//! Indexes an interrupted `CREATE INDEX CONCURRENTLY` left behind.
//!
//! A concurrent build commits its catalog entry before it scans the table, so a
//! cancelled, timed-out or disconnected build leaves an invalid index under the
//! migration's name while sqlx records nothing. The retry's `IF NOT EXISTS`
//! would then skip that useless index and record the migration as applied.

use std::collections::BTreeSet;

use sqlx::migrate::{Migrate, MigrateError, Migrator};
use sqlx::{AssertSqlSafe, PgConnection};

const QUALIFIERS: [&str; 3] = ["IF", "NOT", "EXISTS"];
const INVALID_INDEXES: &str = "SELECT format('%I.%I', namespace.nspname, class.relname) \
     FROM pg_index JOIN pg_class class ON class.oid = pg_index.indexrelid \
     JOIN pg_namespace namespace ON namespace.oid = class.relnamespace \
     WHERE NOT pg_index.indisvalid AND namespace.nspname = current_schema() \
       AND class.relname = ANY($1)";

/// The index a `CREATE [UNIQUE] INDEX CONCURRENTLY` statement in `sql` builds,
/// named as Postgres stores it.
pub fn index_built(sql: &str) -> Option<String> {
    let code = sql
        .lines()
        .map(|line| line.split_once("--").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n");
    let words: Vec<&str> = code.split_whitespace().collect();
    let keyword = |at: usize, expected: &str| {
        words
            .get(at)
            .is_some_and(|word| word.eq_ignore_ascii_case(expected))
    };
    let at = (0..words.len()).find(|&at| {
        keyword(at, "INDEX")
            && keyword(at + 1, "CONCURRENTLY")
            && (at >= 1 && keyword(at - 1, "CREATE")
                || at >= 2 && keyword(at - 1, "UNIQUE") && keyword(at - 2, "CREATE"))
    })?;
    let named = words[at + 2..].iter().find(|word| {
        !QUALIFIERS
            .iter()
            .any(|qualifier| word.eq_ignore_ascii_case(qualifier))
    })?;
    let named = named.split('(').next().unwrap_or(named);
    let named = named.rsplit('.').next().unwrap_or(named);
    match named
        .strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
    {
        Some(quoted) => Some(quoted.to_string()),
        None if named.is_empty() || named.eq_ignore_ascii_case("ON") => None,
        None => Some(named.to_ascii_lowercase()),
    }
}

/// Drop each invalid index a not-yet-applied concurrent migration builds, so
/// the migration builds it again instead of skipping it. Runs under the
/// migration advisory lock, so no other boot is mid-build; an index that is
/// valid, or that no pending migration names, is left alone.
pub async fn drop_invalid_indexes(
    connection: &mut PgConnection,
    migrator: &Migrator,
) -> Result<(), MigrateError> {
    connection
        .ensure_migrations_table(&migrator.table_name)
        .await?;
    let applied: BTreeSet<i64> = connection
        .list_applied_migrations(&migrator.table_name)
        .await?
        .into_iter()
        .map(|migration| migration.version)
        .collect();
    let pending: Vec<String> = migrator
        .iter()
        .filter(|migration| migration.no_tx && !applied.contains(&migration.version))
        .filter(|migration| !migration.migration_type.is_down_migration())
        .filter_map(|migration| index_built(migration.sql.as_str()))
        .collect();
    if pending.is_empty() {
        return Ok(());
    }
    let invalid: Vec<String> = sqlx::query_scalar(INVALID_INDEXES)
        .bind(&pending)
        .fetch_all(&mut *connection)
        .await?;
    for index in &invalid {
        sqlx::query(AssertSqlSafe(format!("DROP INDEX CONCURRENTLY {index}")))
            .execute(&mut *connection)
            .await?;
        tracing::warn!(
            "dropped the invalid index {index} an interrupted migration left, to build it again"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::index_built;

    #[test]
    fn a_concurrent_build_names_the_index_it_builds() {
        for (sql, name) in [
            (
                "-- no-transaction\nCREATE INDEX CONCURRENTLY IF NOT EXISTS idx_runs\n    ON task_runs (id);",
                "idx_runs",
            ),
            (
                "create unique index concurrently Idx_Runs on task_runs(id)",
                "idx_runs",
            ),
            (
                "CREATE INDEX CONCURRENTLY public.idx_runs ON task_runs (id)",
                "idx_runs",
            ),
            (
                "CREATE INDEX CONCURRENTLY \"Idx_Runs\" ON task_runs (id)",
                "Idx_Runs",
            ),
        ] {
            assert_eq!(index_built(sql).as_deref(), Some(name), "{sql}");
        }
    }

    #[test]
    fn anything_but_a_named_concurrent_build_names_nothing() {
        for sql in [
            "DROP INDEX CONCURRENTLY IF EXISTS idx_runs",
            "CREATE INDEX idx_runs ON task_runs (id)",
            "CREATE INDEX CONCURRENTLY ON task_runs (id)",
            "-- CREATE INDEX CONCURRENTLY idx_runs ON task_runs (id)",
            "REINDEX INDEX CONCURRENTLY idx_runs",
        ] {
            assert_eq!(index_built(sql), None, "{sql}");
        }
    }
}
