//! The kinds of quality signal an evaluation tool produces.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalCategory {
    Test,
    Lint,
    Typecheck,
    Build,
    Coverage,
}

impl EvalCategory {
    pub const ALL: [EvalCategory; 5] = [
        EvalCategory::Test,
        EvalCategory::Lint,
        EvalCategory::Typecheck,
        EvalCategory::Build,
        EvalCategory::Coverage,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EvalCategory::Test => "test",
            EvalCategory::Lint => "lint",
            EvalCategory::Typecheck => "typecheck",
            EvalCategory::Build => "build",
            EvalCategory::Coverage => "coverage",
        }
    }
}

impl fmt::Display for EvalCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_every_category_as_a_stable_identifier() {
        let rendered: Vec<&str> = EvalCategory::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            rendered,
            vec!["test", "lint", "typecheck", "build", "coverage"],
            "category identifiers are persisted in run artifacts and must stay stable"
        );
    }

    #[test]
    fn round_trips_through_serde() {
        for category in EvalCategory::ALL {
            let encoded = serde_json::to_string(&category).expect("category serialises");
            let decoded: EvalCategory =
                serde_json::from_str(&encoded).expect("category deserialises");
            assert_eq!(decoded, category);
            assert_eq!(encoded, format!("\"{}\"", category));
        }
    }
}
