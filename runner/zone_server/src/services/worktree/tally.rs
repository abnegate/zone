//! What one sweep did across every repository it visited.

use std::ops::AddAssign;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub removed: usize,
    pub kept: usize,
    pub failed: usize,
}

impl AddAssign for Tally {
    fn add_assign(&mut self, other: Self) {
        self.removed += other.removed;
        self.kept += other.kept;
        self.failed += other.failed;
    }
}
