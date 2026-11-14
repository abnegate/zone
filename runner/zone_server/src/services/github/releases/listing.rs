use chrono::DateTime;
use chrono::Utc;

use super::Release;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub releases: Vec<Release>,
    pub complete: bool,
    pub warnings: Vec<String>,
}

impl Listing {
    pub fn since(&self, since: DateTime<Utc>) -> Self {
        Self {
            releases: self
                .releases
                .iter()
                .filter(|release| {
                    DateTime::parse_from_rfc3339(release.published_at.trim())
                        .is_ok_and(|published| published >= since)
                })
                .cloned()
                .collect(),
            complete: self.complete,
            warnings: self.warnings.clone(),
        }
    }
}
