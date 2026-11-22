use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::time::Instant;

use super::pass::Pass;
use super::tally::Tally;

/// A cluster sweep in progress. Dropping it, whether the sweep finished or was aborted, records
/// the pass.
pub struct Entered {
    pub(super) tally: Arc<Tally>,
    pub(super) replica: usize,
    pub(super) started: Instant,
}

impl Drop for Entered {
    fn drop(&mut self) {
        self.tally.inside.fetch_sub(1, Ordering::SeqCst);
        self.tally
            .passes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(Pass {
                replica: self.replica,
                started: self.started,
                finished: Instant::now(),
            });
    }
}
