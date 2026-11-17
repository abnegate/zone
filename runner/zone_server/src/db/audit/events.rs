use chrono::{DateTime, Utc};
use sqlx::{AssertSqlSafe, Executor, PgConnection, PgPool, Postgres};
use uuid::Uuid;

use super::position::Position;
use super::predicate;
use super::scope::Scope;
use crate::db::DbResult;

pub const REBASE_LOCK: i64 = 0x7A6F_6E65_4155_4454;

const REBASE_ATTEMPTS: u32 = 3;
const SERIALIZATION_FAILURE: &str = "40001";
const SCOPE: &str = "{scope}";
const COLUMN: &str = "organization_id";

macro_rules! columns {
    () => {
        "transaction_id, sequence, id, created_at, organization_id, workspace_id, actor_id, \
         actor_email, action, resource_type, resource_id, old_values, new_values, \
         host(ip_address) AS ip_address, user_agent"
    };
}

macro_rules! xmin {
    () => {
        "((pg_snapshot_xmin(pg_current_snapshot()))::text)::bigint"
    };
}

macro_rules! xmax {
    () => {
        "((pg_snapshot_xmax(pg_current_snapshot()))::text)::bigint"
    };
}

const DETECTED: &str = concat!(
    "SELECT EXISTS (SELECT 1 FROM audit_logs WHERE transaction_id >= ",
    xmax!(),
    ") OR EXISTS (SELECT 1 FROM audit_deliveries WHERE transaction_id >= ",
    xmax!(),
    " OR batch_transaction_id >= ",
    xmax!(),
    ")"
);

#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct Row {
    pub transaction_id: i64,
    pub sequence: i64,
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub organization_id: Option<Uuid>,
    pub workspace_id: Option<Uuid>,
    pub actor_id: Option<Uuid>,
    pub actor_email: Option<String>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub old_values: Option<serde_json::Value>,
    pub new_values: Option<serde_json::Value>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl Row {
    pub fn position(&self) -> Position {
        Position {
            transaction_id: self.transaction_id,
            sequence: self.sequence,
        }
    }

    pub fn bookmark(&self) -> Bookmark {
        Bookmark {
            created_at: self.created_at,
            id: self.id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct Backlog {
    pub count: i64,
    pub oldest: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bookmark {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::FromRow)]
pub struct Expired {
    pub total: i64,
    pub held: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct Cursor {
    destination_id: Uuid,
    organization_id: Option<Uuid>,
    transaction_id: i64,
    sequence: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct Sequences {
    moved: Option<i64>,
    highest: Option<i64>,
    issued: Option<i64>,
}

impl Sequences {
    fn lagged(&self) -> bool {
        self.highest
            .is_some_and(|highest| self.issued.is_none_or(|issued| issued < highest))
    }
}

fn scoped(scope: Scope, sql: &'static str) -> AssertSqlSafe<String> {
    AssertSqlSafe(sql.replace(SCOPE, &predicate::scoped(COLUMN, scope, 1)))
}

/// The position a new destination starts from: every row below the snapshot horizon counts as
/// delivered. Run on the pool, or in a transaction that has not written, because a transaction's
/// own xid holds the horizon back.
pub async fn start<'e, E>(executor: E) -> DbResult<Position>
where
    E: Executor<'e, Database = Postgres>,
{
    let transaction_id: i64 = sqlx::query_scalar(concat!("SELECT ", xmin!(), " - 1"))
        .fetch_one(executor)
        .await?;
    Ok(Position {
        transaction_id,
        sequence: i64::MAX,
    })
}

/// The final rows of `scope` after `after` and up to `through`, in position order. A row is final
/// once no transaction that could still commit behind it is open. Run on the pool, or in a
/// transaction that has not written, because a transaction's own xid holds the horizon back.
pub async fn after<'e, E>(
    executor: E,
    scope: Scope,
    after: Position,
    through: Option<Position>,
    limit: i64,
) -> DbResult<Vec<Row>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as::<_, Row>(scoped(
        scope,
        concat!(
            "SELECT ",
            columns!(),
            " FROM audit_logs WHERE {scope} AND (transaction_id, sequence) > ($2, $3) \
             AND ($4::bigint IS NULL OR (transaction_id, sequence) <= ($4, $5)) \
             AND transaction_id < ",
            xmin!(),
            " ORDER BY transaction_id, sequence LIMIT $6"
        ),
    ))
    .bind(scope.organization_id())
    .bind(after.transaction_id)
    .bind(after.sequence)
    .bind(through.map(|through| through.transaction_id))
    .bind(through.map(|through| through.sequence))
    .bind(limit)
    .fetch_all(executor)
    .await
}

/// The committed rows of `scope` after `after`, final or not. Run on the pool, or in a
/// transaction that has not written.
pub async fn backlog<'e, E>(executor: E, scope: Scope, after: Position) -> DbResult<Backlog>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as::<_, Backlog>(scoped(
        scope,
        "SELECT count(*) AS count, min(created_at) AS oldest FROM audit_logs \
         WHERE {scope} AND (transaction_id, sequence) > ($2, $3)",
    ))
    .bind(scope.organization_id())
    .bind(after.transaction_id)
    .bind(after.sequence)
    .fetch_one(executor)
    .await
}

