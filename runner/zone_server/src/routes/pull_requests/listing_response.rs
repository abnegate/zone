use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;

use crate::services::github::pull_requests::Counts;
use crate::services::github::pull_requests::Overview;
use crate::services::github::pull_requests::PullRequest;
use crate::services::github::source_status::SourceStatus;

#[derive(Debug, Serialize)]
pub struct ListingResponse {
    pub generated_at: DateTime<Utc>,
    pub stale: bool,
    pub partial: bool,
    pub ttl_secs: u64,
    pub refresh_floor_secs: u64,
    pub counts: Counts,
    pub sources: Vec<SourceStatus>,
    pub pull_requests: Vec<PullRequest>,
    pub warnings: Vec<String>,
}

impl ListingResponse {
    pub fn new(overview: Overview, ttl: Duration, floor: Duration) -> Self {
        Self {
            generated_at: overview.generated_at,
            stale: overview.stale,
            partial: overview.partial,
            ttl_secs: ttl.as_secs(),
            refresh_floor_secs: floor.as_secs(),
            counts: overview.counts,
            sources: overview.sources,
            pull_requests: overview.pull_requests,
            warnings: overview.warnings,
        }
    }
}
