use std::cmp::Reverse;

use chrono::DateTime;
use chrono::Utc;

use super::Listing;
use super::Release;
use crate::services::github::connection::ConnectedSource;
use crate::services::github::freshness::Freshness;
use crate::services::github::snapshot::Snapshot;
use crate::services::github::source_status::SourceStatus;

pub(crate) struct Recent {
    pub(crate) generated_at: DateTime<Utc>,
    pub(crate) stale: bool,
    pub(crate) partial: bool,
    pub(crate) window_days: u32,
    pub(crate) sources: Vec<SourceStatus>,
    pub(crate) releases: Vec<Release>,
    pub(crate) warnings: Vec<String>,
}

impl Recent {
    pub(crate) fn new(window_days: u32, now: DateTime<Utc>, warnings: Vec<String>) -> Self {
        Self {
            generated_at: now,
            stale: false,
            partial: !warnings.is_empty(),
            window_days,
            sources: Vec::new(),
            releases: Vec::new(),
            warnings,
        }
    }

    pub(crate) fn read(
        &mut self,
        source: &ConnectedSource,
        origin: &str,
        snapshot: &Snapshot<Listing>,
        freshness: Freshness,
    ) {
        self.generated_at = self.generated_at.min(snapshot.generated_at);
        self.stale |= freshness == Freshness::Stale;
        self.partial |= !snapshot.value.complete;
        self.warnings.extend(
            snapshot
                .value
                .warnings
                .iter()
                .map(|warning| format!("{}: {warning}", source.source.name)),
        );
        self.releases
            .extend(snapshot.value.releases.iter().cloned());
        self.sources
            .push(SourceStatus::read(source, origin, snapshot, freshness));
    }

    pub(crate) fn failed(&mut self, source: &ConnectedSource, origin: &str, warning: String) {
        self.stale = true;
        self.partial = true;
        self.sources
            .push(SourceStatus::failed(source, origin, warning));
    }

    pub(crate) fn sorted(mut self) -> Self {
        self.releases
            .sort_by_key(|release| Reverse(release.pipeline.release.published()));
        self
    }
}
