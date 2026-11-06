use std::collections::HashMap;

use zone_core::llm::Subagent;

use super::pending::Pending;

/// The updates that arrived together, folded into one per subagent in the
/// order each subagent was first heard of.
#[derive(Default)]
pub(super) struct Batch {
    pending: Vec<Pending>,
    positions: HashMap<String, usize>,
}

impl Batch {
    pub(super) fn add(&mut self, update: Subagent) {
        match self.positions.get(&update.id) {
            Some(&position) => self.pending[position].fold(update),
            None => {
                self.positions.insert(update.id.clone(), self.pending.len());
                self.pending.push(Pending::new(update));
            }
        }
    }

    pub(super) fn into_pending(self) -> Vec<Pending> {
        self.pending
    }
}
