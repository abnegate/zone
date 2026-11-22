mod change;
mod listed;
mod registered;
mod removal;
mod runner;
mod updated;

pub use change::Change;
pub use listed::Listed;
pub use registered::Registered;
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

/// How recently a runner must have been seen to count as connected.
pub const CONNECTED: TimeDelta = TimeDelta::seconds(60);

const RUNS: &str = "(SELECT COUNT(*) FROM task_runs run \
     WHERE run.runner_id = runner.id AND run.status IN ('running', 'waiting')) AS runs";

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
        "SELECT {COLUMNS}, {RUNS} FROM fleet_runners runner ORDER BY runner.name"
    )))
    .fetch_all(executor)
    .await
}

/// Runner `id` as [`list`] gives it, with its live runs.
pub async fn listed(executor: impl PgExecutor<'_>, id: Uuid) -> DbResult<Option<Listed>> {
    sqlx::query_as(AssertSqlSafe(format!(
        "SELECT {COLUMNS}, {RUNS} FROM fleet_runners runner WHERE runner.id = $1"
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

/// Removes an offline runner. A connected one is refused, since its next heartbeat would only
/// register it again.
pub async fn remove(pool: &PgPool, id: Uuid) -> DbResult<Removal> {
    let mut transaction = pool.begin().await?;
    let connected: Option<bool> = sqlx::query_scalar(
        "SELECT last_seen_at > now() - $2 FROM fleet_runners WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .bind(CONNECTED)
    .fetch_optional(&mut *transaction)
    .await?;
    let removal = match connected {
        None => Removal::Missing,
        Some(true) => Removal::Online,
        Some(false) => Removal::Removed(
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
           AND NOT EXISTS ( \
               SELECT 1 FROM task_runs run \
               WHERE run.runner_id = runner.id AND run.status IN ('running', 'waiting') \
           ) \
           AND NOT EXISTS ( \
               SELECT 1 FROM chat_leases lease \
               WHERE lease.runner_id = runner.id AND lease.expires_at > now() \
           ) \
         RETURNING {COLUMNS}"
    )))
    .bind(offline)
    .bind(State::Revoked.as_str())
    .fetch_all(executor)
    .await
}

/// Counts the connected runners that would be active and current after `change`. The caller
/// holds `fleet_policy` locked, so concurrent changes can't each leave the other's runner last.
pub async fn remaining(
    transaction: &mut Transaction<'_, Postgres>,
    change: Change,
) -> DbResult<i64> {
    let (excluded, replaced, minimum) = match change {
        Change::Cordon(runner) | Change::Revoke(runner) => (Some(runner), false, None),
        Change::Minimum(minimum) => (
            None,
            true,
            minimum.map(|minimum| minimum.version().key().to_vec()),
        ),
    };
    sqlx::query_scalar(
        "WITH minimum AS ( \
             SELECT CASE WHEN $3 THEN $4::integer[] ELSE minimum_version_key END AS key \
             FROM fleet_policy \
         ) \
         SELECT COUNT(*) FROM fleet_runners runner CROSS JOIN minimum \
         WHERE runner.state = $5 \
           AND runner.last_seen_at > now() - $1 \
           AND runner.id IS DISTINCT FROM $2 \
           AND (minimum.key IS NULL OR runner.version_key >= minimum.key)",
    )
    .bind(CONNECTED)
    .bind(excluded)
    .bind(replaced)
    .bind(minimum)
    .bind(State::Active.as_str())
    .fetch_one(&mut **transaction)
    .await
}
