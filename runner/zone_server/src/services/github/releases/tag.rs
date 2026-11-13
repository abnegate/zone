use std::cmp::Ordering;

const PREFIX: &str = "v";
const UNPREFIXED: &str = "";
const SEPARATOR: char = '.';
const PRERELEASE: char = '-';
const ZERO: &str = "0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tag {
    name: String,
    prefix: &'static str,
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<String>,
}

impl Tag {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        let (prefix, version) = match name.strip_prefix(PREFIX) {
            Some(version) => (PREFIX, version),
            None => (UNPREFIXED, name),
        };
        let (core, prerelease) = match version.split_once(PRERELEASE) {
            Some((core, prerelease)) => (core, Some(prerelease)),
            None => (version, None),
        };
        if prerelease.is_some_and(|prerelease| !identifiers(prerelease)) {
            return None;
        }
        let mut numbers = core.split(SEPARATOR).map(number);
        let (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) = (
            numbers.next(),
            numbers.next(),
            numbers.next(),
            numbers.next(),
        ) else {
            return None;
        };
        Some(Self {
            name: name.to_string(),
            prefix,
            major,
            minor,
            patch,
            prerelease: prerelease.map(str::to_string),
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn stable(&self) -> bool {
        self.prerelease.is_none()
    }

    pub(crate) fn next_patch(&self) -> String {
        format!(
            "{}{}.{}.{}",
            self.prefix,
            self.major,
            self.minor,
            self.patch.saturating_add(1)
        )
    }
}

impl Ord for Tag {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(
                || match (self.prerelease.as_deref(), other.prerelease.as_deref()) {
                    (None, None) => Ordering::Equal,
                    (None, Some(_)) => Ordering::Greater,
                    (Some(_), None) => Ordering::Less,
                    (Some(left), Some(right)) => precedence(left, right),
                },
            )
            .then_with(|| self.name.cmp(&other.name))
    }
}

impl PartialOrd for Tag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn number(part: &str) -> Option<u64> {
    let canonical = !part.is_empty()
        && part.bytes().all(|byte| byte.is_ascii_digit())
        && (part == ZERO || !part.starts_with(ZERO));
    canonical.then(|| part.parse().ok()).flatten()
}

fn identifiers(prerelease: &str) -> bool {
    prerelease.split(SEPARATOR).all(|identifier| {
        !identifier.is_empty()
            && identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn precedence(left: &str, right: &str) -> Ordering {
    let mut left = left.split(SEPARATOR);
    let mut right = right.split(SEPARATOR);
    loop {
        let ordering = match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left), Some(right)) => identifier(left, right),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
}

fn identifier(left: &str, right: &str) -> Ordering {
    match (left.parse::<u64>(), right.parse::<u64>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => left.cmp(right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str) -> Tag {
        Tag::parse(name).unwrap_or_else(|| panic!("{name} is a version tag"))
    }

    #[test]
    fn a_version_with_or_without_its_prefix_is_a_tag() {
        for name in [
            "v1.2.3",
            "1.2.3",
            "v0.0.0",
            "v1.2.3-rc.1",
            "v10.20.30-beta-2",
        ] {
            assert_eq!(tag(name).name(), name);
        }
    }

    #[test]
    fn anything_but_a_three_part_version_is_refused() {
        for name in [
            "",
            "v",
            "v1",
            "v1.2",
            "v1.2.3.4",
            "v1.2.x",
            "v01.2.3",
            "release-1.2.3",
            "V1.2.3",
            "v1.2.3-",
            "v1.2.3-rc..1",
            "v1.2.3+build",
            " v1.2.3",
            "pkg/v1.2.3",
        ] {
            assert!(Tag::parse(name).is_none(), "{name:?}");
        }
    }

    #[test]
    fn a_prerelease_is_not_stable() {
        assert!(tag("v1.2.3").stable());
        assert!(!tag("v1.2.3-rc.1").stable());
    }

    #[test]
    fn the_next_patch_keeps_the_prefix() {
        assert_eq!(tag("v1.2.3").next_patch(), "v1.2.4");
        assert_eq!(tag("1.9.9").next_patch(), "1.9.10");
        assert_eq!(tag("v2.0.0-rc.1").next_patch(), "v2.0.1");
    }

    #[test]
    fn versions_order_numerically_and_a_prerelease_before_its_release() {
        let mut tags = [
            "v1.10.0",
            "v1.2.0",
            "v1.10.0-rc.2",
            "v1.10.0-rc.10",
            "v1.10.0-alpha",
            "v1.10.0-rc",
            "v1.9.9",
        ]
        .map(tag);
        tags.sort();
        let names: Vec<&str> = tags.iter().map(Tag::name).collect();
        assert_eq!(
            names,
            [
                "v1.2.0",
                "v1.9.9",
                "v1.10.0-alpha",
                "v1.10.0-rc",
                "v1.10.0-rc.2",
                "v1.10.0-rc.10",
                "v1.10.0",
            ]
        );
    }
}
