//! The retention period of a scope's audit log, and the sweep that applies it. A row past the
//! period is deleted only once every enabled destination of its scope has delivered it.

use chrono::{DateTime, Utc};
use serde_json::{Number, json};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;
use zone_enterprise::audit::retention::{cutoff, wait};
use zone_enterprise::audit::{Error, Standing, Wait};

use super::standing;
use crate::db::DbResult;
use crate::db::audit::retentions::{self, RetentionRecord};
use crate::db::audit::scope::Scope;
use crate::db::audit::{AuditContext, actions, deliveries, events, log_action, resources};
use crate::state::AppState;

pub const SWEEP_INTERVAL_SECONDS: u64 = 60 * 60;
pub const BATCH: i64 = 5_000;

/// Keeps a delivery-tick rebase from renumbering positions between reading the floor and
/// deleting at or below it.
const HOLD_POSITIONS: &str = "SELECT pg_advisory_xact_lock_shared($1)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub destination_id: Uuid,
    pub name: String,
    pub wait: Wait,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub days: Option<i32>,
    pub cutoff: Option<DateTime<Utc>>,
    pub expired: i64,
    pub held: i64,
    pub holders: Vec<Holder>,
    pub swept_at: Option<DateTime<Utc>>,
    pub deleted_count: i64,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub previous: Option<i32>,
    pub next: Option<i32>,
}

impl Change {
    pub fn changed(self) -> bool {
        self.previous != self.next
    }
}

/// A period of whole days from 1 to `i32::MAX`; none keeps audit events forever.
pub fn period(days: Option<&Number>) -> Result<Option<i32>, Error> {
    days.map(|days| {
        days.as_i64()
            .and_then(|days| i32::try_from(days).ok())
            .filter(|days| *days > 0)
            .ok_or(Error::RetentionInvalid)
    })
    .transpose()
}

pub async fn status(
    pool: &PgPool,
    scope: Scope,
    standing: &Standing,
    now: DateTime<Utc>,
) -> DbResult<Status> {
    let Some(record) = retentions::get(pool, scope).await? else {
        return Ok(Status::default());
    };
    let before = cutoff(now, record.days.unsigned_abs());
    let floor = deliveries::floor(pool, scope).await?;
    let expired = events::expired(pool, scope, before, floor).await?;
    let holders = deliveries::holding(pool, scope, before)
        .await?
        .into_iter()
        .map(|holding| Holder {
            destination_id: holding.destination_id,
            name: holding.name,
            wait: wait(standing.pause, holding.attempts.unsigned_abs()),
        })
        .collect();
    Ok(Status {
        days: Some(record.days),
        cutoff: Some(before),
        expired: expired.total,
        held: expired.held,
        holders,
        swept_at: record.swept_at,
        deleted_count: record.deleted_count,
        updated_at: Some(record.updated_at),
    })
}

pub async fn update(
    connection: &mut PgConnection,
    scope: Scope,
    days: Option<i32>,
    user: Uuid,
) -> DbResult<Change> {
    let previous = retentions::get(&mut *connection, scope)
        .await?
        .map(|record| record.days);
    let change = Change {
        previous,
        next: days,
    };
    if !change.changed() {
        return Ok(change);
    }
    match days {
        Some(days) => {
            retentions::set(&mut *connection, scope, days, user).await?;
        }
        None => {
            retentions::clear(&mut *connection, scope).await?;
        }
    }
    Ok(change)
}

/// Rebases positions first, so the floor is never compared across a logical restore, and
/// returns how many rows it deleted. A configured period keeps applying under any installed
/// license, lapsed or not, and under an unverified key; only removing the license stops it.
/// A scope that fails is logged and the others are still swept; the first failure is returned.
pub async fn sweep(state: &AppState) -> Result<u64, sqlx::Error> {
    let pool = state.db();
    events::rebase(pool).await?;
    let standing = standing(state).await?;
    if !standing.applies {
        return Ok(0);
    }
    let mut deleted = 0;
    let mut failure = None;
    for record in retentions::all(pool).await? {
        match apply(pool, &record, Utc::now()).await {
            Ok(count) => deleted += count,
            Err(error) => {
                tracing::error!(
                    %error,
                    organization_id = ?record.organization_id,
                    "Could not apply the audit retention period"
                );
                failure.get_or_insert(error);
            }
        }
    }
    failure.map_or(Ok(deleted), Err)
}

async fn apply(pool: &PgPool, record: &RetentionRecord, now: DateTime<Utc>) -> DbResult<u64> {
    let scope = record.scope();
    let before = cutoff(now, record.days.unsigned_abs());
    let mut deleted = 0;
    loop {
        let pruned = prune(pool, scope, before).await?;
        deleted += pruned;
        if pruned < BATCH.unsigned_abs() {
            break;
        }
    }
    let count = i64::try_from(deleted).unwrap_or(i64::MAX);
    retentions::swept(pool, scope, count).await?;
    if deleted > 0 {
        let context = AuditContext {
            org_id: scope.organization_id(),
            workspace_id: None,
            actor_id: None,
            actor_email: None,
            ip_address: None,
            user_agent: None,
        };
        log_action(
            pool,
            &context,
            actions::AUDIT_RETENTION_APPLIED,
            resources::AUDIT_RETENTION,
            scope.organization_id(),
            None,
            Some(json!({ "days": record.days, "cutoff": before, "deleted_count": count })),
        )
        .await?;
    }
    Ok(deleted)
}

async fn prune(pool: &PgPool, scope: Scope, before: DateTime<Utc>) -> DbResult<u64> {
    let mut transaction = pool.begin().await?;
    sqlx::query(HOLD_POSITIONS)
        .bind(events::REBASE_LOCK)
        .execute(&mut *transaction)
        .await?;
    let floor = deliveries::floor(&mut *transaction, scope).await?;
    let pruned = events::prune(&mut *transaction, scope, before, floor, BATCH).await?;
    transaction.commit().await?;
    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(value: serde_json::Value) -> Number {
        match value {
            serde_json::Value::Number(number) => number,
            other => panic!("{other} is not a number"),
        }
    }

    #[test]
    fn no_period_keeps_events_forever() {
        assert_eq!(period(None), Ok(None));
    }

    #[test]
    fn a_period_is_whole_days_from_one_to_the_column_maximum() {
        assert_eq!(period(Some(&number(json!(1)))), Ok(Some(1)));
        assert_eq!(period(Some(&number(json!(i32::MAX)))), Ok(Some(i32::MAX)));
    }

    #[test]
    fn a_period_outside_the_range_or_fractional_is_invalid() {
        for value in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(i64::from(i32::MAX) + 1),
            json!(u64::MAX),
        ] {
            assert_eq!(
                period(Some(&number(value.clone()))),
                Err(Error::RetentionInvalid),
                "{value}"
            );
        }
    }

    #[test]
    fn a_change_is_a_different_period() {
        assert!(
            Change {
                previous: None,
                next: Some(30)
            }
            .changed()
        );
        assert!(
            Change {
                previous: Some(30),
                next: None
            }
            .changed()
        );
        assert!(
            !Change {
                previous: Some(30),
                next: Some(30)
            }
            .changed()
        );
    }
}
