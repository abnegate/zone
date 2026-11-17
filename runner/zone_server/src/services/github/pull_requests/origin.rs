/// Where a listing's verdict on one pull request came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Kept,
    Batch,
    FollowUp,
    Rest,
    Late,
}
