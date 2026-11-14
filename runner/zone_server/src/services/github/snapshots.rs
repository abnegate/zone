#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashSet;
use uuid::Uuid;

use super::cache::Cache;
use super::pull_requests;
use super::release::ReleasePipeline;
use super::releases;
use super::releases::SourceOptions;
use super::releasing_guard::ReleasingGuard;
use super::throttle::Throttle;
use crate::config::GithubConfig;

pub const PIPELINE_TTL: Duration = Duration::from_secs(15);
pub const OPTIONS_TTL: Duration = Duration::from_secs(300);
pub const REFRESH_FLOOR: Duration = Duration::from_secs(10);

pub struct Snapshots {
    pub pull_requests: Cache<Uuid, pull_requests::Listing>,
    pub releases: Cache<Uuid, releases::Listing>,
    pub options: Cache<Uuid, SourceOptions>,
    pub pipelines: Cache<(Uuid, u64), ReleasePipeline>,
    pub throttle: Throttle,
    releasing: Arc<DashSet<Uuid>>,
}

impl Snapshots {
    pub fn new(config: &GithubConfig) -> Self {
        Self {
            pull_requests: Cache::new(config.pull_request_ttl, REFRESH_FLOOR),
            releases: Cache::new(config.release_ttl, REFRESH_FLOOR),
            options: Cache::new(OPTIONS_TTL, REFRESH_FLOOR),
            pipelines: Cache::new(PIPELINE_TTL, REFRESH_FLOOR),
            throttle: Throttle::default(),
            releasing: Arc::new(DashSet::new()),
        }
    }

    pub fn invalidate(&self, source_id: Uuid) {
        self.pull_requests.invalidate(&source_id);
        self.releases.invalidate(&source_id);
        self.options.invalidate(&source_id);
    }

    pub fn try_release(&self, source_id: Uuid) -> Option<ReleasingGuard> {
        ReleasingGuard::claim(&self.releasing, source_id)
    }
}
