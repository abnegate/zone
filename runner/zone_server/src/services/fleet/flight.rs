use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::Mutex;

/// One attempt at a time, shared with every caller that waited on it: a caller queued behind an
/// attempt takes that attempt's outcome instead of making one of its own.
#[derive(Default)]
pub struct Flight {
    turn: Mutex<()>,
    attempts: AtomicU64,
}

impl Flight {
    /// Runs `attempt` and answers its outcome, or answers `None` without running it when another
    /// caller's attempt finished while this one waited its turn. An attempt dropped part way
    /// counts for nothing, so the next caller makes its own.
    pub async fn join<T>(&self, attempt: impl Future<Output = T>) -> Option<T> {
        let seen = self.attempts.load(Ordering::Acquire);
        let _turn = self.turn.lock().await;
        if self.attempts.load(Ordering::Acquire) != seen {
            return None;
        }
        let outcome = attempt.await;
        self.attempts.fetch_add(1, Ordering::Release);
        Some(outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use futures::future::join_all;

    use super::*;

    #[tokio::test]
    async fn callers_that_wait_on_an_attempt_share_it() {
        let flight = Flight::default();
        let made = AtomicUsize::new(0);
        let attempt = || async {
            made.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
        };

        let outcomes = join_all((0..8).map(|_| flight.join(attempt()))).await;

        assert_eq!(made.load(Ordering::SeqCst), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_some()).count(),
            1
        );
    }

    #[tokio::test]
    async fn a_caller_after_an_attempt_makes_its_own() {
        let flight = Flight::default();

        assert_eq!(flight.join(async { 1 }).await, Some(1));
        assert_eq!(flight.join(async { 2 }).await, Some(2));
    }

    #[tokio::test]
    async fn an_attempt_dropped_part_way_leaves_the_next_caller_to_make_one() {
        let flight = Flight::default();
        let dropped = tokio::time::timeout(
            std::time::Duration::from_millis(10),
            flight.join(std::future::pending::<()>()),
        )
        .await;

        assert!(dropped.is_err());
        assert_eq!(flight.join(async { 3 }).await, Some(3));
    }
}
