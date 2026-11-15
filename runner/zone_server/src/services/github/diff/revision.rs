use super::Diff;
use super::File;
use super::Status;

const BLOB: &str = "blob";
const SEPARATOR: &str = "/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revision<'a> {
    pub repository_url: &'a str,
    pub number: u64,
    pub head: &'a str,
    pub base: &'a str,
}

impl Revision<'_> {
    pub(super) fn blob_url(
        &self,
        status: Status,
        path: &str,
        previous_path: Option<&str>,
    ) -> String {
        let (commit, shown) = match status {
            Status::Removed => (self.base, previous_path.unwrap_or(path)),
            _ => (self.head, path),
        };
        format!(
            "{}{SEPARATOR}{BLOB}{SEPARATOR}{commit}{SEPARATOR}{}",
            self.repository_url.trim_end_matches(SEPARATOR),
            encoded(shown)
        )
    }

    pub(super) fn diff(&self, files: Vec<File>, complete: bool, warning: Option<&str>) -> Diff {
        Diff {
            number: self.number,
            head: self.head.to_string(),
            base: self.base.to_string(),
            complete,
            warning: warning.map(str::to_string),
            files,
        }
    }
}

fn encoded(path: &str) -> String {
    path.split(SEPARATOR)
        .map(urlencoding::encode)
        .collect::<Vec<_>>()
        .join(SEPARATOR)
}
