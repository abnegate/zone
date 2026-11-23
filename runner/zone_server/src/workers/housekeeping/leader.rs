//! One leader among the replicas sharing a database, for the sweeps that must run once.

pub mod tenure;
pub mod timing;

use std::io::ErrorKind;
use std::sync::{Arc, Mutex, PoisonError};

use sqlx::postgres::PgConnectOptions;
use sqlx::{Connection, PgConnection};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, sleep_until, timeout};
use tokio_util::sync::{CancellationToken, DropGuard};

pub use tenure::Tenure;
pub use timing::Timing;

use crate::db::DbResult;

pub const LEADER_KEY: i64 = 0x7A6F_6E65_4C45_4144;

const IDLE_SESSION_TIMEOUT: &str = "SELECT set_config('idle_session_timeout', $1, false)";
const TRY_LOCK: &str = "SELECT pg_try_advisory_lock($1)";
const UNLOCK: &str = "SELECT pg_advisory_unlock($1)";
const PING: &str = "SELECT 1";

/// This process's standing in the election for [`LEADER_KEY`].
///
/// The lock is a session advisory lock on a connection of its own, opened outside the pool so
/// no pooled connection can carry it to another holder. A clone shares the same election; the
/// last clone to drop stops it, which closes that connection and so frees the lock.
#[derive(Clone)]
pub struct Leadership {
    timing: Timing,
    tenure: Arc<watch::Sender<Option<Tenure>>>,
    stop: CancellationToken,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    _guard: Arc<DropGuard>,
}

impl Leadership {
    /// Start campaigning for leadership against the database `options` reaches.
    pub fn start(options: PgConnectOptions, timing: Timing) -> Self {
        let leadership = Self::detached(timing);
        let task = tokio::spawn(campaign(
            options,
            timing,
            Arc::clone(&leadership.tenure),
            leadership.stop.clone(),
        ));
        *leadership
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(task);
        leadership
    }

    fn detached(timing: Timing) -> Self {
        let stop = CancellationToken::new();
        Self {
            timing,
            tenure: Arc::new(watch::Sender::new(None)),
            _guard: Arc::new(stop.clone().drop_guard()),
            stop,
            task: Arc::new(Mutex::new(None)),
        }
    }

    pub fn timing(&self) -> Timing {
        self.timing
    }

    pub fn tenure(&self) -> Option<Tenure> {
        *self.tenure.borrow()
    }

    /// Whether this process may start a cluster sweep now: it holds the lock, a ping it sent
    /// within `freshness` was answered, and the wait after acquiring has passed.
    pub fn leads(&self) -> bool {
        self.tenure()
            .is_some_and(|tenure| tenure.leads(&self.timing, Instant::now()))
    }

    /// When this process will lead, if it holds the lock and is still sitting out the wait.
    pub fn ready(&self) -> Option<Instant> {
        let now = Instant::now();
        self.tenure()
            .filter(|tenure| tenure.is_fresh(&self.timing, now) && now < tenure.ready(&self.timing))
            .map(|tenure| tenure.ready(&self.timing))
    }

    /// Resolves once this process no longer leads, including when its pings stop being
    /// answered while the task sending them is starved.
    pub async fn lost(&self) {
        let mut tenure = self.tenure.subscribe();
        loop {
            let current = *tenure.borrow_and_update();
            let expires = match current {
                Some(held) if held.leads(&self.timing, Instant::now()) => {
                    held.expires(&self.timing)
                }
                _ => return,
            };
            tokio::select! {
                () = sleep_until(expires) => {}
                changed = tenure.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
            }
        }
    }

    /// Stop leading at once, then stop campaigning and give the lock back if
    /// this process holds it.
    pub async fn release(&self) {
        self.stop.cancel();
        self.tenure.send_replace(None);
        let task = self
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(task) = task
            && let Err(error) = task.await
        {
            tracing::warn!(%error, "The leader election stopped abnormally");
        }
    }

    #[cfg(test)]
    pub fn manual(timing: Timing) -> Self {
        Self::detached(timing)
    }

    #[cfg(test)]
    pub fn promote(&self, sent: Instant) {
        self.tenure.send_replace(Some(Tenure::new(sent)));
    }

    #[cfg(test)]
    pub fn confirm(&self, sent: Instant) {
        self.tenure.send_if_modified(|tenure| {
            if let Some(tenure) = tenure {
                tenure.confirm(sent);
            }
            false
        });
    }

    #[cfg(test)]
    pub fn demote(&self) {
        self.tenure.send_replace(None);
    }
}

