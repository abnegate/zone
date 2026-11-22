use std::sync::Arc;

use uuid::Uuid;

use super::shared::Shared;

/// A request waiting for its reply. Dropping it, answered or not, forgets the
/// correlation, so a late reply finds nobody and an abandoned wait leaks nothing.
pub struct Pending {
    correlation: Uuid,
    shared: Arc<Shared>,
}

impl Pending {
    pub fn new(correlation: Uuid, shared: Arc<Shared>) -> Self {
        Self {
            correlation,
            shared,
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.shared.pending.remove(&self.correlation);
    }
}
