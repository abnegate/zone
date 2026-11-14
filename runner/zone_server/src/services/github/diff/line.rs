use serde::Serialize;

use super::LineKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Line {
    pub kind: LineKind,
    pub content: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}
