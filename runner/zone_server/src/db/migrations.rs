//! Schema upgrades with narrowly scoped recovery for interrupted task indexes.

use sqlx::migrate::{Migrate, MigrateError};
use sqlx::{PgConnection, PgPool};

const ACTIVE: &str = "CREATE UNIQUE INDEX task_runs_active ON public.task_runs USING btree (task_id) WHERE (status = 'running'::text)";
const HEARTBEAT: &str = "CREATE INDEX task_runs_heartbeat ON public.task_runs USING btree (heartbeat_at) WHERE (status = 'running'::text)";

/// Hold SQLx's migration lock across validation, repair and migration execution.
/// Closing this dedicated connection also releases the lock on cancellation.
pub async fn run(pool: &PgPool) -> Result<(), MigrateError> {
    let mut connection = pool.acquire().await?;
    connection.close_on_drop();
    // Detect a disconnected client even during a long index build.
    sqlx::query("SET client_connection_check_interval = '1s'")
        .execute(&mut *connection)
        .await?;
    let timeout: String = sqlx::query_scalar("SHOW lock_timeout")
        .fetch_one(&mut *connection)
        .await?;
    // A blocking advisory-lock SELECT holds a virtual transaction that a
    // concurrent index build/drop can itself need. End each wait promptly.
    sqlx::query("SET lock_timeout = '100ms'")
        .execute(&mut *connection)
        .await?;
    loop {
        match connection.lock().await {
            Ok(()) => break,
            Err(MigrateError::Execute(error))
                if error
                    .as_database_error()
                    .and_then(|error| error.code())
                    .as_deref()
                    .is_some_and(|code| matches!(code, "55P03" | "40P01")) =>
            {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            Err(error) => return Err(error),
        }
    }
    sqlx::query("SELECT set_config('lock_timeout', $1, false)")
        .bind(timeout)
        .execute(&mut *connection)
        .await?;
    let result = async {
        let mut migrator = sqlx::migrate!("./migrations");
        migrator.set_locking(false);
        // run_to stops checksum comparisons at its target, so validate every
        // applied version explicitly before any repair or older pending work.
        connection
            .ensure_migrations_table(&migrator.table_name)
            .await?;
        if let Some(version) = connection.dirty_version(&migrator.table_name).await? {
            return Err(MigrateError::Dirty(version));
        }
        for applied in connection
            .list_applied_migrations(&migrator.table_name)
            .await?
        {
            let expected = migrator
                .iter()
                .find(|migration| migration.version == applied.version)
                .ok_or(MigrateError::VersionMissing(applied.version))?;
            if applied.checksum != expected.checksum {
                return Err(MigrateError::VersionMismatch(applied.version));
            }
        }
        migrator
            .run_direct(Some(18), &mut *connection, false)
            .await?;
        repair(&mut connection).await?;
        migrator.run_direct(None, &mut *connection, false).await
    }
    .await;
    let unlocked = connection.unlock().await;
    result.and(unlocked)?;
    connection.close().await?;
    super::recovery::reconcile(pool).await?;
    Ok(())
}

async fn repair(connection: &mut PgConnection) -> Result<(), MigrateError> {
    let fenced: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid=to_regclass('public.task_runs') AND tgname='task_runs_migration_admission' AND NOT tgisinternal AND tgenabled='O' AND tgfoid=to_regprocedure('public.guard_task_run_upgrade()')) AND NOT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=21)",
    ).fetch_one(&mut *connection).await?;
    if !fenced {
        return Ok(());
    }
    for (version, name, expected, statement) in [
        (
            19_i64,
            "task_runs_active",
            ACTIVE,
            "DROP INDEX CONCURRENTLY public.task_runs_active",
        ),
        (
            20_i64,
            "task_runs_heartbeat",
            HEARTBEAT,
            "DROP INDEX CONCURRENTLY public.task_runs_heartbeat",
        ),
    ] {
        let applied: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=$1)")
                .bind(version)
                .fetch_one(&mut *connection)
                .await?;
        if applied {
            continue;
        }
        let index: Option<(bool, String)> = sqlx::query_as(
            "SELECT indisvalid, pg_get_indexdef(indexrelid) FROM pg_index WHERE indexrelid=to_regclass($1)",
        ).bind(format!("public.{name}")).fetch_optional(&mut *connection).await?;
        if let Some((valid, definition)) = index {
            if definition != expected {
                return Err(MigrateError::Execute(sqlx::Error::Protocol(format!(
                    "Refusing to repair unexpected task index {name}"
                ))));
            }
            if !valid {
                sqlx::query(statement).execute(&mut *connection).await?;
            }
        }
    }
    Ok(())
}
