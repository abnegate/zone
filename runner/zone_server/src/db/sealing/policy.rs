use std::fmt;

/// What the readers of a credential column accept, and so what sealing may write to it.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Policy {
    /// Every reader refuses plaintext.
    Strict,
    /// Some readers refuse plaintext and some hand it back.
    Mixed,
    /// Every reader hands plaintext back.
    Lenient,
    /// Counted for the report, never written.
    Counted,
}

impl Policy {
    /// Whether plaintext in the column is sealed. Only a column whose every reader hands
    /// plaintext back can take an envelope in its place without a reader changing behaviour.
    pub fn seals_plaintext(self) -> bool {
        matches!(self, Self::Lenient)
    }

    /// Whether sealing writes to the column at all.
    pub fn writes(self) -> bool {
        !matches!(self, Self::Counted)
    }

    /// The policy as the report prints it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Mixed => "mixed",
            Self::Lenient => "lenient",
            Self::Counted => "counted",
        }
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
