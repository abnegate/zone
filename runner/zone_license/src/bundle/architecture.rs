use std::fmt;

pub const AMD64: &str = "amd64";
pub const ARM64: &str = "arm64";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Architecture {
    Amd64,
    Arm64,
}

impl Architecture {
    pub const ALL: [Self; 2] = [Self::Amd64, Self::Arm64];

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|architecture| architecture.as_str() == text)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Amd64 => AMD64,
            Self::Arm64 => ARM64,
        }
    }
}

impl fmt::Display for Architecture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
