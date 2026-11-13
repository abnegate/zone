use std::sync::Arc;

use dashmap::DashSet;
use uuid::Uuid;

#[derive(Debug)]
pub struct ReleasingGuard {
    releasing: Arc<DashSet<Uuid>>,
    source_id: Uuid,
}

impl ReleasingGuard {
    pub(super) fn claim(releasing: &Arc<DashSet<Uuid>>, source_id: Uuid) -> Option<Self> {
        releasing.insert(source_id).then(|| Self {
            releasing: Arc::clone(releasing),
            source_id,
        })
    }

    pub fn source_id(&self) -> Uuid {
        self.source_id
    }
}

impl Drop for ReleasingGuard {
    fn drop(&mut self) {
        self.releasing.remove(&self.source_id);
    }
}
