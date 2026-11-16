const OPEN: &str = "open";
const MERGED: &str = "merged";
const CLOSED: &str = "closed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Open,
    Merged,
    Closed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => OPEN,
            Self::Merged => MERGED,
            Self::Closed => CLOSED,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            OPEN => Some(Self::Open),
            MERGED => Some(Self::Merged),
            CLOSED => Some(Self::Closed),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_reads_back_what_it_writes() {
        for status in [Status::Open, Status::Merged, Status::Closed] {
            assert_eq!(Status::parse(status.as_str()), Some(status));
        }
    }

    #[test]
    fn every_status_writes_the_string_already_stored() {
        assert_eq!(
            [Status::Open, Status::Merged, Status::Closed].map(Status::as_str),
            ["open", "merged", "closed"],
            "these strings are stored in tasks.pr_status and read by the console, so renaming \
             one orphans every row and every console build that already holds it"
        );
    }

    #[test]
    fn an_unknown_status_is_not_parsed() {
        assert_eq!(Status::parse("draft"), None);
        assert_eq!(Status::parse("Merged"), None);
    }
}
