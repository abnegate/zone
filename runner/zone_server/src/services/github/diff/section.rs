use super::File;
use super::HUNK_START;
use super::Revision;
use super::Status;
use super::hunks::Hunks;
use super::path;

const NEW_FILE_MODE: &str = "new file mode ";
const DELETED_FILE_MODE: &str = "deleted file mode ";
const RENAME_FROM: &str = "rename from ";
const RENAME_TO: &str = "rename to ";
const COPY_FROM: &str = "copy from ";
const COPY_TO: &str = "copy to ";
const OLD_SIDE: &str = "--- ";
const NEW_SIDE: &str = "+++ ";
const BINARY_FILES: &str = "Binary files ";
const BINARY_PATCH: &str = "GIT binary patch";

pub(super) struct Section {
    old_path: Option<String>,
    new_path: Option<String>,
    status: Status,
    binary: bool,
    hunks: Hunks,
}

impl Section {
    pub(super) fn open(header: &str) -> Self {
        let (old_path, new_path) = path::header_paths(header).unzip();
        Self {
            old_path,
            new_path,
            status: Status::Modified,
            binary: false,
            hunks: Hunks::default(),
        }
    }

    pub(super) fn accept(&mut self, line: &str) -> bool {
        self.hunks.accept(line)
    }

    pub(super) fn read(&mut self, line: &str) {
        if line.starts_with(HUNK_START) {
            self.hunks.open(line);
        } else if self.hunks.started() {
            self.hunks.stray();
        } else {
            self.describe(line);
        }
    }

    pub(super) fn finish(self, revision: &Revision<'_>) -> File {
        let patch = self.hunks.finish();
        let (primary, secondary) = match self.status {
            Status::Removed => (self.old_path.clone(), self.new_path),
            _ => (self.new_path, self.old_path.clone()),
        };
        let path = primary.or(secondary).unwrap_or_default();
        let previous_path = self
            .old_path
            .filter(|previous| self.status.moves() && *previous != path);
        File {
            blob_url: revision.blob_url(self.status, &path, previous_path.as_deref()),
            path,
            previous_path,
            status: self.status,
            additions: patch.additions,
            deletions: patch.deletions,
            changes: patch.additions.saturating_add(patch.deletions),
            binary: self.binary,
            truncated: patch.truncated,
            hunks: patch.hunks,
        }
    }

    fn describe(&mut self, line: &str) {
        if line.starts_with(NEW_FILE_MODE) {
            self.status = Status::Added;
        } else if line.starts_with(DELETED_FILE_MODE) {
            self.status = Status::Removed;
        } else if let Some(name) = line.strip_prefix(RENAME_FROM) {
            self.moved(Status::Renamed, Some(name), None);
        } else if let Some(name) = line.strip_prefix(RENAME_TO) {
            self.moved(Status::Renamed, None, Some(name));
        } else if let Some(name) = line.strip_prefix(COPY_FROM) {
            self.moved(Status::Copied, Some(name), None);
        } else if let Some(name) = line.strip_prefix(COPY_TO) {
            self.moved(Status::Copied, None, Some(name));
        } else if let Some(name) = line.strip_prefix(OLD_SIDE) {
            match path::side(name, path::OLD_PREFIX) {
                Some(old) => self.old_path = Some(old),
                None => self.status = Status::Added,
            }
        } else if let Some(name) = line.strip_prefix(NEW_SIDE) {
            match path::side(name, path::NEW_PREFIX) {
                Some(new) => self.new_path = Some(new),
                None => self.status = Status::Removed,
            }
        } else if line.starts_with(BINARY_FILES) || line == BINARY_PATCH {
            self.binary = true;
        }
    }

    fn moved(&mut self, status: Status, from: Option<&str>, to: Option<&str>) {
        self.status = status;
        if let Some(from) = from {
            self.old_path = Some(path::unquote(from));
        }
        if let Some(to) = to {
            self.new_path = Some(path::unquote(to));
        }
    }
}
