use zone_enterprise::fleet::Labels;

use crate::db::fleet::labels::Stored;

/// A save of required labels. `None` is a scope that never required any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub previous: Option<Stored>,
    pub current: Option<Stored>,
}

impl Saved {
    pub fn changed(&self) -> bool {
        labels(self.previous.as_ref()) != labels(self.current.as_ref())
    }
}

pub(super) fn labels(stored: Option<&Stored>) -> Labels {
    stored
        .map(|stored| stored.labels.clone())
        .unwrap_or_default()
}
