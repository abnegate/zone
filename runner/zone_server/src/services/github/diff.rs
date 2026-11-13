mod entry;
mod file;
mod hunk;
mod hunks;
mod line;
mod line_kind;
mod open_hunk;
mod patch;
mod path;
mod revision;
mod section;
mod status;
#[cfg(test)]
mod tests;

use serde::Serialize;

pub use entry::Entry;
pub use file::File;
pub use hunk::Hunk;
use hunks::Hunks;
pub use line::Line;
pub use line_kind::LineKind;
pub use patch::Patch;
pub use revision::Revision;
use section::Section;
pub use status::Status;

pub const TRUNCATED_DIFF: &str =
    "The diff is larger than the server reads at once; showing its start.";
pub const PER_FILE_DIFF: &str = "GitHub would not render the whole diff; showing per-file patches.";
pub const UNREADABLE_PATCH: &str = "GitHub returned a patch that ends early for at least one file.";

const FILE_START: &str = "diff --git ";
const HUNK_START: &str = "@@";
const LINE_END: char = '\n';

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub number: u64,
    pub head: String,
    pub base: String,
    pub complete: bool,
    pub warning: Option<String>,
    pub files: Vec<File>,
}

pub fn parse(text: &str, truncated: bool, revision: &Revision<'_>) -> Diff {
    let readable = if truncated { whole_lines(text) } else { text };
    let mut files = Vec::new();
    let mut current: Option<Section> = None;
    for line in readable.split_terminator(LINE_END) {
        if current.as_mut().is_some_and(|section| section.accept(line)) {
            continue;
        }
        if let Some(header) = line.strip_prefix(FILE_START) {
            files.extend(current.take().map(|section| section.finish(revision)));
            current = Some(Section::open(header));
        } else if let Some(section) = current.as_mut() {
            section.read(line);
        }
    }
    files.extend(current.map(|section| section.finish(revision)));
    if truncated && let Some(last) = files.last_mut() {
        last.truncated = true;
    }
    let complete = !truncated && files.iter().all(|file| !file.truncated);
    let warning = if truncated {
        Some(TRUNCATED_DIFF)
    } else if !complete {
        Some(UNREADABLE_PATCH)
    } else {
        None
    };
    revision.diff(files, complete, warning)
}

pub fn per_file<'a>(entries: impl IntoIterator<Item = Entry<'a>>, revision: &Revision<'_>) -> Diff {
    let files = entries
        .into_iter()
        .map(|entry| entry.file(revision))
        .collect();
    revision.diff(files, false, Some(PER_FILE_DIFF))
}

pub fn patch(text: &str) -> Patch {
    let mut hunks = Hunks::default();
    for line in text.split_terminator(LINE_END) {
        if hunks.accept(line) {
            continue;
        }
        if line.starts_with(HUNK_START) {
            hunks.open(line);
        } else {
            hunks.stray();
        }
    }
    hunks.finish()
}

fn whole_lines(text: &str) -> &str {
    text.rfind(LINE_END).map_or("", |end| &text[..=end])
}
