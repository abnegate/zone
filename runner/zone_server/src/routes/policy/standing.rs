use chrono::{DateTime, Utc};
use zone_enterprise::{Edition, Feature, Status, status};

use super::access::Access;
use crate::auth::entitlement::Unentitled;
use crate::services::license::Stored;

/// Where a license leaves one Enterprise feature: whether changes are allowed and whether the
/// saved settings apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    pub feature: Feature,
    pub entitled: bool,
    pub enforced: bool,
    pub edition: Edition,
    pub status: Status,
}

impl Standing {
    pub fn of(stored: &Stored, now: DateTime<Utc>, feature: Feature) -> Self {
        let license = stored.license();
        Self {
            feature,
            entitled: feature.entitled(license, now),
            enforced: stored.enforces(feature),
            edition: Edition::from(license),
            status: status(license, now),
        }
    }

    pub fn editable(&self, access: Access) -> bool {
        self.entitled && access.writable()
    }

    pub fn require(&self) -> Result<(), Unentitled> {
        if self.entitled {
            return Ok(());
        }
        Err(Unentitled(self.feature))
    }
}

#[cfg(test)]
mod tests {
    use zone_enterprise::Plan;

    use super::*;

    #[test]
    fn a_server_without_a_license_is_refused_for_the_feature_it_asked_about() {
        let now = Utc::now();
        for feature in [Feature::Policy, Feature::FleetPolicy] {
            let standing = Standing::of(&Stored::Empty, now, feature);
            assert!(!standing.entitled, "{feature:?}");
            assert!(!standing.enforced, "{feature:?}");
            assert!(!standing.editable(Access::Write), "{feature:?}");
            assert_eq!(standing.require(), Err(Unentitled(feature)));
            assert_eq!(feature.plan(), Plan::Enterprise, "{feature:?}");
        }
    }
}
