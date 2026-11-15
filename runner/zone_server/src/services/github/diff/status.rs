use abnegate_vcs::FileStatus;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Added,
    Removed,
    Modified,
    Renamed,
    Copied,
    Changed,
    Unchanged,
    Unknown,
}

impl Status {
    pub(super) fn moves(self) -> bool {
        matches!(self, Status::Renamed | Status::Copied)
    }
}

impl From<FileStatus> for Status {
    fn from(status: FileStatus) -> Self {
        match status {
            FileStatus::Added => Status::Added,
            FileStatus::Removed => Status::Removed,
            FileStatus::Modified => Status::Modified,
            FileStatus::Renamed => Status::Renamed,
            FileStatus::Copied => Status::Copied,
            FileStatus::Changed => Status::Changed,
            FileStatus::Unchanged => Status::Unchanged,
            _ => Status::Unknown,
        }
    }
}
