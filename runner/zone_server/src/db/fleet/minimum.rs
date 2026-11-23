use std::fmt;

use zone_enterprise::fleet::{Error, Version};

/// The lowest version a fleet admits, always a plain release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Minimum(Version);

impl Minimum {
    pub fn parse(text: &str) -> Result<Self, Error> {
        Version::minimum(text).map(Self)
    }

    pub fn version(&self) -> &Version {
        &self.0
    }

    pub fn admits(&self, version: &Version) -> bool {
        version.key() >= self.0.key()
    }
}

impl fmt::Display for Minimum {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimum_admits_its_release_and_later_but_not_its_pre_releases() {
        let minimum = Minimum::parse("1.4.0").unwrap();
        let admits = |text: &str| minimum.admits(&Version::parse(text).unwrap());
        assert!(admits("1.4.0"));
        assert!(admits("1.4.1-rc.1"));
        assert!(admits("1.10.0"));
        assert!(!admits("1.4.0-rc.1"));
        assert!(!admits("1.3.9"));
    }

    #[test]
    fn a_minimum_refuses_anything_but_a_plain_release() {
        for text in ["1.4", "v1.4.0", "1.4.0-rc.1", ""] {
            assert_eq!(
                Minimum::parse(text),
                Err(Error::VersionInvalid(text.to_string()))
            );
        }
        assert_eq!(Minimum::parse("1.4.0").unwrap().to_string(), "1.4.0");
    }
}
