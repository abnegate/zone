use super::File;
use super::Revision;
use super::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    pub path: &'a str,
    pub previous_path: Option<&'a str>,
    pub status: Status,
    pub additions: u32,
    pub deletions: u32,
    pub patch: Option<&'a str>,
}

impl Entry<'_> {
    pub(super) fn file(self, revision: &Revision<'_>) -> File {
        let changes = self.additions.saturating_add(self.deletions);
        let unchanged = self.status.moves() || self.status == Status::Unchanged;
        let (hunks, binary, truncated) = match self.patch {
            Some(text) => {
                let patch = super::patch(text);
                let truncated = patch.truncated
                    || patch.additions != self.additions
                    || patch.deletions != self.deletions;
                (patch.hunks, false, truncated)
            }
            None => (Vec::new(), changes == 0 && !unchanged, changes > 0),
        };
        File {
            path: self.path.to_string(),
            previous_path: self.previous_path.map(str::to_string),
            status: self.status,
            additions: self.additions,
            deletions: self.deletions,
            changes,
            binary,
            truncated,
            blob_url: revision.blob_url(self.status, self.path, self.previous_path),
            hunks,
        }
    }
}
