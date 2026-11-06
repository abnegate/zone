use std::sync::Arc;
use std::time::Instant;

use abnegate_vcs::pull_request::InstalledRepository;
use uuid::Uuid;

use super::{MINIMUM_REFRESH_AGE, TTL};

pub(super) struct Entry {
    pub(super) organization: Uuid,
    pub(super) repositories: Arc<[InstalledRepository]>,
    pub(super) listed_at: Instant,
}

impl Entry {
    pub(super) fn fresh(&self, now: Instant) -> bool {
        now < self.listed_at + TTL
    }

    pub(super) fn serves(&self, organization: Uuid, now: Instant) -> bool {
        self.organization == organization && self.fresh(now)
    }

    pub(super) fn refreshable(&self, now: Instant) -> bool {
        now >= self.listed_at + MINIMUM_REFRESH_AGE
    }
}
