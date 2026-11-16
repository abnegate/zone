use chrono::{DateTime, Utc};
use zone_enterprise::{Edition, Feature, License, Status, status};

use super::access::Access;
use super::refusal::Refusal;
use crate::auth::entitlement::Unentitled;

/// What the server's license means for agent policies: whether they can be changed, and whether
/// they are enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
}

impl Standing {
    pub fn of(license: Option<&License>, now: DateTime<Utc>) -> Self {
        Self {
            entitled: Feature::Policy.entitled(license, now),
            enforced: Feature::Policy.enforced(license),
            edition: Edition::from(license),
            status: status(license, now),
        }
    }

    pub fn editable(&self, access: Access) -> bool {
        self.entitled && access.writable()
    }

    /// Refuses a change unless the license is Team or Enterprise and active or in its grace
    /// period.
    pub fn require(&self) -> Result<(), Refusal> {
        if self.entitled {
            return Ok(());
        }
        Err(Unentitled(Feature::Policy).into())
    }
}
