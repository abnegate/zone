use std::fmt;

const SUFFIX: &str = "-lts";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BundleVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl BundleVersion {
    pub fn parse(text: &str) -> Option<Self> {
        let mut numbers = text.strip_suffix(SUFFIX)?.split('.');
        let version = Self {
            major: number(numbers.next()?)?,
            minor: number(numbers.next()?)?,
            patch: number(numbers.next()?)?,
        };
        numbers.next().is_none().then_some(version)
    }
}

fn number(text: &str) -> Option<u64> {
    let digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let padded = text.len() > 1 && text.starts_with('0');
    if digits && !padded {
        text.parse().ok()
    } else {
        None
    }
}

impl fmt::Display for BundleVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}.{}.{}{SUFFIX}",
            self.major, self.minor, self.patch
        )
    }
}
