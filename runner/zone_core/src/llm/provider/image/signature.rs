use std::ops::Range;

const RIFF: &[u8] = b"RIFF";

/// Where a RIFF container names its format, after its four-byte length.
const RIFF_FORMAT: Range<usize> = 8..12;

/// How a file of some image type begins.
pub enum Signature {
    Prefix(&'static [u8]),
    Riff(&'static [u8]),
}

impl Signature {
    pub fn starts(&self, bytes: &[u8]) -> bool {
        match self {
            Self::Prefix(prefix) => bytes.starts_with(prefix),
            Self::Riff(format) => {
                bytes.starts_with(RIFF) && bytes.get(RIFF_FORMAT) == Some(*format)
            }
        }
    }
}
