/// What became of one message a turn was asked to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sent {
    Saved,
    Unsaved,
    Refused,
}

impl Sent {
    /// Whether the message was stored, so a queued turn stopped before then is put back.
    pub(super) fn saved(self) -> bool {
        self == Self::Saved
    }
}
