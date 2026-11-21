use chrono::{DateTime, Utc};
use sqlx::{AssertSqlSafe, Connection, Executor, PgConnection, PgPool, Postgres};
use uuid::Uuid;

use super::lock::Lock;
use super::position::Position;
use super::predicate;
use super::scope::Scope;
use crate::db::DbResult;

pub const REBASE_LOCK: i64 = 0x7A6F_6E65_4155_4454;
/// Counts shown to an admin stop here, so reading them costs the same however far behind a
/// destination or the retention sweep is.
pub const COUNT_CAP: i64 = 10_000;

const REBASE_ATTEMPTS: u32 = 3;
const SERIALIZATION_FAILURE: &str = "40001";
const SCOPE: &str = "{scope}";
const DESTINATIONS: &str = "{destinations}";
const COLUMN: &str = "organization_id";
const DESTINATION_COLUMN: &str = "destination.organization_id";

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

const COLLIDED: &str = "SELECT EXISTS (SELECT 1 FROM audit_logs GROUP BY sequence \
     HAVING count(*) > 1 AND bool_or(transaction_id > 0 AND transaction_id < $1))";

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

/// `count` stops at [`COUNT_CAP`]; `capped` says there are more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backlog {
    pub count: i64,
    pub capped: bool,
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

/// Each count stops at [`COUNT_CAP`]; its `_capped` flag says there are more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expired {
    pub total: i64,
    pub total_capped: bool,
    pub held: i64,
    pub held_capped: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct Pending {
    count: i64,
    oldest: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
struct Counted {
    total: i64,
    held: i64,
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
    #[sqlx(skip)]
    collided: bool,
}

impl Sequences {
    /// The highest stored sequence, when the identity has not yet issued past it.
    fn behind(&self) -> Option<i64> {
        self.highest
            .filter(|highest| self.issued.is_none_or(|issued| issued < *highest))
    }

    /// Whether rows written on this server may carry sequences restored rows already use: the
    /// identity is behind, or it already issued past the restored rows and a row shares its
    /// sequence with another.
    fn lagged(&self) -> bool {
        self.behind().is_some() || self.collided
    }
}

fn scoped(scope: Scope, sql: &'static str) -> AssertSqlSafe<String> {
    AssertSqlSafe(
        sql.replace(SCOPE, &predicate::scoped(COLUMN, scope, 1))
            .replace(
                DESTINATIONS,
                &predicate::scoped(DESTINATION_COLUMN, scope, 1),
            ),
    )
}

fn capped(counted: i64) -> (i64, bool) {
    (counted.min(COUNT_CAP), counted > COUNT_CAP)
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

/// The final rows of `scope` after `after` and up to `through`, in position order, read from the
/// scope's position index. It is planned for its parameters on each call, as are [`backlog`] and
/// [`expired`]: a cached generic plan reads every scope's rows in transaction order instead. A row
/// is final once no transaction that could still commit behind it is open. Run on the pool, or in
/// a transaction that has not written, because a transaction's own xid holds the horizon back.
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
    .persistent(false)
    .bind(scope.organization_id())
    .bind(after.transaction_id)
    .bind(after.sequence)
    .bind(through.map(|through| through.transaction_id))
    .bind(through.map(|through| through.sequence))
    .bind(limit)
    .fetch_all(executor)
    .await
}

/// The committed rows of `scope` after `after`, final or not, counted up to [`COUNT_CAP`], and
/// when the next of them in position order was written. Both read the scope's position index
/// from `after`, in both scopes, so the cost does not grow with the backlog. Run on the pool, or
/// in a transaction that has not written.
pub async fn backlog<'e, E>(executor: E, scope: Scope, after: Position) -> DbResult<Backlog>
where
    E: Executor<'e, Database = Postgres>,
{
    let pending = sqlx::query_as::<_, Pending>(scoped(
        scope,
        "SELECT (SELECT count(*) FROM (SELECT 1 FROM audit_logs \
         WHERE {scope} AND (transaction_id, sequence) > ($2, $3) LIMIT $4) pending) AS count, \
         (SELECT created_at FROM audit_logs \
         WHERE {scope} AND (transaction_id, sequence) > ($2, $3) \
         ORDER BY transaction_id, sequence LIMIT 1) AS oldest",
    ))
    .persistent(false)
    .bind(scope.organization_id())
    .bind(after.transaction_id)
    .bind(after.sequence)
    .bind(COUNT_CAP + 1)
    .fetch_one(executor)
    .await?;
    let (count, capped) = capped(pending.count);
    Ok(Backlog {
        count,
        capped,
        oldest: pending.oldest,
    })
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

/// Deletes up to `limit` final rows of `scope` written before `before` that every enabled
/// destination of `scope` has delivered. The floor of those destinations is read by the same
/// statement, so both see one snapshot. Every row it deletes is below that snapshot's horizon,
/// so at or below the start of any destination created after it. Run on the pool, or in a
/// transaction that has not written, because a transaction's own xid holds the horizon back.
pub async fn prune<'e, E>(
    executor: E,
    scope: Scope,
    before: DateTime<Utc>,
    limit: i64,
) -> DbResult<u64>
where
    E: Executor<'e, Database = Postgres>,
{
    let result = sqlx::query(scoped(
        scope,
        concat!(
            "WITH floor AS (SELECT delivery.transaction_id, delivery.sequence \
             FROM audit_deliveries delivery \
             JOIN audit_destinations destination ON destination.id = delivery.destination_id \
             WHERE destination.enabled AND {destinations} \
             ORDER BY delivery.transaction_id, delivery.sequence LIMIT 1) \
             DELETE FROM audit_logs WHERE id IN (SELECT id FROM audit_logs WHERE {scope} \
             AND created_at < $2 AND transaction_id < ",
            xmin!(),
            " AND (transaction_id, sequence) <= ( \
             COALESCE((SELECT transaction_id FROM floor), $4), \
             COALESCE((SELECT sequence FROM floor), $4)) \
             LIMIT $3 FOR UPDATE SKIP LOCKED)"
        ),
    ))
    .bind(scope.organization_id())
    .bind(before)
    .bind(limit)
    .bind(i64::MAX)
    .execute(executor)
    .await?;
    Ok(result.rows_affected())
}

/// How many rows of `scope` were written before `before`, and how many of those sit above
/// `floor`, still owed to a destination, each counted up to [`COUNT_CAP`]. The held rows are
/// counted among the first rows after `floor` in the scope's position index, in both scopes, so
/// the cost does not grow with the slowest destination's backlog.
pub async fn expired<'e, E>(
    executor: E,
    scope: Scope,
    before: DateTime<Utc>,
    floor: Option<Position>,
) -> DbResult<Expired>
where
    E: Executor<'e, Database = Postgres>,
{
    let counted = sqlx::query_as::<_, Counted>(scoped(
        scope,
        "SELECT (SELECT count(*) FROM (SELECT 1 FROM audit_logs \
         WHERE {scope} AND created_at < $2 LIMIT $5) expired) AS total, \
         (SELECT count(*) FILTER (WHERE created_at < $2) FROM (SELECT created_at FROM audit_logs \
         WHERE {scope} AND (transaction_id, sequence) > ($3, $4) \
         ORDER BY transaction_id, sequence LIMIT $5) pending) AS held",
    ))
    .persistent(false)
    .bind(scope.organization_id())
    .bind(before)
    .bind(floor.map(|floor| floor.transaction_id))
    .bind(floor.map(|floor| floor.sequence))
    .bind(COUNT_CAP + 1)
    .fetch_one(executor)
    .await?;
    let (total, total_capped) = capped(counted.total);
    let (held, held_capped) = capped(counted.held);
    Ok(Expired {
        total,
        total_capped,
        held,
        held_capped,
    })
}

