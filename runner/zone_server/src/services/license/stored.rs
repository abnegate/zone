use zone_enterprise::{Error, Feature, License};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    Empty,
    Verified(License),
    Unverified(Error),
}

impl Stored {
    pub fn license(&self) -> Option<&License> {
        match self {
            Self::Verified(license) => Some(license),
            Self::Empty | Self::Unverified(_) => None,
        }
    }

    pub fn into_license(self) -> Option<License> {
        match self {
            Self::Verified(license) => Some(license),
            Self::Empty | Self::Unverified(_) => None,
        }
    }

    pub fn refusal(&self) -> Option<&Error> {
        match self {
            Self::Unverified(error) => Some(error),
            Self::Empty | Self::Verified(_) => None,
        }
    }

    /// A key that stopped verifying, such as after a keyring change, keeps every feature
    /// enforced: only removing the license stops enforcement.
    pub fn enforces(&self, feature: Feature) -> bool {
        match self {
            Self::Unverified(_) => true,
            Self::Empty | Self::Verified(_) => feature.enforced(self.license()),
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;
    use zone_enterprise::{Keyring, Payload, Plan, SigningKey, TrustedKey, sign};

    use super::*;

    const KEY_ID: &str = "test-1";
    const SEED: [u8; 32] = [0x11; 32];

    fn license(plan: Option<Plan>) -> License {
        let signing = SigningKey::from_bytes(&SEED);
        let keyring =
            Keyring::new([TrustedKey::new(KEY_ID, signing.verifying_key().to_bytes()).unwrap()])
                .unwrap();
        let text = sign(
            &Payload {
                id: Uuid::new_v4(),
                customer: "Acme Ltd".into(),
                seats: 3,
                issued_at: "2026-01-01T00:00:00Z".parse().unwrap(),
                expires_at: "2027-01-01T00:00:00Z".parse().unwrap(),
                key_id: KEY_ID.into(),
                subscription: None,
                plan,
            },
            &signing,
        );
        keyring.verify(&text).unwrap()
    }

    #[test]
    fn nothing_stored_has_no_license_and_no_refusal() {
        assert_eq!(Stored::Empty.license(), None);
        assert_eq!(Stored::Empty.refusal(), None);
        assert_eq!(Stored::Empty.into_license(), None);
    }

    #[test]
    fn an_unverified_key_has_its_refusal_and_no_license() {
        let stored = Stored::Unverified(Error::Signature);
        assert_eq!(stored.license(), None);
        assert_eq!(stored.refusal(), Some(&Error::Signature));
        assert_eq!(stored.into_license(), None);
    }

    #[test]
    fn a_key_that_stopped_verifying_enforces_every_feature() {
        let stored = Stored::Unverified(Error::Signature);
        for feature in Feature::ALL {
            assert!(stored.enforces(feature), "{feature:?}");
        }
    }

    #[test]
    fn nothing_stored_enforces_nothing() {
        for feature in Feature::ALL {
            assert!(!Stored::Empty.enforces(feature), "{feature:?}");
        }
    }

    #[test]
    fn a_verified_team_key_enforces_team_features_only() {
        let stored = Stored::Verified(license(Some(Plan::Team)));
        assert!(stored.enforces(Feature::Spend));
        assert!(!stored.enforces(Feature::Policy));
    }

    #[test]
    fn a_verified_key_without_a_plan_enforces_every_feature() {
        let stored = Stored::Verified(license(None));
        for feature in Feature::ALL {
            assert!(stored.enforces(feature), "{feature:?}");
        }
    }
}
