//! A digest a workspace is owed, claimed in Postgres before it is sent.

use chrono::NaiveDateTime;
use sqlx::PgPool;
use uuid::Uuid;

use super::schedule::{Due, Schedule, due};
use crate::db::DbResult;
use crate::db::report_deliveries;

/// What one workspace is owed, and the slot it was last sent, which a claim compares against.
///
/// Claiming moves the stored slot forward only if nobody moved it first, so of two replicas
/// owing the same digest exactly one sends it. A process that dies between claiming and
/// sending loses that digest rather than sending it twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    workspace_id: Uuid,
    last: NaiveDateTime,
    due: Due,
}

impl Claim {
    /// What `workspace_id` is owed at `now`. A workspace seen for the first time is seeded with
    /// the slot that has already gone by, so it waits for the next one.
    pub async fn owed(
        pool: &PgPool,
        workspace_id: Uuid,
        schedule: &Schedule,
        now: NaiveDateTime,
    ) -> DbResult<Option<Self>> {
        let last =
            report_deliveries::last(pool, workspace_id, schedule.slot_at_or_before(now)).await?;
        Ok(due(schedule, Some(last), now).map(|due| Self {
            workspace_id,
            last,
            due,
        }))
    }

    pub fn workspace_id(&self) -> Uuid {
        self.workspace_id
    }

    pub fn last(&self) -> NaiveDateTime {
        self.last
    }

    pub fn due(&self) -> &Due {
        &self.due
    }

    /// Record the digest as sent, and say whether this caller may send it.
    pub async fn take(&self, pool: &PgPool) -> DbResult<bool> {
        report_deliveries::advance(pool, self.workspace_id, self.last, self.due.slot).await
    }

    /// Give back a taken claim whose delivery may succeed on a later tick.
    pub async fn release(&self, pool: &PgPool) -> DbResult<bool> {
        report_deliveries::advance(pool, self.workspace_id, self.due.slot, self.last).await
    }
}
