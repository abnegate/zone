use std::time::Duration;

use abnegate_http::Backoff;
use async_trait::async_trait;
use chrono::Utc;
use sqlx::PgPool;
use tokio::time::Instant;
use uuid::Uuid;
use zone_enterprise::audit::delivery::{RETRY_BASE, RETRY_FACTOR, RETRY_MAXIMUM, S3_FLUSH_AGE};
use zone_enterprise::audit::{FailureCode, Kind, Pause, Standing};

use super::events::event;
use super::transport::{Batch, Failure, Transport};
use crate::db::audit::deliveries::{self, Lease, Resume};
use crate::db::audit::destinations::{self, DestinationRecord};
use crate::db::audit::events;
use crate::db::audit::position::Position;
use crate::state::AppState;

const TICK: Duration = Duration::from_secs(5);
const LEASE: Duration = Duration::from_secs(2 * 60);
const CONCURRENCY: usize = 8;
const BACKOFF: Backoff = Backoff::new(
    RETRY_BASE,
    RETRY_FACTOR,
    RETRY_MAXIMUM,
    Backoff::DEFAULT_JITTER,
);
const UNKNOWN_KIND: &str = "The destination's kind is not one this server can deliver to";
const OUTLASTED: &str = "The send ran into the end of its lease and was abandoned";

#[async_trait]
pub trait Open: Send + Sync {
    async fn open(&self, destination: &DestinationRecord) -> Result<Box<dyn Transport>, Failure>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    pub tick: Duration,
    pub lease: Duration,
    pub concurrency: usize,
}

impl Default for Cadence {
    fn default() -> Self {
        Self {
            tick: TICK,
            lease: LEASE,
            concurrency: CONCURRENCY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Delivered(usize),
    Waiting(Uuid),
    Failed(FailureCode),
    Paused(Pause),
    Idle(Uuid),
    Lost,
    Unclaimed,
}

struct Claim<'a> {
    pool: &'a PgPool,
    lease: Lease,
    cadence: Cadence,
    deadline: Instant,
}

/// Claims one due destination and sends it the next batch of final events after its cursor.
///
/// Rows are read on the pool rather than in a transaction: a transaction that has written holds
/// the snapshot horizon back behind its own xid, and nothing here holds a transaction open across
/// a send. A standing pause claims nothing, so the backlog stays where it is until the license
/// allows streaming again. The send is abandoned a tick before the lease runs out, before another
/// worker can claim the same batch. Every due time is computed on the database's clock, the
/// clock claims are judged by, so a server whose clock lags never releases a destination already
/// due again.
pub async fn deliver(
    state: &AppState,
    cadence: Cadence,
    open: &dyn Open,
) -> Result<Outcome, sqlx::Error> {
    let stored = state.stored_license().await?;
    let standing = Standing::of(stored.license(), stored.refusal().is_some(), Utc::now());
    if let Some(pause) = standing.pause {
        return Ok(Outcome::Paused(pause));
    }
    let pool = state.db();
    let deadline = Instant::now() + budget(cadence);
    let Some(lease) = deliveries::claim(pool, cadence.lease).await? else {
        return Ok(Outcome::Unclaimed);
    };
    let claim = Claim {
        pool,
        lease,
        cadence,
        deadline,
    };
    let Some(destination) = destinations::find(pool, claim.id()).await? else {
        return Ok(Outcome::Lost);
    };
    if !destination.enabled {
        return claim
            .release(Resume::After(cadence.tick), Outcome::Idle(claim.id()))
            .await;
    }
    let Some(kind) = Kind::parse(&destination.kind) else {
        return claim
            .fail(Failure::new(FailureCode::Configuration, UNKNOWN_KIND))
            .await;
    };
    claim.send(&destination, kind, open).await
}

impl Claim<'_> {
    async fn send(
        &self,
        destination: &DestinationRecord,
        kind: Kind,
        open: &dyn Open,
    ) -> Result<Outcome, sqlx::Error> {
        let scope = destination.scope();
        let delivery = &self.lease.delivery;
        let delivered = delivery.delivered;
        let rows = match delivery.batch {
            Some(pinned) => events::after(self.pool, scope, delivered, Some(pinned), i64::MAX),
            None => events::after(self.pool, scope, delivered, None, limit(kind)),
        }
        .await?;
        let now = self.lease.claimed_at;
        let through = match (delivery.batch, rows.last()) {
            (Some(pinned), _) => pinned,
            (None, None) => {
                return self
                    .release(Resume::After(self.cadence.tick), Outcome::Idle(self.id()))
                    .await;
            }
            (None, Some(last)) => {
                let oldest = rows.first().map(|row| row.created_at);
                if !kind.ready(rows.len(), oldest, now) {
                    let flush = oldest.map_or(now, |oldest| oldest + S3_FLUSH_AGE);
                    return self
                        .release(Resume::At(flush), Outcome::Waiting(self.id()))
                        .await;
                }
                let through = last.position();
                if !deliveries::pin(self.pool, self.id(), self.lease.token, through).await? {
                    return Ok(self.lost());
                }
                through
            }
        };
        if rows.is_empty() {
            return self.advance(through, 0).await;
        }
        let count = rows.len();
        let batch = Batch {
            destination_id: destination.id,
            scope,
            events: rows.into_iter().map(event).collect(),
        };
        let sent = tokio::time::timeout_at(self.deadline, transmit(destination, &batch, open))
            .await
            .unwrap_or_else(|_| Err(Failure::new(FailureCode::Timeout, OUTLASTED)));
        match sent {
            Ok(()) => self.advance(through, count).await,
            Err(failure) => self.fail(failure).await,
        }
    }

