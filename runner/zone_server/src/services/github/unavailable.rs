const UNREADABLE: &str = "Could not read the source.";
const MISSING: &str = "Source not found in this workspace or inactive.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unavailable {
    Unreadable,
    Missing,
    NotGithub,
}

impl Unavailable {
    pub(crate) fn message(self, not_github: &'static str) -> &'static str {
        match self {
            Self::Unreadable => UNREADABLE,
            Self::Missing => MISSING,
            Self::NotGithub => not_github,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOT_GITHUB: &str = "Not a GitHub source.";

    #[test]
    fn each_unavailable_source_reads_as_what_it_is() {
        for (unavailable, expected) in [
            (Unavailable::Unreadable, "Could not read the source."),
            (
                Unavailable::Missing,
                "Source not found in this workspace or inactive.",
            ),
            (Unavailable::NotGithub, NOT_GITHUB),
        ] {
            assert_eq!(unavailable.message(NOT_GITHUB), expected, "{unavailable:?}");
        }
    }
}
