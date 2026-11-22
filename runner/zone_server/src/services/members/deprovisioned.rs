use super::Halted;

/// What deprovisioning a member revoked and stopped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Deprovisioned {
    pub sessions: i64,
    pub halted: Halted,
}
