use crate::db::fleet::shortfall::Shortfall;

/// How a claim on a piece of work ended. `Unavailable` work is no one's to claim: another
/// execution holds it, it ended, or its admission lapsed. `Refused` work is still there for a
/// runner that meets fleet policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    Claimed,
    Unavailable,
    Refused(Shortfall),
}

impl Claim {
    pub fn claimed(&self) -> bool {
        matches!(self, Self::Claimed)
    }
}
