use uuid::Uuid;

use super::relayed::Relayed;

/// The last frame a replay already showed one connection, so the relayed
/// copies of it and of everything before it are not shown twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Floor {
    pub origin: Uuid,
    pub sequence: u64,
}

impl Floor {
    pub fn covers(&self, relayed: &Relayed) -> bool {
        relayed.origin == self.origin && relayed.sequence <= self.sequence
    }
}
