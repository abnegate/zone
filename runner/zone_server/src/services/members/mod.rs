mod admission_error;
mod cap;
mod deprovision;
mod deprovisioned;
mod halt;
mod halted;

use std::time::Duration;

use chrono::Utc;
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;
use zone_enterprise::{Members, period_start};

use crate::db::seats;
use crate::state::AppState;

pub use admission_error::AdmissionError;
pub use cap::Cap;
pub use deprovision::deprovision;
pub use deprovisioned::Deprovisioned;
pub use halt::halt;
pub use halted::Halted;

/// The transaction-scoped advisory lock ("zoneSEAT") every capped admission holds from its count
/// until its membership commits.
pub const SEATS_LOCK: i64 = 0x7A6F_6E65_5345_4154;

/// How long an admission waits for [`SEATS_LOCK`] unless configured otherwise.
pub const ADMISSION_WAIT: Duration = Duration::from_secs(5);

const LIMIT_WAIT: &str = "SELECT set_config('lock_timeout', $1, true)";
const LOCK: &str = "SELECT pg_advisory_xact_lock($1)";
const RESTORE_WAIT: &str = "SET LOCAL lock_timeout = DEFAULT";
const SHORTEST_WAIT_MILLISECONDS: u128 = 1;

/// The cap [`check`], [`admit`] and [`reactivate`] hold the server to. Reading it may load the
/// license through the pool, so read it before beginning the admitting transaction.
pub async fn cap(state: &AppState) -> Result<Cap, AdmissionError> {
    let wait = state.config().admission_wait;
    #[cfg(any(test, feature = "testing"))]
    if !state.members_capped() {
        return Ok(Cap { limit: None, wait });
    }
    let license = state.settled_license().await?;
    Ok(Cap {
        limit: Members::cap(license.as_ref(), Utc::now()),
        wait,
    })
}

/// Whether [`admit`] would let `user`, or a person without an account when `None`, in right now.
/// Takes no lock, so it only spares a refusal the work that leads up to the authoritative
/// [`admit`].
pub async fn check(state: &AppState, cap: Cap, user: Option<Uuid>) -> Result<(), AdmissionError> {
    let Some(limit) = cap.limit else {
        return Ok(());
    };
    let mut connection = state.db().acquire().await?;
    room(&mut connection, limit, user).await
}

/// Admits `user`, or a person without an account when `None`, unless the server is capped and
/// full. An existing member is always admitted, however far over the cap the server is.
///
/// `transaction` must be the one that then inserts the membership: the count holds
/// [`SEATS_LOCK`] until it commits. Holding that lock, the transaction must take no other pool
/// connection, and must take no row lock before calling this.
pub async fn admit(
    transaction: &mut Transaction<'_, Postgres>,
    cap: Cap,
    user: Option<Uuid>,
) -> Result<(), AdmissionError> {
    let Some(limit) = cap.limit else {
        return Ok(());
    };
    let connection = &mut **transaction;
    lock(connection, cap.wait).await?;
    room(connection, limit, user).await
}

/// Admits everyone that activating `organization` would make a member again, holding
/// [`SEATS_LOCK`] as [`admit`] does.
pub async fn reactivate(
    transaction: &mut Transaction<'_, Postgres>,
    cap: Cap,
    organization: Uuid,
) -> Result<(), AdmissionError> {
    let Some(limit) = cap.limit else {
        return Ok(());
    };
    let connection = &mut **transaction;
    lock(connection, cap.wait).await?;
    match seats::returning(&mut *connection, organization).await? {
        0 => Ok(()),
        returning => make_room(connection, limit, returning).await,
    }
}

async fn lock(connection: &mut PgConnection, wait: Duration) -> Result<(), AdmissionError> {
    sqlx::query(LIMIT_WAIT)
        .bind(lock_timeout(wait))
        .execute(&mut *connection)
        .await?;
    sqlx::query(LOCK)
        .bind(SEATS_LOCK)
        .execute(&mut *connection)
        .await
        .map_err(AdmissionError::locking)?;
    sqlx::query(RESTORE_WAIT).execute(&mut *connection).await?;
    Ok(())
}

/// `wait` as a `lock_timeout`, never zero, which would wait forever.
fn lock_timeout(wait: Duration) -> String {
    format!("{}ms", wait.as_millis().max(SHORTEST_WAIT_MILLISECONDS))
}

async fn room(
    connection: &mut PgConnection,
    limit: u32,
    user: Option<Uuid>,
) -> Result<(), AdmissionError> {
    if let Some(user) = user
        && seats::member(&mut *connection, user).await?
    {
        return Ok(());
    }
    make_room(connection, limit, 1).await
}

async fn make_room(
    connection: &mut PgConnection,
    limit: u32,
    joining: u32,
) -> Result<(), AdmissionError> {
    let active = seats::used(&mut *connection, period_start(Utc::now())).await?;
    if active.saturating_add(joining) > limit {
        return Err(AdmissionError::Full { cap: limit });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