async fn campaign(
    options: PgConnectOptions,
    timing: Timing,
    tenure: Arc<watch::Sender<Option<Tenure>>>,
    stop: CancellationToken,
) {
    loop {
        let attempt = tokio::select! {
            () = stop.cancelled() => return,
            attempt = timeout(timing.timeout, acquire(&options, &timing)) => attempt,
        };
        match attempt {
            Ok(Ok(Some((connection, sent)))) => {
                tenure.send_replace(Some(Tenure::new(sent)));
                tracing::info!(
                    wait_seconds = timing.wait.as_secs(),
                    "This replica now leads the cluster housekeeping sweeps, once the wait ends"
                );
                hold(connection, &timing, &tenure, &stop).await;
                tenure.send_replace(None);
                if stop.is_cancelled() {
                    return;
                }
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => {
                tracing::warn!(
                    %error,
                    "Could not stand for housekeeping leadership, which needs Postgres 14 or later \
                     over a session-mode connection; retrying"
                );
            }
            Err(_) => tracing::warn!("Standing for housekeeping leadership timed out; retrying"),
        }
        tokio::select! {
            () = stop.cancelled() => return,
            () = sleep(timing.acquire) => {}
        }
    }
}

async fn acquire(
    options: &PgConnectOptions,
    timing: &Timing,
) -> DbResult<Option<(PgConnection, Instant)>> {
    let mut connection = PgConnection::connect_with(options).await?;
    sqlx::query(IDLE_SESSION_TIMEOUT)
        .bind(format!("{}ms", timing.idle.as_millis()))
        .execute(&mut connection)
        .await?;
    let sent = Instant::now();
    let acquired: bool = sqlx::query_scalar(TRY_LOCK)
        .bind(LEADER_KEY)
        .fetch_one(&mut connection)
        .await?;
    if acquired {
        return Ok(Some((connection, sent)));
    }
    connection.close().await?;
    Ok(None)
}

async fn hold(
    mut connection: PgConnection,
    timing: &Timing,
    tenure: &watch::Sender<Option<Tenure>>,
    stop: &CancellationToken,
) {
    loop {
        tokio::select! {
            () = stop.cancelled() => {
                unlock(connection, timing).await;
                return;
            }
            () = sleep(timing.ping) => {}
        }
        let sent = Instant::now();
        let fresh = tenure
            .borrow()
            .is_some_and(|held| held.is_fresh(timing, sent));
        if !fresh {
            tracing::warn!(
                "No leader ping was answered within the freshness window; stepping down"
            );
            return;
        }
        match timeout(timing.timeout, sqlx::query(PING).execute(&mut connection)).await {
            Ok(Ok(_)) => {
                tenure.send_if_modified(|held| {
                    if let Some(held) = held {
                        held.confirm(sent);
                    }
                    false
                });
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "The leader connection failed; stepping down");
                return;
            }
            Err(_) => {
                tracing::warn!("A leader ping went unanswered; stepping down");
                return;
            }
        }
    }
}

async fn unlock(mut connection: PgConnection, timing: &Timing) {
    let released = timeout(timing.timeout, async {
        sqlx::query(UNLOCK)
            .bind(LEADER_KEY)
            .execute(&mut connection)
            .await?;
        connection.close().await
    })
    .await
    .unwrap_or_else(|_| Err(std::io::Error::from(ErrorKind::TimedOut).into()));
    match released {
        Ok(()) => tracing::info!("Released housekeeping leadership"),
        Err(error) => tracing::warn!(
            %error,
            "Could not release housekeeping leadership cleanly; it frees once the connection closes"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn timing() -> Timing {
        Timing::default()
    }

    #[tokio::test(start_paused = true)]
    async fn a_follower_does_not_lead() {
        let leadership = Leadership::manual(timing());

        assert!(!leadership.leads());
        assert_eq!(leadership.tenure(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_leader_leads_only_after_its_wait() {
        let leadership = Leadership::manual(timing());
        leadership.promote(Instant::now());

        assert!(!leadership.leads());
        tokio::time::advance(Duration::from_secs(5)).await;
        leadership.confirm(Instant::now());
        tokio::time::advance(Duration::from_secs(5)).await;
        leadership.confirm(Instant::now());
        tokio::time::advance(Duration::from_secs(5)).await;
        leadership.confirm(Instant::now());
        tokio::time::advance(Duration::from_secs(5)).await;

        assert!(leadership.leads());
    }

    #[tokio::test(start_paused = true)]
    async fn a_paused_leader_stops_leading_while_it_still_holds_the_lock() {
        let leadership = Leadership::manual(timing());
        leadership.promote(Instant::now());
        tokio::time::advance(Duration::from_secs(20)).await;
        leadership.confirm(Instant::now());
        assert!(leadership.leads());

        tokio::time::advance(Duration::from_secs(11)).await;

        assert!(
            leadership.tenure().is_some(),
            "nothing has noticed the pause, so the lock is still recorded as held"
        );
        assert!(
            !leadership.leads(),
            "a leader whose last answered ping is older than freshness must not dispatch"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn losing_resolves_when_pings_stop_being_answered() {
        let leadership = Leadership::manual(timing());
        let start = Instant::now();
        leadership.promote(start);
        tokio::time::advance(Duration::from_secs(20)).await;
        leadership.confirm(Instant::now());

        leadership.lost().await;

        assert_eq!(Instant::now(), start + Duration::from_secs(30));
    }

    #[tokio::test(start_paused = true)]
    async fn losing_resolves_on_demotion() {
        let leadership = Leadership::manual(timing());
        leadership.promote(Instant::now());
        tokio::time::advance(Duration::from_secs(20)).await;
        leadership.confirm(Instant::now());
        let demoting = leadership.clone();

        let lost = tokio::spawn(async move { leadership.lost().await });
        tokio::time::advance(Duration::from_secs(1)).await;
        demoting.demote();

        tokio::time::timeout(Duration::from_millis(1), lost)
            .await
            .expect("demotion is noticed at once")
            .expect("the waiter does not panic");
    }

    #[tokio::test(start_paused = true)]
    async fn releasing_stops_leading_before_the_lock_is_given_back() {
        let leadership = Leadership::manual(timing());
        leadership.promote(Instant::now());
        tokio::time::advance(Duration::from_secs(20)).await;
        leadership.confirm(Instant::now());
        assert!(leadership.leads());
        let watching = leadership.clone();
        let lost = tokio::spawn(async move { watching.lost().await });

        leadership.release().await;

        assert!(!leadership.leads());
        tokio::time::timeout(Duration::from_millis(1), lost)
            .await
            .expect("releasing is noticed at once")
            .expect("the waiter does not panic");
    }

    #[tokio::test]
    async fn releasing_a_leadership_that_never_started_returns() {
        Leadership::manual(timing()).release().await;
    }

    #[test]
    fn the_leader_key_is_in_the_zone_namespace() {
        assert_eq!(LEADER_KEY >> 32, 0x7A6F_6E65);
    }
}
