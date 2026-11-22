use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::time::Instant;

use super::entered::Entered;
use super::pass::Pass;

/// Every cluster pass and replica turn the workers under test made, shared between them.
#[derive(Default)]
pub struct Tally {
    pub(super) passes: Mutex<Vec<Pass>>,
    pub(super) inside: AtomicUsize,
    peak: AtomicUsize,
    turns: Mutex<BTreeMap<usize, usize>>,
}

impl Tally {
    pub fn enter(self: &Arc<Self>, replica: usize) -> Entered {
        let inside = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(inside, Ordering::SeqCst);
        Entered {
            tally: Arc::clone(self),
            replica,
            started: Instant::now(),
        }
    }

    pub fn turn(&self, replica: usize) {
        *self
            .turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(replica)
            .or_default() += 1;
    }

    /// The most cluster sweeps that were ever running at once, on any replica.
    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    pub fn passes(&self) -> Vec<Pass> {
        let mut passes = self
            .passes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        passes.sort_by_key(|pass| pass.started);
        passes
    }

    pub fn passes_by(&self, replica: usize) -> Vec<Pass> {
        self.passes()
            .into_iter()
            .filter(|pass| pass.replica == replica)
            .collect()
    }

    pub fn turns(&self, replica: usize) -> usize {
        self.turns
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&replica)
            .copied()
            .unwrap_or(0)
    }
}
