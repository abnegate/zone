use serde::Serialize;

use crate::gate::code::Code;
use crate::gate::detail::Detail;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Errors {
    pub errors: [Detail; 1],
}

impl Errors {
    pub fn new(code: Code, message: &'static str) -> Self {
        Self {
            errors: [Detail { code, message }],
        }
    }
}
