use serde::Serialize;

use super::Line;

const HEADER_START: &str = "@@ -";
const NEW_RANGE_START: &str = " +";
const HEADER_END: &str = " @@";
const COUNT_SEPARATOR: char = ',';
const IMPLIED_COUNT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<Line>,
}

impl Hunk {
    pub(super) fn open(header: &str) -> Option<Self> {
        let ranges = header.strip_prefix(HEADER_START)?;
        let (old, rest) = ranges.split_once(NEW_RANGE_START)?;
        let (new, _) = rest.split_once(HEADER_END)?;
        let (old_start, old_lines) = range(old)?;
        let (new_start, new_lines) = range(new)?;
        Some(Self {
            header: header.to_string(),
            old_start,
            old_lines,
            new_start,
            new_lines,
            lines: Vec::new(),
        })
    }
}

fn range(text: &str) -> Option<(u32, u32)> {
    match text.split_once(COUNT_SEPARATOR) {
        Some((start, count)) => Some((number(start)?, number(count)?)),
        None => Some((number(text)?, IMPLIED_COUNT)),
    }
}

fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}
