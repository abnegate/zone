use std::cmp::Ordering;

use abnegate_vcs::BranchName;

const MARKER: &str = "v";
const UNMARKED: &str = "";
const PREFIX_SEPARATOR: char = '/';
const SEPARATOR: char = '.';
const PRERELEASE: char = '-';
const ZERO: &str = "0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tag {
    name: String,
    prefix: Option<String>,
    marker: &'static str,
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<String>,
}

impl Tag {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        if BranchName::parse(name).is_err() {
            return None;
        }
        let (prefix, versioned) = match name.rsplit_once(PREFIX_SEPARATOR) {
            Some((prefix, versioned)) if !prefix.is_empty() => (Some(prefix), versioned),
            Some(_) => return None,
            None => (None, name),
        };
        let (marker, version) = match versioned.strip_prefix(MARKER) {
            Some(version) => (MARKER, version),
            None => (UNMARKED, versioned),
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
            prefix: prefix.map(str::to_string),
            marker,
            major,
            minor,
            patch,
            prerelease: prerelease.map(str::to_string),
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    pub(crate) fn stable(&self) -> bool {
        self.prerelease.is_none()
    }

    pub(crate) fn newer_than(&self, other: &Self) -> bool {
        self.version(other) == Ordering::Greater
    }

    pub(crate) fn next_patch(&self) -> String {
        format!(
            "{}{}{}.{}.{}",
            lead(self.prefix()),
            self.marker,
            self.major,
            self.minor,
            self.patch.saturating_add(1)
        )
    }

    pub(crate) fn version(&self, other: &Self) -> Ordering {
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
    }
}

impl Ord for Tag {
    fn cmp(&self, other: &Self) -> Ordering {
        self.version(other).then_with(|| self.name.cmp(&other.name))
    }
}

impl PartialOrd for Tag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(crate) fn lead(prefix: Option<&str>) -> String {
    prefix.map_or_else(String::new, |prefix| format!("{prefix}{PREFIX_SEPARATOR}"))
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
    fn a_version_with_or_without_its_marker_or_a_prefix_is_a_tag() {
        for (name, prefix) in [
            ("v1.2.3", None),
            ("1.2.3", None),
            ("v0.0.0", None),
            ("v1.2.3-rc.1", None),
            ("v10.20.30-beta-2", None),
            ("abnegate-vcs/v0.2.1", Some("abnegate-vcs")),
            (
                "crates/abnegate-vcs/0.2.1-rc.1",
                Some("crates/abnegate-vcs"),
            ),
        ] {
            let parsed = tag(name);
            assert_eq!(parsed.name(), name);
            assert_eq!(parsed.prefix(), prefix, "{name}");
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
            "/v1.2.3",
            "pkg//v1.2.3",
            "pkg/",
            "pkg/v1.2",
            "pkg..x/v1.2.3",
            "pkg name/v1.2.3",
        ] {
            assert!(Tag::parse(name).is_none(), "{name:?}");
        }
    }

    #[test]
    fn a_prerelease_is_not_stable() {
        assert!(tag("v1.2.3").stable());
        assert!(!tag("v1.2.3-rc.1").stable());
        assert!(!tag("pkg/v1.2.3-rc.1").stable());
    }

    #[test]
    fn the_next_patch_keeps_the_prefix_and_marker() {
        assert_eq!(tag("v1.2.3").next_patch(), "v1.2.4");
        assert_eq!(tag("1.9.9").next_patch(), "1.9.10");
        assert_eq!(tag("v2.0.0-rc.1").next_patch(), "v2.0.1");
        assert_eq!(
            tag("abnegate-vcs/v0.2.1").next_patch(),
            "abnegate-vcs/v0.2.2"
        );
    }

    #[test]
    fn newer_compares_versions_alone() {
        assert!(tag("v1.2.4").newer_than(&tag("v1.2.3")));
        assert!(!tag("v1.2.3").newer_than(&tag("v1.2.3")));
        assert!(!tag("v1.2.3-rc.1").newer_than(&tag("v1.2.3")));
        assert!(tag("pkg/v1.0.0").newer_than(&tag("v0.9.0")));
        assert!(!tag("1.2.3").newer_than(&tag("v1.2.3")));
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
