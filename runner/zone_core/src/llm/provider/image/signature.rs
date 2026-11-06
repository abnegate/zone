const RIFF: &[u8] = b"RIFF";

/// How a file of some image type begins.
pub enum Signature {
    Prefix(&'static [u8]),
    /// A RIFF container, whose format is named after its length.
    Riff(&'static [u8]),
}

impl Signature {
    pub fn starts(&self, bytes: &[u8]) -> bool {
        match self {
            Self::Prefix(prefix) => bytes.starts_with(prefix),
            Self::Riff(format) => bytes.starts_with(RIFF) && bytes.get(8..12) == Some(*format),
        }
    }
}
