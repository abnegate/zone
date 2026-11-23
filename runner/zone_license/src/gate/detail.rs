use serde::Serialize;

use crate::gate::code::Code;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Detail {
    pub code: Code,
    pub message: &'static str,
}
