use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Renewed,
    Current,
    Ended,
    Refused,
    Rejected,
    Unreachable,
}

impl Outcome {
    pub const ALL: [Self; 6] = [
        Self::Renewed,
        Self::Current,
        Self::Ended,
        Self::Refused,
        Self::Rejected,
        Self::Unreachable,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Renewed => "renewed",
            Self::Current => "current",
            Self::Ended => "ended",
            Self::Refused => "refused",
            Self::Rejected => "rejected",
            Self::Unreachable => "unreachable",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|outcome| outcome.as_str() == value)
    }
}

#[cfg(test)]
mod tests;
