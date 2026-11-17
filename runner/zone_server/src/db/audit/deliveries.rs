use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;
use zone_enterprise::audit::FailureCode;

use super::position::Position;
use super::predicate::scoped;
use super::scope::Scope;
use crate::db::DbResult;

const COLUMNS: &str = "delivery.destination_id, delivery.transaction_id, delivery.sequence, \
    delivery.batch_transaction_id, delivery.batch_sequence, delivery.attempts, \
    delivery.next_attempt_at, delivery.lease_owner, delivery.lease_expires_at, \
    delivery.last_success_at, delivery.last_failure_at, delivery.last_error_code, \
    delivery.last_error, delivery.delivered_count";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    pub destination_id: Uuid,
    pub delivered: Position,
    pub batch: Option<Position>,
    pub attempts: i32,
    pub next_attempt_at: DateTime<Utc>,
    pub lease_owner: Option<Uuid>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_failure_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<FailureCode>,
    pub last_error: Option<String>,
    pub delivered_count: i64,
}

/// A claimed delivery. `token` is fresh for every claim and is the only key that pins, advances,
/// fails or releases it, so a task that outlived its lease can't record a result over a later
/// claim, even one made by the same process. `claimed_at` is the database's clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub token: Uuid,
    pub delivery: Delivery,
    pub claimed_at: DateTime<Utc>,
}

/// When a released delivery is next due: after a delay on the database's clock, or at a time
/// read from the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resume {
    After(Duration),
    At(DateTime<Utc>),
}

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct Holding {
    pub destination_id: Uuid,
    pub name: String,
    pub attempts: i32,
}

#[derive(sqlx::FromRow)]
struct Row {
    destination_id: Uuid,
    transaction_id: i64,
    sequence: i64,
    batch_transaction_id: Option<i64>,
    batch_sequence: Option<i64>,
    attempts: i32,
    next_attempt_at: DateTime<Utc>,
    lease_owner: Option<Uuid>,
    lease_expires_at: Option<DateTime<Utc>>,
    last_success_at: Option<DateTime<Utc>>,
    last_failure_at: Option<DateTime<Utc>>,
    last_error_code: Option<String>,
    last_error: Option<String>,
    delivered_count: i64,
}

#[derive(sqlx::FromRow)]
struct Claimed {
    #[sqlx(flatten)]
    row: Row,
    claimed_at: DateTime<Utc>,
}

impl From<Row> for Delivery {
    fn from(row: Row) -> Self {
        let batch =
            row.batch_transaction_id
                .zip(row.batch_sequence)
                .map(|(transaction_id, sequence)| Position {
                    transaction_id,
                    sequence,
                });
        Self {
            destination_id: row.destination_id,
            delivered: Position {
                transaction_id: row.transaction_id,
                sequence: row.sequence,
            },
            batch,
            attempts: row.attempts,
            next_attempt_at: row.next_attempt_at,
            lease_owner: row.lease_owner,
            lease_expires_at: row.lease_expires_at,
            last_success_at: row.last_success_at,
            last_failure_at: row.last_failure_at,
            last_error_code: row.last_error_code.as_deref().and_then(FailureCode::parse),
            last_error: row.last_error,
            delivered_count: row.delivered_count,
        }
    }
}

