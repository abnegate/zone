use chrono::{DateTime, Utc};
use zone_enterprise::{Edition, Feature, Status, status};

use super::refusal::Refusal;
use crate::auth::entitlement::Unentitled;
use crate::auth::{Grant, Permission};
use crate::services::license::Stored;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    pub entitled: bool,
    pub edition: Edition,
    pub status: Status,
}

impl Standing {
    pub fn of(stored: &Stored, now: DateTime<Utc>) -> Self {
        let license = stored.license();
        Self {
            entitled: Feature::CustomRoles.entitled(license, now),
            edition: Edition::from(license),
            status: status(license, now),
        }
    }

    pub fn editable(&self, grant: &Grant) -> bool {
        self.entitled && grant.allows(Permission::RolesUpdate)
    }

    pub fn require(&self) -> Result<(), Refusal> {
        if self.entitled {
            return Ok(());
        }
        Err(Unentitled(Feature::CustomRoles).into())
    }
}
