mod admission_error;

use chrono::Utc;
use sqlx::{Acquire, PgConnection, Postgres};
use uuid::Uuid;
use zone_enterprise::{Members, period_start};

use crate::db::seats;
use crate::state::AppState;

pub use admission_error::{AdmissionError, MEMBER_ADMISSION_BUSY};

/// The transaction-scoped advisory lock ("zoneSEAT") every capped admission holds from its count
/// until its membership commits.
pub const SEATS_LOCK: i64 = 0x7A6F_6E65_5345_4154;

const LIMIT_WAIT: &str = "SET LOCAL lock_timeout = '5s'";
const LOCK: &str = "SELECT pg_advisory_xact_lock($1)";
const RESTORE_WAIT: &str = "SET LOCAL lock_timeout = DEFAULT";

/// Whether [`admit`] would let `user`, or a person without an account when `None`, in right now.
/// Takes no lock, so it only spares a refusal the work that leads up to the authoritative
/// [`admit`].
pub async fn check(state: &AppState, user: Option<Uuid>) -> Result<(), AdmissionError> {
    let Some(cap) = cap(state).await? else {
        return Ok(());
    };
    let mut connection = state.db().acquire().await?;
    room(&mut connection, cap, user).await
}

/// Admits `user`, or a person without an account when `None`, unless the server is capped and
/// full. An existing member is always admitted, however far over the cap the server is.
///
/// `connection` must be the transaction that then inserts the membership: the count holds
/// [`SEATS_LOCK`] until it commits. Holding that lock, the transaction must take no other pool
/// connection, and must take no row lock before calling this.
// As an `async fn`, a handler awaiting this fails its `Send` check (launchbadge/sqlx#1015).
#[allow(clippy::manual_async_fn)]
pub fn admit<'a, 'c, A>(
    state: &'a AppState,
    connection: A,
    user: Option<Uuid>,
) -> impl Future<Output = Result<(), AdmissionError>> + Send + 'a
where
    'c: 'a,
    A: Acquire<'c, Database = Postgres> + Send + 'a,
{
    async move {
        let Some(cap) = cap(state).await? else {
            return Ok(());
        };
        let mut connection = connection.acquire().await?;
        lock(&mut connection).await?;
        room(&mut connection, cap, user).await
    }
}

/// Admits everyone that activating `organization` would make a member again, holding
/// [`SEATS_LOCK`] as [`admit`] does.
// As an `async fn`, a handler awaiting this fails its `Send` check (launchbadge/sqlx#1015).
#[allow(clippy::manual_async_fn)]
pub fn reactivate<'a, 'c, A>(
    state: &'a AppState,
    connection: A,
    organization: Uuid,
) -> impl Future<Output = Result<(), AdmissionError>> + Send + 'a
where
    'c: 'a,
    A: Acquire<'c, Database = Postgres> + Send + 'a,
{
    async move {
        let Some(cap) = cap(state).await? else {
            return Ok(());
        };
        let mut connection = connection.acquire().await?;
        lock(&mut connection).await?;
        match seats::returning(&mut *connection, organization).await? {
            0 => Ok(()),
            returning => make_room(&mut connection, cap, returning).await,
        }
    }
}

async fn cap(state: &AppState) -> Result<Option<u32>, AdmissionError> {
    #[cfg(any(test, feature = "testing"))]
    if !state.members_capped() {
        return Ok(None);
    }
    let license = state.settled_license().await?;
    Ok(Members::cap(license.as_ref(), Utc::now()))
}

async fn lock(connection: &mut PgConnection) -> Result<(), AdmissionError> {
    sqlx::query(LIMIT_WAIT).execute(&mut *connection).await?;
    sqlx::query(LOCK)
        .bind(SEATS_LOCK)
        .execute(&mut *connection)
        .await
        .map_err(AdmissionError::locking)?;
    sqlx::query(RESTORE_WAIT).execute(&mut *connection).await?;
    Ok(())
}

async fn room(
    connection: &mut PgConnection,
    cap: u32,
    user: Option<Uuid>,
) -> Result<(), AdmissionError> {
    if let Some(user) = user
        && seats::member(&mut *connection, user).await?
    {
        return Ok(());
    }
    make_room(connection, cap, 1).await
}

async fn make_room(
    connection: &mut PgConnection,
    cap: u32,
    joining: u32,
) -> Result<(), AdmissionError> {
    let active = seats::used(&mut *connection, period_start(Utc::now())).await?;
    if active.saturating_add(joining) > cap {
        return Err(AdmissionError::Full { cap });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
