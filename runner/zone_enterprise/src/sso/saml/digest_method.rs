use aws_lc_rs::digest::{self, Algorithm, Digest, SHA256, SHA384, SHA512};

use crate::sso::saml::refusal::Refusal;
use crate::sso::saml::uri;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DigestMethod {
    Sha256,
    Sha384,
    Sha512,
}

impl DigestMethod {
    pub const ALL: [Self; 3] = [Self::Sha256, Self::Sha384, Self::Sha512];

    pub fn parse(algorithm: &str) -> Result<Self, Refusal> {
        Self::ALL
            .into_iter()
            .find(|method| method.uri() == algorithm)
            .ok_or(Refusal::Algorithm)
    }

    pub fn uri(self) -> &'static str {
        match self {
            Self::Sha256 => uri::SHA256,
            Self::Sha384 => uri::SHA384,
            Self::Sha512 => uri::SHA512,
        }
    }

    pub fn digest(self, data: &[u8]) -> Digest {
        digest::digest(self.algorithm(), data)
    }

    fn algorithm(self) -> &'static Algorithm {
        match self {
            Self::Sha256 => &SHA256,
            Self::Sha384 => &SHA384,
            Self::Sha512 => &SHA512,
        }
    }
}
