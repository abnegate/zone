mod change;
mod listed;
mod registered;
mod remaining;
mod removal;
mod runner;
mod updated;

pub use change::Change;
pub use listed::Listed;
pub use registered::Registered;
pub use remaining::Remaining;
pub use removal::Removal;
pub use runner::Runner;
pub(crate) use runner::{COLUMNS, undecodable};
pub use updated::Updated;

use chrono::TimeDelta;
use sqlx::{AssertSqlSafe, PgExecutor, PgPool, Postgres, Transaction};
use uuid::Uuid;
use zone_enterprise::fleet::{Labels, Version};

use crate::db::DbResult;
use crate::db::fleet::state::State;
use crate::db::fleet::transition::Transition;
use crate::db::tasks::ACTIVE_RUN_STATUSES;

/// How recently a runner must have been seen to count as connected.
pub const CONNECTED: TimeDelta = TimeDelta::seconds(60);

/// The live runs of `runner` (`fleet_runners`), as the `runs` column.
fn runs() -> String {
    format!(
        "(SELECT COUNT(*) FROM task_runs run \
         WHERE run.runner_id = runner.id AND run.status IN {ACTIVE_RUN_STATUSES}) AS runs"
    )
}

/// Whether the runner aliased `runner` holds no live work: no running or waiting task run and no
/// unexpired chat lease.
fn idle() -> String {
    format!(
        "NOT EXISTS ( \
             SELECT 1 FROM task_runs run \
             WHERE run.runner_id = runner.id AND run.status IN {ACTIVE_RUN_STATUSES} \
         ) \
         AND NOT EXISTS ( \
             SELECT 1 FROM chat_leases lease \
             WHERE lease.runner_id = runner.id AND lease.expires_at > now() \
         )"
    )
}

/// Registers the process `instance` under `name`, or refreshes the row that already holds the
/// name, keeping its state. `started_at` restarts only when another process takes the name over.
pub async fn upsert(
    executor: impl PgExecutor<'_>,
    name: &str,
    instance: Uuid,
    version: &Version,
    labels: &Labels,
) -> DbResult<Registered> {
    sqlx::query_as(AssertSqlSafe(format!(
        "WITH previous AS ( \
             SELECT instance_id, last_seen_at FROM fleet_runners WHERE name = $1 \
         ) \
         INSERT INTO fleet_runners AS runner (name, instance_id, version, version_key, labels) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (name) DO UPDATE SET \
             instance_id = EXCLUDED.instance_id, \
             version = EXCLUDED.version, \
             version_key = EXCLUDED.version_key, \
             labels = EXCLUDED.labels, \
             last_seen_at = now(), \
             started_at = CASE WHEN runner.instance_id = EXCLUDED.instance_id \
                 THEN runner.started_at ELSE now() END \
         RETURNING {COLUMNS}, (runner.xmax = 0) AS created, \
             EXISTS ( \
                 SELECT 1 FROM previous \
                 WHERE previous.instance_id <> $2 AND previous.last_seen_at > now() - $6 \
             ) AS contested"
    )))
    .bind(name)
    .bind(instance)
    .bind(version.to_string())
    .bind(version.key().to_vec())
    .bind(labels.to_vec())
    .bind(CONNECTED)
    .fetch_one(executor)
    .await
}

pub async fn get(executor: impl PgExecutor<'_>, id: Uuid) -> DbResult<Option<Runner>> {
    sqlx::query_as(AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM fleet_runners runner WHERE runner.id = $1"
    )))
    .bind(id)
    .fetch_optional(executor)
    .await
}

pub async fn list(executor: impl PgExecutor<'_>) -> DbResult<Vec<Listed>> {
    sqlx::query_as(AssertSqlSafe(format!(
        "SELECT {COLUMNS}, {} FROM fleet_runners runner ORDER BY runner.name",
        runs()
    )))
    .fetch_all(executor)
    .await
}

/// Runner `id` as [`list`] gives it, with its live runs.
pub async fn listed(executor: impl PgExecutor<'_>, id: Uuid) -> DbResult<Option<Listed>> {
    sqlx::query_as(AssertSqlSafe(format!(
        "SELECT {COLUMNS}, {} FROM fleet_runners runner WHERE runner.id = $1",
        runs()
    )))
    .bind(id)
    .fetch_optional(executor)
    .await
}

