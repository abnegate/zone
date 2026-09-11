//! Schema upgrades with narrowly scoped recovery for interrupted task indexes.

use sqlx::migrate::{Migrate, MigrateError};
use sqlx::{PgConnection, PgPool};

/// An index a migration builds with `CREATE INDEX CONCURRENTLY IF NOT EXISTS`,
/// and the statement that undoes an interrupted build of it.
///
/// `IF NOT EXISTS` reads the invalid leftover of a cancelled build as the index
/// it was asked for: the re-run skips with a notice, the migration is recorded,
/// and nothing is left to rebuild it. Dropping the leftover before the build
/// runs is what keeps that from being permanent.
struct Interrupted {
    version: i64,
    name: &'static str,
    definition: &'static str,
    undo: &'static str,
}

/// The `'running'`-only indexes, superseded by [`WAITING`] in 030 and 031.
const LEGACY: [Interrupted; 2] = [
    Interrupted {
        version: 19,
        name: "task_runs_active",
        definition: "CREATE UNIQUE INDEX task_runs_active ON public.task_runs USING btree (task_id) WHERE (status = 'running'::text)",
        undo: "DROP INDEX CONCURRENTLY public.task_runs_active",
    },
    Interrupted {
        version: 20,
        name: "task_runs_heartbeat",
        definition: "CREATE INDEX task_runs_heartbeat ON public.task_runs USING btree (heartbeat_at) WHERE (status = 'running'::text)",
        undo: "DROP INDEX CONCURRENTLY public.task_runs_heartbeat",
    },
];

/// The indexes that keep a parked run holding its admission slot and its lease.
/// `pg_get_indexdef` renders their `IN` predicate as `= ANY (ARRAY[...])`.
const WAITING: [Interrupted; 2] = [
    Interrupted {
        version: 27,
        name: "task_runs_active_waiting",
        definition: "CREATE UNIQUE INDEX task_runs_active_waiting ON public.task_runs USING btree (task_id) WHERE (status = ANY (ARRAY['running'::text, 'waiting'::text]))",
        undo: "DROP INDEX CONCURRENTLY public.task_runs_active_waiting",
    },
    Interrupted {
        version: 28,
        name: "task_runs_heartbeat_waiting",
        definition: "CREATE INDEX task_runs_heartbeat_waiting ON public.task_runs USING btree (heartbeat_at) WHERE (status = ANY (ARRAY['running'::text, 'waiting'::text]))",
        undo: "DROP INDEX CONCURRENTLY public.task_runs_heartbeat_waiting",
    },
];

/// Only a database still fenced by 017's admission trigger with 021 pending has
/// a [`LEGACY`] build to recover; 021 is what proves both indexes valid.
const LEGACY_PENDING: &str = "SELECT EXISTS(SELECT 1 FROM pg_trigger WHERE tgrelid=to_regclass('public.task_runs') AND tgname='task_runs_migration_admission' AND NOT tgisinternal AND tgenabled='O' AND tgfoid=to_regprocedure('public.guard_task_run_upgrade()')) AND NOT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=21)";

/// 029 is what proves both [`WAITING`] indexes valid, so anything short of it
/// leaves a build that may still need recovering.
const WAITING_PENDING: &str = "SELECT NOT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=29)";

/// Each repair runs between the migrations it recovers and the ones before
/// them: a leftover has to be gone before the build that would adopt it.
const BEFORE_LEGACY: i64 = 18;
const BEFORE_WAITING: i64 = 26;

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
            .run_direct(Some(BEFORE_LEGACY), &mut *connection, false)
            .await?;
        repair(&mut connection, LEGACY_PENDING, &LEGACY).await?;
        migrator
            .run_direct(Some(BEFORE_WAITING), &mut *connection, false)
            .await?;
        repair(&mut connection, WAITING_PENDING, &WAITING).await?;
        migrator.run_direct(None, &mut *connection, false).await
    }
    .await;
    let unlocked = connection.unlock().await;
    result.and(unlocked)?;
    connection.close().await?;
    super::recovery::reconcile(pool).await?;
    Ok(())
}

async fn repair(
    connection: &mut PgConnection,
    gate: &'static str,
    indexes: &[Interrupted],
) -> Result<(), MigrateError> {
    let pending: bool = sqlx::query_scalar(gate).fetch_one(&mut *connection).await?;
    if !pending {
        return Ok(());
    }
    for index in indexes {
        let applied: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=$1)")
                .bind(index.version)
                .fetch_one(&mut *connection)
                .await?;
        if applied {
            continue;
        }
        let name = index.name;
        let found: Option<(bool, String)> = sqlx::query_as(
            "SELECT indisvalid, pg_get_indexdef(indexrelid) FROM pg_index WHERE indexrelid=to_regclass($1)",
        ).bind(format!("public.{name}")).fetch_optional(&mut *connection).await?;
        if let Some((valid, definition)) = found {
            if definition != index.definition {
                return Err(MigrateError::Execute(sqlx::Error::Protocol(format!(
                    "Refusing to repair unexpected task index {name}"
                ))));
            }
            if !valid {
                sqlx::query(index.undo).execute(&mut *connection).await?;
            }
        }
    }
    Ok(())
}