pub async fn claim(executor: impl PgExecutor<'_>, lease: Duration) -> DbResult<Option<Lease>> {
    let token = Uuid::new_v4();
    let claimed: Option<Claimed> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE audit_deliveries AS delivery \
         SET lease_owner = $1, \
             lease_expires_at = statement_timestamp() + make_interval(secs => $2) \
         WHERE delivery.destination_id = ( \
             SELECT candidate.destination_id FROM audit_deliveries candidate \
             JOIN audit_destinations destination ON destination.id = candidate.destination_id \
             WHERE destination.enabled \
               AND candidate.next_attempt_at <= statement_timestamp() \
               AND (candidate.lease_expires_at IS NULL \
                 OR candidate.lease_expires_at <= statement_timestamp()) \
             ORDER BY candidate.next_attempt_at, candidate.destination_id \
             LIMIT 1 \
             FOR UPDATE OF candidate SKIP LOCKED) \
         RETURNING {COLUMNS}, statement_timestamp() AS claimed_at"
    )))
    .bind(token)
    .bind(lease.as_secs_f64())
    .fetch_optional(executor)
    .await?;
    Ok(claimed.map(|claimed| Lease {
        token,
        delivery: Delivery::from(claimed.row),
        claimed_at: claimed.claimed_at,
    }))
}

pub async fn pin(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    token: Uuid,
    through: Position,
) -> DbResult<bool> {
    let result = sqlx::query(
        "UPDATE audit_deliveries SET batch_transaction_id = $3, batch_sequence = $4 \
         WHERE destination_id = $1 AND lease_owner = $2",
    )
    .bind(id)
    .bind(token)
    .bind(through.transaction_id)
    .bind(through.sequence)
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Due again at once, but behind every destination already waiting, so a deep backlog takes
/// turns with the rest.
pub async fn advance(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    token: Uuid,
    through: Position,
    count: i64,
) -> DbResult<bool> {
    let result = sqlx::query(
        "UPDATE audit_deliveries SET transaction_id = $3, sequence = $4, \
             batch_transaction_id = NULL, batch_sequence = NULL, attempts = 0, \
             last_success_at = statement_timestamp(), delivered_count = delivered_count + $5, \
             next_attempt_at = statement_timestamp(), lease_owner = NULL, lease_expires_at = NULL \
         WHERE destination_id = $1 AND lease_owner = $2",
    )
    .bind(id)
    .bind(token)
    .bind(through.transaction_id)
    .bind(through.sequence)
    .bind(count)
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Returns when the retry is due, or `None` when the lease was lost.
pub async fn fail(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    token: Uuid,
    code: FailureCode,
    message: &str,
    delay: Duration,
) -> DbResult<Option<DateTime<Utc>>> {
    sqlx::query_scalar(
        "UPDATE audit_deliveries SET attempts = attempts + 1, \
             last_failure_at = statement_timestamp(), last_error_code = $3, last_error = $4, \
             next_attempt_at = statement_timestamp() + make_interval(secs => $5), \
             lease_owner = NULL, lease_expires_at = NULL \
         WHERE destination_id = $1 AND lease_owner = $2 \
         RETURNING next_attempt_at",
    )
    .bind(id)
    .bind(token)
    .bind(code.as_str())
    .bind(message)
    .bind(delay.as_secs_f64())
    .fetch_optional(executor)
    .await
}

pub async fn release(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    token: Uuid,
    resume: Resume,
) -> DbResult<bool> {
    let (delay, at) = match resume {
        Resume::After(delay) => (Some(delay.as_secs_f64()), None),
        Resume::At(at) => (None, Some(at)),
    };
    let result = sqlx::query(
        "UPDATE audit_deliveries \
         SET next_attempt_at = COALESCE($4, statement_timestamp() + make_interval(secs => $3)), \
             lease_owner = NULL, lease_expires_at = NULL \
         WHERE destination_id = $1 AND lease_owner = $2",
    )
    .bind(id)
    .bind(token)
    .bind(delay)
    .bind(at)
    .execute(executor)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// A lease in flight is left alone: its holder still sends and advances or fails under its own
/// token check, and the next claim repins from the cursor with the new settings.
pub async fn reset(executor: impl PgExecutor<'_>, id: Uuid) -> DbResult<()> {
    sqlx::query(
        "UPDATE audit_deliveries SET batch_transaction_id = NULL, batch_sequence = NULL, \
             attempts = 0, next_attempt_at = statement_timestamp() \
         WHERE destination_id = $1",
    )
    .bind(id)
    .execute(executor)
    .await?;
    Ok(())
}

pub async fn get(executor: impl PgExecutor<'_>, id: Uuid) -> DbResult<Option<Delivery>> {
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM audit_deliveries delivery WHERE delivery.destination_id = $1"
    )))
    .bind(id)
    .fetch_optional(executor)
    .await?;
    Ok(row.map(Delivery::from))
}

