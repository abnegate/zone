use serde::Serialize;

pub const ALGORITHM: &str = "EdDSA";
pub const TYPE: &str = "JWT";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Header<'a> {
    pub alg: &'static str,
    pub typ: &'static str,
    #[serde(rename = "kid")]
    pub key_id: &'a str,
}

impl<'a> Header<'a> {
    pub fn new(key_id: &'a str) -> Self {
        Self {
            alg: ALGORITHM,
            typ: TYPE,
            key_id,
        }
    }
}
