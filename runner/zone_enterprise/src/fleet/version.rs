use std::cmp::Ordering;
use std::fmt;

use crate::fleet::error::Error;

const RELEASE: i32 = 1;
const PRERELEASE: i32 = 0;

/// A Zone version as `MAJOR.MINOR.PATCH[-prerelease]`, ordered by semantic-version precedence.
/// Build metadata is accepted when parsing and dropped, since it carries no precedence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version {
    major: i32,
    minor: i32,
    patch: i32,
    prerelease: Option<String>,
}

impl Version {
    /// A version as a runner reports it: `MAJOR.MINOR.PATCH[-prerelease][+build]`.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let invalid = || Error::VersionInvalid(text.to_string());
        let (rest, build) = match text.split_once('+') {
            Some((rest, build)) => (rest, Some(build)),
            None => (text, None),
        };
        let (core, prerelease) = match rest.split_once('-') {
            Some((core, prerelease)) => (core, Some(prerelease)),
            None => (rest, None),
        };
        let identified = [build, prerelease].into_iter().flatten().all(identifiers);
        if !identified {
            return Err(invalid());
        }
        let [major, minor, patch] = numbers(core).ok_or_else(invalid)?;
        Ok(Self {
            major,
            minor,
            patch,
            prerelease: prerelease.map(str::to_string),
        })
    }

    /// A version a fleet's minimum may name: a plain `MAJOR.MINOR.PATCH`.
    pub fn minimum(text: &str) -> Result<Self, Error> {
        let [major, minor, patch] =
            numbers(text).ok_or_else(|| Error::VersionInvalid(text.to_string()))?;
        Ok(Self {
            major,
            minor,
            patch,
            prerelease: None,
        })
    }

    /// `[major, minor, patch, 1 for a release or 0 for a pre-release]`, which orders as the
    /// versions do when compared element by element, except between two pre-releases of one
    /// release, which share a key.
    pub fn key(&self) -> [i32; 4] {
        let release = match self.prerelease {
            Some(_) => PRERELEASE,
            None => RELEASE,
        };
        [self.major, self.minor, self.patch, release]
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)?;
        match &self.prerelease {
            Some(prerelease) => write!(formatter, "-{prerelease}"),
            None => Ok(()),
        }
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => precedence(left, right),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn numbers(core: &str) -> Option<[i32; 3]> {
    let mut parts = core.split('.');
    let numbers = [
        number(parts.next()?)?,
        number(parts.next()?)?,
        number(parts.next()?)?,
    ];
    parts.next().is_none().then_some(numbers)
}

fn number(text: &str) -> Option<i32> {
    let digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let canonical = text == "0" || !text.starts_with('0');
    match digits && canonical {
        true => text.parse().ok(),
        false => None,
    }
}

fn identifiers(text: &str) -> bool {
    text.split('.').all(|identifier| {
        !identifier.is_empty()
            && identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn precedence(left: &str, right: &str) -> Ordering {
    let mut left = left.split('.');
    let mut right = right.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left), Some(right)) => match identifier(left, right) {
                Ordering::Equal => continue,
                ordering => return ordering,
            },
        }
    }
}

fn identifier(left: &str, right: &str) -> Ordering {
    let numeric = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
    match (numeric(left), numeric(right)) {
        (true, true) => (left.len(), left).cmp(&(right.len(), right)),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => left.cmp(right),
    }
}
