use super::Hunk;
use super::LineKind;
use super::Patch;
use super::open_hunk::OpenHunk;

#[derive(Default)]
pub(super) struct Hunks {
    closed: Vec<Hunk>,
    open: Option<OpenHunk>,
    additions: u32,
    deletions: u32,
    truncated: bool,
}

impl Hunks {
    pub(super) fn accept(&mut self, line: &str) -> bool {
        let Some(open) = self.open.as_mut() else {
            return false;
        };
        match open.accept(line) {
            Some(LineKind::Addition) => self.additions += 1,
            Some(LineKind::Deletion) => self.deletions += 1,
            Some(LineKind::Context | LineKind::Meta) => {}
            None => return false,
        }
        true
    }

    pub(super) fn open(&mut self, header: &str) {
        self.close();
        match OpenHunk::open(header) {
            Some(open) => self.open = Some(open),
            None => self.truncated = true,
        }
    }

    pub(super) fn stray(&mut self) {
        self.close();
        self.truncated = true;
    }

    pub(super) fn started(&self) -> bool {
        self.open.is_some() || !self.closed.is_empty()
    }

    pub(super) fn finish(mut self) -> Patch {
        self.close();
        Patch {
            hunks: self.closed,
            additions: self.additions,
            deletions: self.deletions,
            truncated: self.truncated,
        }
    }

    fn close(&mut self) {
        if let Some(open) = self.open.take() {
            self.truncated |= !open.exhausted();
            self.closed.push(open.close());
        }
    }
}