/// Moves every position below the transaction horizon when a logical restore left rows or
/// cursors at or above it, where a new transaction would sort behind them. Undelivered rows stay
/// after their destination's cursor, at the cost of sending some rows twice. Returns whether it
/// rebased. The rebase holds [`REBASE_LOCK`] from before its snapshot is taken, so it never
/// works from a snapshot older than a rebase or a retention batch that finished while it waited.
pub async fn rebase(pool: &PgPool) -> DbResult<bool> {
    if !detected(pool).await? {
        return Ok(false);
    }
    locked(Lock::acquire(pool, REBASE_LOCK).await?).await
}

/// Like [`rebase`], but never waits for [`REBASE_LOCK`]: None while another session holds it,
/// for a rebase or a retention batch in flight.
pub async fn try_rebase(pool: &PgPool) -> DbResult<Option<bool>> {
    if !detected(pool).await? {
        return Ok(Some(false));
    }
    let Some(lock) = Lock::attempt(pool, REBASE_LOCK).await? else {
        return Ok(None);
    };
    locked(lock).await.map(Some)
}

async fn locked(mut lock: Lock) -> DbResult<bool> {
    let outcome = retried(lock.connection()).await;
    let released = lock.release().await;
    let rebased = outcome?;
    released?;
    Ok(rebased)
}

