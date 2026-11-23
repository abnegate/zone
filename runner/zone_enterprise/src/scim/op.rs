use serde::de::{self, Unexpected};
use serde::{Deserialize, Deserializer};

const EXPECTED: &str = "add, replace or remove";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Op {
    Add,
    Replace,
    Remove,
}

impl Op {
    pub const ALL: [Self; 3] = [Self::Add, Self::Replace, Self::Remove];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Replace => "replace",
            Self::Remove => "remove",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|op| op.as_str().eq_ignore_ascii_case(value))
    }
}

impl<'de> Deserialize<'de> for Op {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value)
            .ok_or_else(|| de::Error::invalid_value(Unexpected::Str(&value), &EXPECTED))
    }
}
