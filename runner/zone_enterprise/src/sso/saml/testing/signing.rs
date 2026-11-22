use crate::sso::saml::testing::Key;
use crate::sso::saml::uri;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signing {
    pub key: Key,
    pub canonicalization: &'static str,
    pub signed_info_prefixes: Option<Vec<String>>,
    pub method: &'static str,
    pub reference: Option<String>,
    pub references: usize,
    pub transforms: Vec<&'static str>,
    pub reference_prefixes: Option<Vec<String>>,
    pub digest_method: &'static str,
    pub key_info: bool,
}

impl Default for Signing {
    fn default() -> Self {
        Self {
            key: Key::Primary,
            canonicalization: uri::EXCLUSIVE_CANONICALIZATION,
            signed_info_prefixes: None,
            method: uri::RSA_SHA256,
            reference: None,
            references: 1,
            transforms: vec![uri::ENVELOPED_SIGNATURE, uri::EXCLUSIVE_CANONICALIZATION],
            reference_prefixes: None,
            digest_method: uri::SHA256,
            key_info: true,
        }
    }
}
