use std::fmt;

/// What a stored credential is, judged under the deployment's key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Class {
    /// An empty string, which seals nothing.
    Empty,
    /// An `ENC[v1:...]` envelope that opens under the key.
    Envelope,
    /// A closed envelope that does not open: another key, a later version, or damage.
    Unopenable,
    /// A bare body sealed before envelopes existed, which opens under the key.
    Legacy,
    /// Text that cannot be a sealed value.
    Plaintext,
    /// Text that opens as nothing but could be a sealed value: it starts an envelope, or is
    /// base64 long enough to hold a nonce and a tag.
    Ambiguous,
}

impl Class {
    /// Every class, in report order.
    pub const ALL: &[Self] = &[
        Self::Empty,
        Self::Envelope,
        Self::Unopenable,
        Self::Legacy,
        Self::Plaintext,
        Self::Ambiguous,
    ];

    /// The class as the report prints it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Envelope => "envelope",
            Self::Unopenable => "unopenable",
            Self::Legacy => "legacy",
            Self::Plaintext => "plaintext",
            Self::Ambiguous => "ambiguous",
        }
    }
}

impl fmt::Display for Class {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
