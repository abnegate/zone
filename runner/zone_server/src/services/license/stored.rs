use zone_enterprise::{Error, License};

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
