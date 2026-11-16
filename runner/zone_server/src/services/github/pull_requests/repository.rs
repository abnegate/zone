use uuid::Uuid;

use crate::services::github::connection::ConnectedSource;

const SEPARATOR: &str = "/";
const PULL_PATH: &str = "pull";
const COMMIT_PATH: &str = "commit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub source_id: Uuid,
    pub name: String,
    pub url: String,
}

impl Repository {
    pub(crate) fn of(source: &ConnectedSource, origin: &str) -> Self {
        Self {
            source_id: source.source.id,
            name: source.configuration.full_name(),
            url: source.configuration.web_url(origin),
        }
    }

    pub fn pull_url(&self, number: u64) -> String {
        self.link(PULL_PATH, &number.to_string())
    }

    pub fn commit_url(&self, sha: &str) -> String {
        self.link(COMMIT_PATH, sha)
    }

    fn link(&self, kind: &str, name: &str) -> String {
        [self.url.trim_end_matches(SEPARATOR), kind, name].join(SEPARATOR)
    }
}
