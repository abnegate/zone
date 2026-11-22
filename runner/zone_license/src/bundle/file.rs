#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum File {
    Archive,
    Checksum,
    Marker,
}

impl File {
    pub const ALL: [Self; 3] = [Self::Archive, Self::Checksum, Self::Marker];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Archive => ".tar",
            Self::Checksum => ".tar.sha256",
            Self::Marker => ".tar.sha256.sigstore.json",
        }
    }
}
