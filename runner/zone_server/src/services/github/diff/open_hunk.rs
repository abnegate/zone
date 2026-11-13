use super::Hunk;
use super::Line;
use super::LineKind;

const ADDITION: u8 = b'+';
const DELETION: u8 = b'-';
const CONTEXT: u8 = b' ';
const META: u8 = b'\\';

pub(super) struct OpenHunk {
    hunk: Hunk,
    old_line: u32,
    new_line: u32,
    old_remaining: u32,
    new_remaining: u32,
}

impl OpenHunk {
    pub(super) fn open(header: &str) -> Option<Self> {
        let hunk = Hunk::open(header)?;
        Some(Self {
            old_line: hunk.old_start,
            new_line: hunk.new_start,
            old_remaining: hunk.old_lines,
            new_remaining: hunk.new_lines,
            hunk,
        })
    }

    pub(super) fn accept(&mut self, line: &str) -> Option<LineKind> {
        let first = line.as_bytes().first().copied();
        if first == Some(META) {
            if self.hunk.lines.is_empty() {
                return None;
            }
            self.push(LineKind::Meta, line.to_string(), None, None);
            return Some(LineKind::Meta);
        }
        if self.exhausted() {
            return None;
        }
        let content = || line.get(1..).unwrap_or_default().to_string();
        match first {
            Some(ADDITION) if self.new_remaining > 0 => {
                self.push(LineKind::Addition, content(), None, Some(self.new_line));
                self.new_line = self.new_line.saturating_add(1);
                self.new_remaining -= 1;
                Some(LineKind::Addition)
            }
            Some(DELETION) if self.old_remaining > 0 => {
                self.push(LineKind::Deletion, content(), Some(self.old_line), None);
                self.old_line = self.old_line.saturating_add(1);
                self.old_remaining -= 1;
                Some(LineKind::Deletion)
            }
            Some(CONTEXT) | None if self.old_remaining > 0 && self.new_remaining > 0 => {
                self.push(
                    LineKind::Context,
                    content(),
                    Some(self.old_line),
                    Some(self.new_line),
                );
                self.old_line = self.old_line.saturating_add(1);
                self.new_line = self.new_line.saturating_add(1);
                self.old_remaining -= 1;
                self.new_remaining -= 1;
                Some(LineKind::Context)
            }
            _ => None,
        }
    }

    pub(super) fn exhausted(&self) -> bool {
        self.old_remaining == 0 && self.new_remaining == 0
    }

    pub(super) fn close(self) -> Hunk {
        self.hunk
    }

    fn push(
        &mut self,
        kind: LineKind,
        content: String,
        old_line: Option<u32>,
        new_line: Option<u32>,
    ) {
        self.hunk.lines.push(Line {
            kind,
            content,
            old_line,
            new_line,
        });
    }
}