    fn id(&self) -> Uuid {
        self.lease.delivery.destination_id
    }

    async fn advance(&self, through: Position, count: usize) -> Result<Outcome, sqlx::Error> {
        let counted = i64::try_from(count).unwrap_or(i64::MAX);
        if !deliveries::advance(self.pool, self.id(), self.lease.token, through, counted).await? {
            return Ok(self.lost());
        }
        tracing::debug!(destination_id = %self.id(), count, "Audit events delivered");
        Ok(Outcome::Delivered(count))
    }

    async fn release(&self, resume: Resume, outcome: Outcome) -> Result<Outcome, sqlx::Error> {
        if !deliveries::release(self.pool, self.id(), self.lease.token, resume).await? {
            return Ok(self.lost());
        }
        Ok(outcome)
    }

    async fn fail(&self, failure: Failure) -> Result<Outcome, sqlx::Error> {
        let attempts = self.lease.delivery.attempts;
        let failed = deliveries::fail(
            self.pool,
            self.id(),
            self.lease.token,
            failure.code,
            &failure.message,
            delay(attempts, failure.retry_after),
        )
        .await?;
        let Some(retry_at) = failed else {
            return Ok(self.lost());
        };
        tracing::warn!(
            destination_id = %self.id(),
            code = %failure.code,
            message = %failure.message,
            attempts = attempts.saturating_add(1),
            %retry_at,
            "Audit delivery failed; the batch is kept and retried with backoff"
        );
        Ok(Outcome::Failed(failure.code))
    }

    fn lost(&self) -> Outcome {
        tracing::warn!(
            destination_id = %self.id(),
            token = %self.lease.token,
            "An audit delivery lost its lease; its result is discarded and the next claim resends the batch"
        );
        Outcome::Lost
    }
}

async fn transmit(
    destination: &DestinationRecord,
    batch: &Batch,
    open: &dyn Open,
) -> Result<(), Failure> {
    open.open(destination).await?.deliver(batch).await
}

fn limit(kind: Kind) -> i64 {
    i64::try_from(kind.batch_size()).unwrap_or(i64::MAX)
}

fn delay(attempts: i32, retry_after: Option<Duration>) -> Duration {
    let attempt = u32::try_from(attempts).unwrap_or_default();
    wait(BACKOFF.delay(attempt), retry_after)
}

fn wait(backoff: Duration, retry_after: Option<Duration>) -> Duration {
    retry_after
        .map_or(backoff, |after| after.max(backoff))
        .min(RETRY_MAXIMUM)
}

fn budget(cadence: Cadence) -> Duration {
    cadence
        .lease
        .checked_sub(cadence.tick)
        .filter(|budget| !budget.is_zero())
        .unwrap_or(cadence.lease / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_defaults_to_five_seconds_two_minutes_and_eight() {
        assert_eq!(
            Cadence::default(),
            Cadence {
                tick: Duration::from_secs(5),
                lease: Duration::from_secs(120),
                concurrency: 8,
            }
        );
    }

    #[test]
    fn retry_after_is_honoured_when_longer_than_the_backoff() {
        let backoff = Duration::from_secs(5);
        assert_eq!(wait(backoff, None), backoff);
        assert_eq!(wait(backoff, Some(Duration::from_secs(1))), backoff);
        assert_eq!(
            wait(backoff, Some(Duration::from_secs(60))),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn retry_after_is_capped_at_the_maximum() {
        assert_eq!(
            wait(Duration::from_secs(5), Some(Duration::from_secs(86_400))),
            RETRY_MAXIMUM
        );
    }

    #[test]
    fn backoff_starts_at_the_base_and_never_exceeds_the_maximum() {
        let first = delay(0, None);
        assert!(first <= RETRY_BASE, "{first:?}");
        assert!(first >= RETRY_BASE.mul_f64(0.75), "{first:?}");
        for attempts in [10, 30, i32::MAX, -1] {
            assert!(delay(attempts, None) <= RETRY_MAXIMUM, "{attempts}");
        }
    }

    #[test]
    fn a_send_stops_a_tick_before_its_lease_runs_out() {
        assert_eq!(budget(Cadence::default()), Duration::from_secs(115));
        let cadence = Cadence {
            tick: Duration::from_millis(50),
            lease: Duration::from_secs(30),
            concurrency: 1,
        };
        assert_eq!(budget(cadence), Duration::from_millis(29_950));
    }

    #[test]
    fn a_tick_as_long_as_the_lease_still_leaves_the_send_some_of_it() {
        for tick in [Duration::from_secs(30), Duration::from_secs(60)] {
            let cadence = Cadence {
                tick,
                lease: Duration::from_secs(30),
                concurrency: 1,
            };
            assert_eq!(budget(cadence), Duration::from_secs(15), "{tick:?}");
        }
    }
}
