use chrono::{DateTime, Utc};
use zone_enterprise::{Edition, Feature, License, Status, status};

use super::access::Access;
use super::refusal::Refusal;
use crate::auth::entitlement::Unentitled;

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

    pub fn require(&self) -> Result<(), Refusal> {
        if self.entitled {
            return Ok(());
        }
        Err(Unentitled(Feature::Policy).into())
    }
}
