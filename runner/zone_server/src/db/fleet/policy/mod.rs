mod saved;
mod stored;

pub use saved::Saved;
pub use stored::Policy;

use sqlx::{PgExecutor, Postgres, Transaction};
use uuid::Uuid;

use crate::db::DbResult;
use crate::db::fleet::minimum::Minimum;

pub async fn get(executor: impl PgExecutor<'_>) -> DbResult<Policy> {
    sqlx::query_as("SELECT minimum_version, updated_by, updated_at FROM fleet_policy WHERE id")
        .fetch_one(executor)
        .await
}

/// Locks the policy row until `transaction` ends. Every change that could leave no runner able
/// to take work takes this lock first, before any runner row.
pub async fn lock(transaction: &mut Transaction<'_, Postgres>) -> DbResult<Policy> {
    sqlx::query_as(
        "SELECT minimum_version, updated_by, updated_at FROM fleet_policy WHERE id FOR UPDATE",
    )
    .fetch_one(&mut **transaction)
    .await
}

/// Sets or clears the minimum version. Writes nothing when it is already `minimum`.
pub async fn save(
    transaction: &mut Transaction<'_, Postgres>,
    minimum: Option<&Minimum>,
    actor: Uuid,
) -> DbResult<Saved> {
    let previous = lock(transaction).await?;
    if previous.minimum.as_ref() == minimum {
        return Ok(Saved {
            current: previous.clone(),
            previous,
        });
    }
    let current = sqlx::query_as(
        "UPDATE fleet_policy \
         SET minimum_version = $1, minimum_version_key = $2, updated_by = $3, updated_at = now() \
         WHERE id \
         RETURNING minimum_version, updated_by, updated_at",
    )
    .bind(minimum.map(Minimum::to_string))
    .bind(minimum.map(|minimum| minimum.version().key().to_vec()))
    .bind(actor)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(Saved { previous, current })
}
