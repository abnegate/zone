use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Serialize;

use crate::registry::header::ALGORITHM;

pub const KEY_TYPE: &str = "OKP";
pub const CURVE: &str = "Ed25519";
pub const USAGE: &str = "sig";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Jwk {
    kty: &'static str,
    crv: &'static str,
    alg: &'static str,
    #[serde(rename = "use")]
    usage: &'static str,
    kid: String,
    x: String,
}

impl Jwk {
    pub fn new(kid: impl Into<String>, public: &[u8; 32]) -> Self {
        Self {
            kty: KEY_TYPE,
            crv: CURVE,
            alg: ALGORITHM,
            usage: USAGE,
            kid: kid.into(),
            x: URL_SAFE_NO_PAD.encode(public),
        }
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }

    pub fn x(&self) -> &str {
        &self.x
    }
}
