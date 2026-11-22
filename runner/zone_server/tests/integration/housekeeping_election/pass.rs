use tokio::time::Instant;

/// One cluster sweep a replica ran, from start to its end or abort.
#[derive(Debug, Clone, Copy)]
pub struct Pass {
    pub replica: usize,
    pub started: Instant,
    pub finished: Instant,
}

impl Pass {
    pub fn overlaps(&self, other: &Pass) -> bool {
        self.started < other.finished && other.started < self.finished
    }
}