pub async fn list(executor: impl PgExecutor<'_>, scope: Scope) -> DbResult<Vec<Delivery>> {
    let predicate = scoped("destination.organization_id", scope, 1);
    let rows = sqlx::query_as::<_, Row>(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM audit_deliveries delivery \
         JOIN audit_destinations destination ON destination.id = delivery.destination_id \
         WHERE {predicate} \
         ORDER BY destination.created_at, destination.id"
    )))
    .bind(scope.organization_id())
    .fetch_all(executor)
    .await?;
    Ok(rows.into_iter().map(Delivery::from).collect())
}

pub async fn floor(executor: impl PgExecutor<'_>, scope: Scope) -> DbResult<Option<Position>> {
    let predicate = scoped("destination.organization_id", scope, 1);
    let lowest = sqlx::query_as::<_, (i64, i64)>(sqlx::AssertSqlSafe(format!(
        "SELECT delivery.transaction_id, delivery.sequence FROM audit_deliveries delivery \
         JOIN audit_destinations destination ON destination.id = delivery.destination_id \
         WHERE destination.enabled AND {predicate} \
         ORDER BY delivery.transaction_id, delivery.sequence \
         LIMIT 1"
    )))
    .bind(scope.organization_id())
    .fetch_optional(executor)
    .await?;
    Ok(lowest.map(|(transaction_id, sequence)| Position {
        transaction_id,
        sequence,
    }))
}

pub async fn holding(
    executor: impl PgExecutor<'_>,
    scope: Scope,
    before: DateTime<Utc>,
) -> DbResult<Vec<Holding>> {
    let destinations = scoped("destination.organization_id", scope, 2);
    let entries = scoped("entry.organization_id", scope, 2);
    sqlx::query_as::<_, Holding>(sqlx::AssertSqlSafe(format!(
        "SELECT destination.id AS destination_id, destination.name, delivery.attempts \
         FROM audit_deliveries delivery \
         JOIN audit_destinations destination ON destination.id = delivery.destination_id \
         WHERE destination.enabled AND {destinations} \
           AND EXISTS ( \
             SELECT 1 FROM audit_logs entry \
             WHERE {entries} AND entry.created_at < $1 \
               AND (entry.transaction_id, entry.sequence) > (delivery.transaction_id, delivery.sequence)) \
         ORDER BY destination.created_at, destination.id"
    )))
    .bind(before)
    .bind(scope.organization_id())
    .fetch_all(executor)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(last_error_code: Option<&str>) -> Row {
        Row {
            destination_id: Uuid::nil(),
            transaction_id: 1,
            sequence: 2,
            batch_transaction_id: None,
            batch_sequence: None,
            attempts: 1,
            next_attempt_at: DateTime::UNIX_EPOCH,
            lease_owner: None,
            lease_expires_at: None,
            last_success_at: None,
            last_failure_at: None,
            last_error_code: last_error_code.map(str::to_owned),
            last_error: None,
            delivered_count: 0,
        }
    }

    #[test]
    fn a_stored_failure_code_is_read_back_typed() {
        for code in FailureCode::ALL {
            assert_eq!(
                Delivery::from(row(Some(code.as_str()))).last_error_code,
                Some(code)
            );
        }
        assert_eq!(Delivery::from(row(None)).last_error_code, None);
    }

    #[test]
    fn an_unknown_stored_failure_code_is_dropped() {
        assert_eq!(Delivery::from(row(Some("gone"))).last_error_code, None);
    }
}
