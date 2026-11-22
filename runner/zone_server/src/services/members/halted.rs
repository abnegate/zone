#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Halted {
    pub turns: usize,
    pub runs: usize,
    pub invitations: u64,
}
