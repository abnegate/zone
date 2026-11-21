use aws_lc_rs::signature::{
    RSA_PKCS1_2048_8192_SHA256, RSA_PKCS1_2048_8192_SHA384, RSA_PKCS1_2048_8192_SHA512,
    RsaParameters,
};

use crate::sso::saml::refusal::Refusal;
use crate::sso::saml::uri;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureMethod {
    RsaSha256,
    RsaSha384,
    RsaSha512,
}

impl SignatureMethod {
    pub const ALL: [Self; 3] = [Self::RsaSha256, Self::RsaSha384, Self::RsaSha512];

    pub fn parse(algorithm: &str) -> Result<Self, Refusal> {
        Self::ALL
            .into_iter()
            .find(|method| method.uri() == algorithm)
            .ok_or(Refusal::Algorithm)
    }

    pub fn uri(self) -> &'static str {
        match self {
            Self::RsaSha256 => uri::RSA_SHA256,
            Self::RsaSha384 => uri::RSA_SHA384,
            Self::RsaSha512 => uri::RSA_SHA512,
        }
    }

    pub fn parameters(self) -> &'static RsaParameters {
        match self {
            Self::RsaSha256 => &RSA_PKCS1_2048_8192_SHA256,
            Self::RsaSha384 => &RSA_PKCS1_2048_8192_SHA384,
            Self::RsaSha512 => &RSA_PKCS1_2048_8192_SHA512,
        }
    }
}
