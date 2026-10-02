use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};
use tracing::Instrument;

use super::snapshot::Snapshot;

/// One reading of a login's usage, which runs to its end even when every refresh waiting on it
/// is dropped, and which every refresh of the login shares while it runs and for the TTL after it
/// started, whether it read anything or not.
#[derive(Clone)]
pub(super) struct Flight {
    started: Instant,
    reading: Shared<BoxFuture<'static, Option<Snapshot>>>,
}

impl Flight {
    pub(super) fn start(reading: impl Future<Output = Option<Snapshot>> + Send + 'static) -> Self {
        let task = tokio::spawn(reading.in_current_span());
        Self {
            started: Instant::now(),
            reading: async move { task.await.ok().flatten() }.boxed().shared(),
        }
    }

    /// Whether a refresh shares this reading rather than starting another.
    pub(super) fn current(&self, ttl: Duration) -> bool {
        self.reading.peek().is_none() || self.started.elapsed() < ttl
    }

    /// The snapshot the reading brought, or `None` when it brought none newer than the one the
    /// login had.
    pub(super) async fn landed(self) -> Option<Snapshot> {
        self.reading.await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;
    use tokio::sync::oneshot;

    use super::*;

    const TTL: Duration = Duration::from_secs(60);

    fn snapshot() -> Snapshot {
        Snapshot::new(Vec::new(), Utc::now())
    }

    #[tokio::test]
    async fn a_reading_runs_once_for_everyone_waiting_on_it() {
        let runs = Arc::new(AtomicUsize::new(0));
        let (release, released) = oneshot::channel::<()>();
        let flight = Flight::start({
            let runs = Arc::clone(&runs);
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                let _ = released.await;
                Some(snapshot())
            }
        });

        assert!(
            flight.current(Duration::ZERO),
            "a running reading is shared"
        );
        let waiters = futures::future::join(flight.clone().landed(), flight.clone().landed());
        let _ = release.send(());
        let (first, second) = waiters.await;

        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert!(first.is_some() && first == second);
    }

    #[tokio::test]
    async fn a_reading_nobody_waits_for_still_finishes() {
        let (finished, finishing) = oneshot::channel();
        let flight = Flight::start(async move {
            tokio::task::yield_now().await;
            let _ = finished.send(());
            None
        });

        drop(flight);

        finishing.await.expect("the reading to finish unwatched");
    }

    #[tokio::test]
    async fn a_finished_reading_is_shared_for_the_ttl_and_then_no_longer() {
        let flight = Flight::start(async { None });
        assert_eq!(flight.clone().landed().await, None);

        assert!(flight.current(TTL));
        assert!(!flight.current(Duration::ZERO));
    }
}
