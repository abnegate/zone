#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    Ours,
    Taken,
    Absent,
    Unknown,
}