/// When the oldest committed row of `scope` that an open transaction keeps from being final was
/// written. Run on the pool, or in a transaction that has not written, because a transaction's
/// own xid holds the horizon back.
pub async fn stalled<'e, E>(executor: E, scope: Scope) -> DbResult<Option<DateTime<Utc>>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar(scoped(
        scope,
        concat!(
            "SELECT min(created_at) FROM audit_logs WHERE {scope} AND transaction_id >= ",
            xmin!()
        ),
    ))
    .bind(scope.organization_id())
    .fetch_one(executor)
    .await
}

pub async fn page<'e, E>(
    executor: E,
    scope: Scope,
    range: Range,
    after: Option<Bookmark>,
    limit: i64,
) -> DbResult<Vec<Row>>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as::<_, Row>(scoped(
        scope,
        concat!(
            "SELECT ",
            columns!(),
            " FROM audit_logs WHERE {scope} AND created_at >= $2 AND created_at <= $3 \
             AND ($4::timestamptz IS NULL OR (created_at, id) > ($4, $5)) \
             ORDER BY created_at, id LIMIT $6"
        ),
    ))
    .bind(scope.organization_id())
    .bind(range.start)
    .bind(range.end)
    .bind(after.map(|bookmark| bookmark.created_at))
    .bind(after.map(|bookmark| bookmark.id))
    .bind(limit)
    .fetch_all(executor)
    .await
}

/// Deletes up to `limit` rows of `scope` written before `before` and at or below `floor`. No
/// floor means no enabled destination holds the rows back.
pub async fn prune<'e, E>(
    executor: E,
    scope: Scope,
    before: DateTime<Utc>,
    floor: Option<Position>,
    limit: i64,
) -> DbResult<u64>
where
    E: Executor<'e, Database = Postgres>,
{
    let result = sqlx::query(scoped(
        scope,
        "DELETE FROM audit_logs WHERE id IN (SELECT id FROM audit_logs WHERE {scope} \
         AND created_at < $2 \
         AND ($3::bigint IS NULL OR (transaction_id, sequence) <= ($3, $4)) \
         LIMIT $5 FOR UPDATE SKIP LOCKED)",
    ))
    .bind(scope.organization_id())
    .bind(before)
    .bind(floor.map(|floor| floor.transaction_id))
    .bind(floor.map(|floor| floor.sequence))
    .bind(limit)
    .execute(executor)
    .await?;
    Ok(result.rows_affected())
}

/// How many rows of `scope` were written before `before`, and how many of those sit above
/// `floor`, still owed to a destination.
pub async fn expired<'e, E>(
    executor: E,
    scope: Scope,
    before: DateTime<Utc>,
    floor: Option<Position>,
) -> DbResult<Expired>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_as::<_, Expired>(scoped(
        scope,
        "SELECT count(*) AS total, count(*) FILTER (WHERE $3::bigint IS NOT NULL \
         AND (transaction_id, sequence) > ($3, $4)) AS held \
         FROM audit_logs WHERE {scope} AND created_at < $2",
    ))
    .bind(scope.organization_id())
    .bind(before)
    .bind(floor.map(|floor| floor.transaction_id))
    .bind(floor.map(|floor| floor.sequence))
    .fetch_one(executor)
    .await
}

/// Moves every position below the transaction horizon when a logical restore left rows or
/// cursors at or above it, where a new transaction would sort behind them. Undelivered rows stay
/// after their destination's cursor, at the cost of sending some rows twice. Returns whether it
/// rebased.
pub async fn rebase(pool: &PgPool) -> DbResult<bool> {
    let mut attempt = 1;
    loop {
        if !detected(pool).await? {
            return Ok(false);
        }
        match rebased(pool).await {
            Err(error) if attempt < REBASE_ATTEMPTS && serialization_failure(&error) => {
                attempt += 1;
            }
            outcome => return outcome,
        }
    }
}

async fn detected<'e, E>(executor: E) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar(DETECTED).fetch_one(executor).await
}

fn serialization_failure(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == SERIALIZATION_FAILURE)
}

