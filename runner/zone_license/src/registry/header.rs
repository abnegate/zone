use serde::Serialize;

pub const ALGORITHM: &str = "EdDSA";
pub const TYPE: &str = "JWT";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Header<'a> {
    pub alg: &'static str,
    pub typ: &'static str,
    pub kid: &'a str,
}

impl<'a> Header<'a> {
    pub fn new(kid: &'a str) -> Self {
        Self {
            alg: ALGORITHM,
            typ: TYPE,
            kid,
        }
    }
}
