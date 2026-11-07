//! How an answer ended, as far as the reader needs telling.

use super::{CUT_SHORT_NOTE, INTERRUPTED_NOTE, TRUNCATED_NOTE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ending {
    /// The model finished it.
    Finished,
    /// It ran past the length limit, which is what the reader can act on, so it outranks the rest.
    Truncated,
    /// The turn failed partway, whatever the agent kept.
    Interrupted,
    /// A coding agent was stopped before it finished, keeping the answer it had given.
    CutShort,
}

impl Ending {
    /// What the stored answer closes with, so a reader can tell it did not end on its own.
    pub(super) const fn note(self) -> Option<&'static str> {
        match self {
            Self::Finished => None,
            Self::Truncated => Some(TRUNCATED_NOTE),
            Self::Interrupted => Some(INTERRUPTED_NOTE),
            Self::CutShort => Some(CUT_SHORT_NOTE),
        }
    }
}