async fn rebased(pool: &PgPool) -> DbResult<bool> {
    // Repeatable read keeps a row that commits mid-rebase out of every statement here, so it
    // keeps its own transaction id and sorts after every rebased cursor.
    let mut transaction = pool
        .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(REBASE_LOCK)
        .execute(&mut *transaction)
        .await?;
    if !detected(&mut *transaction).await? {
        transaction.commit().await?;
        return Ok(false);
    }
    let horizon: i64 = sqlx::query_scalar(concat!("SELECT ", xmax!()))
        .fetch_one(&mut *transaction)
        .await?;
    let cursors = sqlx::query_as::<_, Cursor>(
        "SELECT d.destination_id, t.organization_id, d.transaction_id, d.sequence \
         FROM audit_deliveries d JOIN audit_destinations t ON t.id = d.destination_id \
         FOR UPDATE OF d",
    )
    .fetch_all(&mut *transaction)
    .await?;
    let sequences = sqlx::query_as::<_, Sequences>(
        "SELECT (SELECT max(sequence) FROM audit_logs WHERE transaction_id >= $1) AS moved, \
         (SELECT max(sequence) FROM audit_logs) AS highest, \
         pg_sequence_last_value(pg_get_serial_sequence('public.audit_logs', 'sequence')::regclass) \
         AS issued",
    )
    .bind(horizon)
    .fetch_one(&mut *transaction)
    .await?;
    let lagged = sequences.lagged();
    let renumbered = match sequences.highest.filter(|_| lagged) {
        Some(highest) => renumber(&mut transaction, horizon, highest).await?,
        None => 0,
    };
    for cursor in &cursors {
        let sequence = restart(&mut transaction, cursor, horizon, sequences.moved, lagged).await?;
        sqlx::query(
            "UPDATE audit_deliveries SET transaction_id = 0, sequence = $2, \
             batch_transaction_id = NULL, batch_sequence = NULL, lease_owner = NULL, \
             lease_expires_at = NULL WHERE destination_id = $1",
        )
        .bind(cursor.destination_id)
        .bind(sequence)
        .execute(&mut *transaction)
        .await?;
    }
    let rows = sqlx::query("UPDATE audit_logs SET transaction_id = 0 WHERE transaction_id <> 0")
        .execute(&mut *transaction)
        .await?
        .rows_affected();
    transaction.commit().await?;
    tracing::warn!(
        horizon,
        deliveries = cursors.len(),
        rows,
        renumbered,
        lagged,
        "Audit positions rebased after a logical restore; undelivered events will be sent again"
    );
    Ok(true)
}

async fn renumber(connection: &mut PgConnection, horizon: i64, highest: i64) -> DbResult<u64> {
    sqlx::query("SELECT setval(pg_get_serial_sequence('public.audit_logs', 'sequence'), $1)")
        .bind(highest)
        .execute(&mut *connection)
        .await?;
    let result = sqlx::query("UPDATE audit_logs SET sequence = DEFAULT WHERE transaction_id < $1")
        .bind(horizon)
        .execute(&mut *connection)
        .await?;
    Ok(result.rows_affected())
}

async fn restart(
    connection: &mut PgConnection,
    cursor: &Cursor,
    horizon: i64,
    moved: Option<i64>,
    lagged: bool,
) -> DbResult<i64> {
    let scope = Scope::from(cursor.organization_id);
    sqlx::query_scalar(scoped(
        scope,
        "SELECT COALESCE( \
         (SELECT min(sequence) - 1 FROM audit_logs WHERE {scope} \
         AND ((transaction_id, sequence) > ($2, $3) \
         OR (transaction_id < $4 AND ($5::bigint IS NULL OR $6 OR sequence > $5)))), \
         (SELECT max(sequence) FROM audit_logs WHERE {scope}), \
         0)",
    ))
    .bind(scope.organization_id())
    .bind(cursor.transaction_id)
    .bind(cursor.sequence)
    .bind(horizon)
    .bind(moved)
    .bind(lagged)
    .fetch_one(connection)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::reminders;

    #[test]
    fn rebase_lock_differs_from_the_reminder_claim_lock() {
        assert_ne!(REBASE_LOCK, reminders::CLAIM_LOCK);
    }

    #[test]
    fn scope_predicate_binds_the_organization_in_both_scopes() {
        let sql = "SELECT 1 FROM audit_logs WHERE {scope}";
        assert_eq!(
            scoped(Scope::Server, sql).0,
            "SELECT 1 FROM audit_logs WHERE organization_id IS NULL AND $1::uuid IS NULL"
        );
        assert_eq!(
            scoped(Scope::Organization(Uuid::nil()), sql).0,
            "SELECT 1 FROM audit_logs WHERE organization_id = $1"
        );
    }

    #[test]
    fn sequence_lags_when_never_issued_or_behind_the_rows() {
        let sequences = |issued, highest| Sequences {
            moved: None,
            highest,
            issued,
        };
        assert!(sequences(None, Some(3)).lagged());
        assert!(sequences(Some(2), Some(3)).lagged());
        assert!(!sequences(Some(3), Some(3)).lagged());
        assert!(!sequences(None, None).lagged());
    }
}
