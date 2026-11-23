/// What became of one message a turn was asked to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sent {
    Saved,
    Unsaved,
    /// Fleet policy refused this runner the chat's lease, so nothing reached the chat. An owed
    /// turn is still owed.
    Refused,
}

impl Sent {
    /// Whether the message was stored, so a queued turn stopped before then is put back.
    pub(super) fn saved(self) -> bool {
        self == Self::Saved
    }
}
