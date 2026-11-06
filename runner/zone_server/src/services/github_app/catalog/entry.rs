use std::sync::Arc;
use std::time::Instant;

use abnegate_vcs::pull_request::InstalledRepository;
use uuid::Uuid;

pub(super) struct Entry {
    pub(super) organization: Uuid,
    pub(super) repositories: Arc<[InstalledRepository]>,
    pub(super) expires_at: Instant,
}

impl Entry {
    pub(super) fn fresh(&self, now: Instant) -> bool {
        now < self.expires_at
    }

    pub(super) fn serves(&self, organization: Uuid, now: Instant) -> bool {
        self.organization == organization && self.fresh(now)
    }
}
