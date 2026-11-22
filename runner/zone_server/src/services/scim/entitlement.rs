use chrono::{DateTime, Utc};
use zone_enterprise::{Feature, License};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entitlement {
    Full,
    Deprovisioning,
}

impl Entitlement {
    pub fn of(license: Option<&License>, now: DateTime<Utc>) -> Self {
        if Feature::Scim.entitled(license, now) {
            Self::Full
        } else {
            Self::Deprovisioning
        }
    }

    pub fn is_full(self) -> bool {
        self == Self::Full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_license_only_deprovisioning_is_allowed() {
        assert_eq!(
            Entitlement::of(None, Utc::now()),
            Entitlement::Deprovisioning
        );
        assert!(!Entitlement::Deprovisioning.is_full());
        assert!(Entitlement::Full.is_full());
    }
}
