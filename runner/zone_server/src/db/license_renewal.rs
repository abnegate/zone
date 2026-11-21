use chrono::{DateTime, Utc};
use sqlx::{Executor, PgPool, Postgres};

use super::DbResult;
use super::licenses::SINGLETON;
use crate::services::license::renewal::Outcome;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct LicenseRenewal {
    pub attempted_at: DateTime<Utc>,
    pub outcome: String,
}

pub async fn get<'e, E>(executor: E) -> DbResult<Option<LicenseRenewal>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as(
        "SELECT renewal_attempted_at AS attempted_at, renewal_outcome AS outcome
         FROM licenses
         WHERE id = $1 AND renewal_attempted_at IS NOT NULL AND renewal_outcome IS NOT NULL",
    )
    .bind(SINGLETON)
    .fetch_optional(executor)
    .await
}

pub async fn record(
    pool: &PgPool,
    key: &str,
    outcome: Outcome,
    at: DateTime<Utc>,
) -> DbResult<bool> {
    let result = sqlx::query(
        "UPDATE licenses SET renewal_attempted_at = $3, renewal_outcome = $4
         WHERE id = $1 AND key = $2",
    )
    .bind(SINGLETON)
    .bind(key)
    .bind(at)
    .bind(outcome.as_str())
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}
