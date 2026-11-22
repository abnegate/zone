mod available;
mod owner;
mod saved;
mod stored;

pub use available::Available;
pub use saved::Saved;
pub use stored::Stored;

use sqlx::{AssertSqlSafe, PgExecutor, Postgres, Transaction};
use uuid::Uuid;
use zone_enterprise::fleet::Labels;

use crate::db::DbResult;
use crate::db::fleet::eligibility::CURRENT;
use crate::db::fleet::runners::{CONNECTED, undecodable};
use crate::db::fleet::state::State;

use owner::Owner;

pub async fn organization(
    executor: impl PgExecutor<'_>,
    organization_id: Uuid,
) -> DbResult<Option<Stored>> {
    read(executor, Owner::Organization, organization_id).await
}

pub async fn workspace(
    executor: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> DbResult<Option<Stored>> {
    read(executor, Owner::Workspace, workspace_id).await
}

/// The labels unattended work in `workspace_id` requires: its organization's and its own.
pub async fn required(executor: impl PgExecutor<'_>, workspace_id: Uuid) -> DbResult<Labels> {
    let required: Option<Vec<String>> = sqlx::query_scalar(
        "SELECT COALESCE(ol.labels, '{}') || COALESCE(wl.labels, '{}') \
         FROM workspaces w \
         LEFT JOIN organization_runner_labels ol ON ol.organization_id = w.organization_id \
         LEFT JOIN workspace_runner_labels wl ON wl.workspace_id = w.id \
         WHERE w.id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(executor)
    .await?;
    Labels::from_strings(required.unwrap_or_default()).map_err(|error| undecodable("labels", error))
}

/// Replaces the labels an organization requires, holding its row locked until `transaction`
/// ends so concurrent saves queue. Writes nothing when the labels are unchanged.
pub async fn save_organization(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    labels: &Labels,
    actor: Uuid,
) -> DbResult<Saved> {
    save(
        transaction,
        Owner::Organization,
        organization_id,
        labels,
        actor,
    )
    .await
}

/// Replaces the labels a workspace requires, as [`save_organization`] does an organization's.
pub async fn save_workspace(
    transaction: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    labels: &Labels,
    actor: Uuid,
) -> DbResult<Saved> {
    save(transaction, Owner::Workspace, workspace_id, labels, actor).await
}

/// The labels connected runners that aren't revoked carry, with how many carry each.
pub async fn catalogue(executor: impl PgExecutor<'_>) -> DbResult<Vec<Available>> {
    sqlx::query_as(
        "SELECT carried.label, COUNT(*) AS runners \
         FROM fleet_runners runner CROSS JOIN LATERAL unnest(runner.labels) AS carried(label) \
         WHERE runner.state <> $2 AND runner.last_seen_at > now() - $1 \
         GROUP BY label ORDER BY label",
    )
    .bind(CONNECTED)
    .bind(State::Revoked.as_str())
    .fetch_all(executor)
    .await
}

/// Counts the connected, active and current runners that carry every label of `required`.
pub async fn matching(executor: impl PgExecutor<'_>, required: &Labels) -> DbResult<i64> {
    sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT COUNT(*) FROM fleet_runners runner CROSS JOIN fleet_policy policy \
         WHERE runner.state = $3 AND runner.last_seen_at > now() - $1 AND {CURRENT} \
           AND $2::text[] <@ runner.labels"
    )))
    .bind(CONNECTED)
    .bind(required.to_vec())
    .bind(State::Active.as_str())
    .fetch_one(executor)
    .await
}

async fn read(executor: impl PgExecutor<'_>, owner: Owner, id: Uuid) -> DbResult<Option<Stored>> {
    sqlx::query_as(AssertSqlSafe(format!(
        "SELECT labels, updated_by, updated_at FROM {} WHERE {} = $1",
        owner.table(),
        owner.column()
    )))
    .bind(id)
    .fetch_optional(executor)
    .await
}

async fn save(
    transaction: &mut Transaction<'_, Postgres>,
    owner: Owner,
    id: Uuid,
    labels: &Labels,
    actor: Uuid,
) -> DbResult<Saved> {
    let (table, column) = (owner.table(), owner.column());
    let created = sqlx::query(AssertSqlSafe(format!(
        "INSERT INTO {table} ({column}) VALUES ($1) ON CONFLICT ({column}) DO NOTHING"
    )))
    .bind(id)
    .execute(&mut **transaction)
    .await?
    .rows_affected()
        > 0;
    let previous: Option<Stored> = if created {
        None
    } else {
        sqlx::query_as(AssertSqlSafe(format!(
            "SELECT labels, updated_by, updated_at FROM {table} WHERE {column} = $1 FOR UPDATE"
        )))
        .bind(id)
        .fetch_optional(&mut **transaction)
        .await?
    };
    if saved::labels(previous.as_ref()) == *labels {
        if created {
            sqlx::query(AssertSqlSafe(format!(
                "DELETE FROM {table} WHERE {column} = $1"
            )))
            .bind(id)
            .execute(&mut **transaction)
            .await?;
        }
        return Ok(Saved {
            current: previous.clone(),
            previous,
        });
    }
    let current: Stored = sqlx::query_as(AssertSqlSafe(format!(
        "UPDATE {table} SET labels = $2, updated_by = $3, updated_at = now() \
         WHERE {column} = $1 RETURNING labels, updated_by, updated_at"
    )))
    .bind(id)
    .bind(labels.to_vec())
    .bind(actor)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(Saved {
        previous,
        current: Some(current),
    })
}
