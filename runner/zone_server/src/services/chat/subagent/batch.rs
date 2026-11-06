use std::collections::HashMap;

use abnegate_agent_cli::Subagent;

use super::pending::Pending;

/// The updates that arrived together, folded into one per subagent in the
/// order each subagent was first heard of, after any a failed write left over.
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

impl From<Vec<Pending>> for Batch {
    fn from(pending: Vec<Pending>) -> Self {
        let positions = pending
            .iter()
            .enumerate()
            .map(|(position, pending)| (pending.update.id.clone(), position))
            .collect();
        Self { pending, positions }
    }
}
