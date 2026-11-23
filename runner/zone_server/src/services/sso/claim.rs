#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    Verified,
    Reclaim,
    Pristine,
}
