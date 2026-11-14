use super::unavailable::Unavailable;

const CONFIGURATION_INVALID: &str = "The GitHub source configuration is invalid.";
pub(crate) const NOT_GITHUB: &str = "Not a GitHub source.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unconnected {
    Unavailable(Unavailable),
    InvalidConfiguration,
}

impl Unconnected {
    pub(crate) fn message(self, not_github: &'static str) -> &'static str {
        match self {
            Self::Unavailable(unavailable) => unavailable.message(not_github),
            Self::InvalidConfiguration => CONFIGURATION_INVALID,
        }
    }
}

impl From<Unavailable> for Unconnected {
    fn from(unavailable: Unavailable) -> Self {
        Self::Unavailable(unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_unconnected_source_reads_as_what_it_is() {
        for (unconnected, expected) in [
            (
                Unconnected::Unavailable(Unavailable::Unreadable),
                "Could not read the source.",
            ),
            (
                Unconnected::Unavailable(Unavailable::Missing),
                "Source not found in this workspace or inactive.",
            ),
            (Unconnected::Unavailable(Unavailable::NotGithub), NOT_GITHUB),
            (
                Unconnected::InvalidConfiguration,
                "The GitHub source configuration is invalid.",
            ),
        ] {
            assert_eq!(unconnected.message(NOT_GITHUB), expected, "{unconnected:?}");
        }
    }
}
