use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::services::github::fault::Fault;

pub(crate) const GRAPHQL_SLOW: &str =
    "GitHub's GraphQL API took too long to answer, so this listing asks it nothing more.";

/// How long one listing waits on each GitHub GraphQL read, and whether one
/// of them already ran out of that time, after which none is sent.
#[derive(Debug, Clone)]
pub(crate) struct Guard {
    patience: Duration,
    slow: Arc<AtomicBool>,
}

impl Guard {
    pub(crate) fn new(patience: Duration) -> Self {
        Self {
            patience,
            slow: Arc::new(AtomicBool::new(false)),
        }
    }

    /// What `read` answers, unless GitHub already took too long over a read
    /// of this listing, in which case it is never sent, or takes too long
    /// over this one.
    pub(crate) async fn bounded<T>(&self, read: impl Future<Output = T>) -> Result<T, Fault> {
        if self.slow.load(Ordering::Acquire) {
            return Err(Fault::upstream(GRAPHQL_SLOW));
        }
        match tokio::time::timeout(self.patience, read).await {
            Ok(read) => Ok(read),
            Err(_) => {
                self.slow.store(true, Ordering::Release);
                Err(Fault::upstream(GRAPHQL_SLOW))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_read_past_its_patience_stops_every_read_after_it() {
        let guard = Guard::new(Duration::from_millis(10));
        let sent = AtomicBool::new(false);

        let slow = guard
            .bounded(tokio::time::sleep(Duration::from_secs(5)))
            .await;
        let after = guard
            .clone()
            .bounded(async { sent.store(true, Ordering::Release) })
            .await;

        assert_eq!(slow, Err(Fault::upstream(GRAPHQL_SLOW)));
        assert_eq!(after, Err(Fault::upstream(GRAPHQL_SLOW)));
        assert!(!sent.load(Ordering::Acquire), "a read after it was sent");
    }

    #[tokio::test]
    async fn a_read_inside_its_patience_is_answered() {
        let guard = Guard::new(Duration::from_secs(5));

        assert_eq!(guard.bounded(async { 7 }).await, Ok(7));
    }
}
