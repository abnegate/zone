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
    #[serde(rename = "kid")]
    key_id: String,
    x: String,
}

impl Jwk {
    pub fn new(key_id: impl Into<String>, public: &[u8; 32]) -> Self {
        Self {
            kty: KEY_TYPE,
            crv: CURVE,
            alg: ALGORITHM,
            usage: USAGE,
            key_id: key_id.into(),
            x: URL_SAFE_NO_PAD.encode(public),
        }
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn x(&self) -> &str {
        &self.x
    }
}
