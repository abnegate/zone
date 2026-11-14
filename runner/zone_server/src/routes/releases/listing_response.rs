use std::time::Duration;

use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;

use crate::services::github::releases::Recent;
use crate::services::github::releases::Release;
use crate::services::github::source_status::SourceStatus;

#[derive(Debug, Serialize)]
pub struct ListingResponse {
    pub generated_at: DateTime<Utc>,
    pub stale: bool,
    pub partial: bool,
    pub window_days: u32,
    pub ttl_secs: u64,
    pub sources: Vec<SourceStatus>,
    pub releases: Vec<Release>,
    pub warnings: Vec<String>,
}

impl ListingResponse {
    pub(crate) fn new(recent: Recent, ttl: Duration) -> Self {
        Self {
            generated_at: recent.generated_at,
            stale: recent.stale,
            partial: recent.partial,
            window_days: recent.window_days,
            ttl_secs: ttl.as_secs(),
            sources: recent.sources,
            releases: recent.releases,
            warnings: recent.warnings,
        }
    }
}