/// Delivery workers move cursors without the lock, so the rebase's update of a cursor can still
/// meet a serialization failure.
async fn retried(connection: &mut PgConnection) -> DbResult<bool> {
    let mut attempt = 1;
    loop {
        match rebased(&mut *connection).await {
            Err(error) if attempt < REBASE_ATTEMPTS && serialization_failure(&error) => {
                attempt += 1;
            }
            outcome => return outcome,
        }
    }
}

/// Whether a row below `horizon` shares its sequence with another row. Grouping by sequence reads
/// the log once, where any join leaves the planner free to compare a restored log row against row.
pub async fn collided<'e, E>(executor: E, horizon: i64) -> DbResult<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query_scalar(COLLIDED)
        .bind(horizon)
        .fetch_one(executor)
        .await
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

async fn rebased(connection: &mut PgConnection) -> DbResult<bool> {
    // Repeatable read keeps a row that commits mid-rebase out of every statement here, so it
    // keeps its own transaction id and sorts after every rebased cursor.
    let mut transaction = connection
        .begin_with("BEGIN ISOLATION LEVEL REPEATABLE READ")
        .await?;
    if !detected(&mut *transaction).await? {
        transaction.commit().await?;
        return Ok(false);
    }
    let horizon: i64 = sqlx::query_scalar(concat!("SELECT ", xmax!()))
        .fetch_one(&mut *transaction)
        .await?;
    let cursors = sqlx::query_as::<_, Cursor>(
        "SELECT delivery.destination_id, destination.organization_id, delivery.transaction_id, \
         delivery.sequence FROM audit_deliveries delivery \
         JOIN audit_destinations destination ON destination.id = delivery.destination_id \
         FOR UPDATE OF delivery",
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
    let sequences = Sequences {
        collided: collided(&mut *transaction, horizon).await?,
        ..sequences
    };
    let lagged = sequences.lagged();
    let renumbered = if lagged {
        renumber(&mut transaction, horizon, sequences.behind()).await?
    } else {
        0
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
        collided = sequences.collided,
        "Audit positions rebased after a logical restore; undelivered events will be sent again"
    );
    Ok(true)
}

/// Gives every row below `horizon` a new sequence from the identity, first moving the identity
/// past `behind` when it has not issued that far. An identity already past every stored row is
/// left alone: moving it back could hand out a sequence a row committing now already holds.
async fn renumber(
    connection: &mut PgConnection,
    horizon: i64,
    behind: Option<i64>,
) -> DbResult<u64> {
    if let Some(highest) = behind {
        sqlx::query("SELECT setval(pg_get_serial_sequence('public.audit_logs', 'sequence'), $1)")
            .bind(highest)
            .execute(&mut *connection)
            .await?;
    }
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

    fn sequences(issued: Option<i64>, highest: Option<i64>, collided: bool) -> Sequences {
        Sequences {
            moved: None,
            highest,
            issued,
            collided,
        }
    }

    #[test]
    fn sequence_lags_when_never_issued_or_behind_the_rows() {
        assert!(sequences(None, Some(3), false).lagged());
        assert!(sequences(Some(2), Some(3), false).lagged());
        assert!(!sequences(Some(3), Some(3), false).lagged());
        assert!(!sequences(None, None, false).lagged());
    }

    #[test]
    fn sequence_lags_when_a_row_shares_its_sequence_even_once_the_identity_caught_up() {
        assert!(sequences(Some(9), Some(9), true).lagged());
        assert_eq!(sequences(Some(9), Some(9), true).behind(), None);
    }

    #[test]
    fn the_identity_moves_only_when_it_is_behind_the_rows() {
        assert_eq!(sequences(Some(2), Some(3), false).behind(), Some(3));
        assert_eq!(sequences(None, Some(3), false).behind(), Some(3));
        assert_eq!(sequences(Some(4), Some(3), true).behind(), None);
    }

    #[test]
    fn a_count_past_the_cap_reports_the_cap_and_that_there_are_more() {
        assert_eq!(capped(0), (0, false));
        assert_eq!(capped(COUNT_CAP), (COUNT_CAP, false));
        assert_eq!(capped(COUNT_CAP + 1), (COUNT_CAP, true));
    }

    #[test]
    fn the_destination_predicate_binds_the_same_organization_as_the_rows() {
        let sql = "SELECT 1 FROM audit_logs WHERE {scope} AND EXISTS (SELECT 1 FROM \
                   audit_destinations destination WHERE {destinations})";
        assert_eq!(
            scoped(Scope::Organization(Uuid::nil()), sql).0,
            "SELECT 1 FROM audit_logs WHERE organization_id = $1 AND EXISTS (SELECT 1 FROM \
             audit_destinations destination WHERE destination.organization_id = $1)"
        );
    }
}
