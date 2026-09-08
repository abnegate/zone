//! Application of the embedded migrations to Postgres under an advisory lock.

use std::time::{Duration, Instant};

use sqlx::{Connection, Executor, PgConnection, PgPool};

use super::checksum::Checksum;
use super::embedded::EMBEDDED;
use super::error::MigrationError;
use super::ledger::{self, AppliedMigration};
use super::lock;
use super::plan::Plan;
use super::resolve::{Migration, resolve};

const CURRENT_DATABASE: &str = "SELECT current_database()";
const ACQUIRE_LOCK: &str = "SELECT pg_advisory_lock($1)";
const RELEASE_LOCK: &str = "SELECT pg_advisory_unlock($1)";

/// Bring the database up to the schema this binary carries and return the versions applied.
///
/// The whole run holds one pooled connection, because the advisory lock is session scoped:
/// taking it on one connection and releasing it on another would leave the lock held and the
/// next starter blocked forever.
pub async fn run(pool: &PgPool) -> Result<Vec<i64>, MigrationError> {
    let migrations = resolve(EMBEDDED)?;
    let mut connection = pool.acquire().await?;

    let database_name: String = sqlx::query_scalar(CURRENT_DATABASE)
        .fetch_one(&mut *connection)
        .await?;
    let lock_identifier = lock::identifier(&database_name);

    sqlx::query(ACQUIRE_LOCK)
        .bind(lock_identifier)
        .execute(&mut *connection)
        .await?;

    let outcome = apply_pending(&mut connection, &migrations).await;

    let released = sqlx::query(RELEASE_LOCK)
        .bind(lock_identifier)
        .execute(&mut *connection)
        .await;

    let applied = outcome?;
    released?;

    Ok(applied)
}

async fn apply_pending(
    connection: &mut PgConnection,
    migrations: &[Migration],
) -> Result<Vec<i64>, MigrationError> {
    connection
        .execute(sqlx::raw_sql(ledger::CREATE_TABLE))
        .await?;

    let applied = list_applied(&mut *connection).await?;
    let plan = Plan::new(migrations, &applied)?;

    let mut versions = Vec::with_capacity(plan.pending().len());
    for migration in plan.pending() {
        let elapsed = apply(&mut *connection, migration).await?;

        sqlx::query(ledger::UPDATE_EXECUTION_TIME)
            .bind(i64::try_from(elapsed.as_nanos()).unwrap_or(i64::MAX))
            .bind(migration.version)
            .execute(&mut *connection)
            .await?;

        versions.push(migration.version);
    }

    Ok(versions)
}

async fn list_applied(
    connection: &mut PgConnection,
) -> Result<Vec<AppliedMigration>, MigrationError> {
    let rows: Vec<(i64, Vec<u8>, bool)> = sqlx::query_as(ledger::SELECT_APPLIED)
        .fetch_all(connection)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(version, checksum, success)| AppliedMigration {
            version,
            checksum: Checksum::from_stored(checksum),
            success,
        })
        .collect())
}

async fn apply(
    connection: &mut PgConnection,
    migration: &Migration,
) -> Result<Duration, MigrationError> {
    let started = Instant::now();

    if migration.no_transaction {
        execute(&mut *connection, migration).await?;
    } else {
        let mut transaction = connection.begin().await?;
        execute(&mut transaction, migration).await?;
        transaction.commit().await?;
    }

    Ok(started.elapsed())
}

/// The ledger row is written inside the migration's own transaction so a crash between the
/// schema change and the bookkeeping cannot leave a migration applied but unrecorded.
async fn execute(
    connection: &mut PgConnection,
    migration: &Migration,
) -> Result<(), MigrationError> {
    connection
        .execute(sqlx::raw_sql(migration.sql))
        .await
        .map_err(|source| MigrationError::Execute {
            version: migration.version,
            source,
        })?;

    sqlx::query(ledger::INSERT_APPLIED)
        .bind(migration.version)
        .bind(&migration.description)
        .bind(migration.checksum.as_bytes())
        .execute(connection)
        .await?;

    Ok(())
}
