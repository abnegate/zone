use super::timing::Timing;

/// Intervals under which a demoted leader could still be sweeping when its successor starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "the housekeeping leader timing lets two leaders overlap: it needs ping < freshness, \
     ping < idle and freshness < wait, but is {0:?}"
)]
pub struct UnsafeTiming(pub Timing);
