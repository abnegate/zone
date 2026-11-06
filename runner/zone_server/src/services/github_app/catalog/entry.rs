use std::sync::Arc;
use std::time::Instant;

use abnegate_vcs::pull_request::InstalledRepository;

pub(super) struct Entry {
    pub(super) repositories: Arc<[InstalledRepository]>,
    pub(super) expires_at: Instant,
}

impl Entry {
    pub(super) fn fresh(&self, now: Instant) -> bool {
        now < self.expires_at
    }
}