/// Sets the state of runner `id`, locking its row until `transaction` ends. Writes nothing when
/// the runner is already in that state. Answers `None` for an unknown runner.
pub async fn set_state(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    transition: Transition,
    actor: Uuid,
) -> DbResult<Option<Updated>> {
    let current: Option<Runner> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM fleet_runners runner WHERE runner.id = $1 FOR UPDATE"
    )))
    .bind(id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(current) = current else {
        return Ok(None);
    };
    let state = transition.state();
    if current.state == state {
        return Ok(Some(Updated {
            previous: state,
            runner: current,
        }));
    }
    let runner: Runner = sqlx::query_as(AssertSqlSafe(format!(
        "UPDATE fleet_runners runner \
         SET state = $2, state_changed_by = $3, state_changed_at = now() \
         WHERE runner.id = $1 \
         RETURNING {COLUMNS}"
    )))
    .bind(id)
    .bind(state.as_str())
    .bind(actor)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(Some(Updated {
        previous: current.state,
        runner,
    }))
}

/// Removes an offline runner that holds no live work. A connected one is refused, since its next
/// heartbeat would only register it again, and so is one whose task runs or chat leases are still
/// live, as [`forget`] spares it.
pub async fn remove(pool: &PgPool, id: Uuid) -> DbResult<Removal> {
    let mut transaction = pool.begin().await?;
    let found: Option<(bool, bool)> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT runner.last_seen_at > now() - $2, {} \
         FROM fleet_runners runner WHERE runner.id = $1 FOR UPDATE",
        idle()
    )))
    .bind(id)
    .bind(CONNECTED)
    .fetch_optional(&mut *transaction)
    .await?;
    let removal = match found {
        None => Removal::Missing,
        Some((true, _)) => Removal::Online,
        Some((false, false)) => Removal::Busy,
        Some((false, true)) => Removal::Removed(
            sqlx::query_as(AssertSqlSafe(format!(
                "DELETE FROM fleet_runners runner WHERE runner.id = $1 RETURNING {COLUMNS}"
            )))
            .bind(id)
            .fetch_one(&mut *transaction)
            .await?,
        ),
    };
    transaction.commit().await?;
    Ok(removal)
}

/// Removes runners unseen for `offline` that hold no live work. Revoked runners are kept, so a
/// quarantined name stays quarantined.
pub async fn forget(executor: impl PgExecutor<'_>, offline: TimeDelta) -> DbResult<Vec<Runner>> {
    sqlx::query_as(AssertSqlSafe(format!(
        "DELETE FROM fleet_runners runner \
         WHERE runner.state <> $2 \
           AND runner.last_seen_at <= now() - $1 \
           AND {} \
         RETURNING {COLUMNS}",
        idle()
    )))
    .bind(offline)
    .bind(State::Revoked.as_str())
    .fetch_all(executor)
    .await
}

/// Counts the connected runners that are active and current now, and that would be after
/// `change`. The caller holds `fleet_policy` locked, so concurrent changes can't each leave the
/// other's runner last.
pub async fn remaining(
    transaction: &mut Transaction<'_, Postgres>,
    change: Change,
) -> DbResult<Remaining> {
    let (excluded, replaced, minimum) = match change {
        Change::Cordon(runner) | Change::Revoke(runner) => (Some(runner), false, None),
        Change::Minimum(minimum) => (
            None,
            true,
            minimum.map(|minimum| minimum.version().key().to_vec()),
        ),
    };
    sqlx::query_as(
        "WITH minimum AS ( \
             SELECT minimum_version_key AS current, \
                 CASE WHEN $3 THEN $4::integer[] ELSE minimum_version_key END AS next \
             FROM fleet_policy \
         ) \
         SELECT \
             COUNT(*) FILTER ( \
                 WHERE minimum.current IS NULL OR runner.version_key >= minimum.current \
             ) AS before, \
             COUNT(*) FILTER ( \
                 WHERE runner.id IS DISTINCT FROM $2 \
                   AND (minimum.next IS NULL OR runner.version_key >= minimum.next) \
             ) AS after \
         FROM fleet_runners runner CROSS JOIN minimum \
         WHERE runner.state = $5 AND runner.last_seen_at > now() - $1",
    )
    .bind(CONNECTED)
    .bind(excluded)
    .bind(replaced)
    .bind(minimum)
    .bind(State::Active.as_str())
    .fetch_one(&mut **transaction)
    .await
}
