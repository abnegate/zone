use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::services::github::fault::Fault;

pub(crate) const GRAPHQL_SLOW: &str =
    "GitHub's GraphQL API took too long to answer, so this listing asks it nothing more.";
pub(crate) const GRAPHQL_LATE: &str = "GitHub's GraphQL API took too long to answer this read.";

/// How long one listing waits on each GitHub GraphQL read. A `bounded` read
/// that runs out of that time keeps every `bounded` read after it from being
/// sent; a `limited` read that runs out of it fails alone.
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

    pub(crate) async fn limited<T>(&self, read: impl Future<Output = T>) -> Result<T, Fault> {
        tokio::time::timeout(self.patience, read)
            .await
            .map_err(|_| Fault::upstream(GRAPHQL_LATE))
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
    async fn a_limited_read_past_its_patience_leaves_the_reads_after_it_alone() {
        let guard = Guard::new(Duration::from_millis(10));

        let slow = guard
            .limited(tokio::time::sleep(Duration::from_secs(5)))
            .await;
        let after = guard.bounded(async { 7 }).await;

        assert_eq!(slow, Err(Fault::upstream(GRAPHQL_LATE)));
        assert_eq!(after, Ok(7));
    }

    #[tokio::test]
    async fn a_read_inside_its_patience_is_answered() {
        let guard = Guard::new(Duration::from_secs(5));

        assert_eq!(guard.bounded(async { 7 }).await, Ok(7));
    }
}
