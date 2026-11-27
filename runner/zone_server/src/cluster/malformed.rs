use std::fmt;

use serde_json::error::Category;

/// Why a payload from another replica or from Valkey could not be read, with
/// none of the payload in it: serde_json quotes the fragments it fails on, and
/// what replicas tell each other is sealed for a reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Malformed {
    category: Category,
    line: usize,
    column: usize,
}

impl From<serde_json::Error> for Malformed {
    fn from(error: serde_json::Error) -> Self {
        Self {
            category: error.classify(),
            line: error.line(),
            column: error.column(),
        }
    }
}

impl fmt::Display for Malformed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let reason = match self.category {
            Category::Io => "could not be read",
            Category::Syntax => "is not JSON",
            Category::Data => "does not have the expected shape",
            Category::Eof => "ends early",
        };
        write!(
            formatter,
            "the payload {reason} (line {}, column {})",
            self.line, self.column
        )
    }
}

impl std::error::Error for Malformed {}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct Count {
        #[serde(rename = "count")]
        _count: u64,
    }

    #[test]
    fn a_payload_of_the_wrong_shape_is_described_without_quoting_it() {
        let payload = br#"{"count": "hunter2-secret"}"#;
        let error = serde_json::from_slice::<Count>(payload).unwrap_err();
        assert!(
            error.to_string().contains("hunter2-secret"),
            "serde_json quotes the payload: {error}"
        );

        let malformed = Malformed::from(error);

        let described = format!("{malformed} {malformed:?}");
        assert!(!described.contains("hunter2"), "{described}");
        assert_eq!(
            malformed.to_string(),
            "the payload does not have the expected shape (line 1, column 26)"
        );
    }

    #[test]
    fn a_payload_that_is_not_json_or_ends_early_says_which_and_nothing_more() {
        let garbage = Malformed::from(serde_json::from_slice::<Count>(b"secret").unwrap_err());
        let cut = Malformed::from(serde_json::from_slice::<Count>(br#"{"count": 1"#).unwrap_err());

        assert!(
            garbage.to_string().starts_with("the payload is not JSON"),
            "{garbage}"
        );
        assert!(
            cut.to_string().starts_with("the payload ends early"),
            "{cut}"
        );
        assert!(!garbage.to_string().contains("secret"), "{garbage}");
    }
}
