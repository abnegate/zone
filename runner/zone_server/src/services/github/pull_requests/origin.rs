/// Where a listing's verdict on one pull request came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    /// The verdict the listing before held, still current.
    Kept,
    /// A batch of evidence that held everything the verdict weighs.
    Batch,
    /// A batch of evidence, with threads or checks it could not hold read
    /// again.
    FollowUp,
    /// REST alone, after GitHub refused or could not answer for the batch.
    Rest,
    /// Nothing: the listing ran out of time before it was assessed again.
    Late,
}
